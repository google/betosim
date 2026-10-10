// Copyright 2024 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::wifi::error::{WifiError, WifiResult};
use bytes::Bytes;
use log::{debug, warn};
use socket2::{Protocol, Socket};
use std::mem::MaybeUninit;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::mpsc;

const MDNS_IP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;

struct MacAddress(u64);

impl MacAddress {
    fn to_be_bytes(&self) -> [u8; 6] {
        // NOTE: mac address is le
        self.0.to_le_bytes()[0..6].try_into().unwrap()
    }
}

impl From<MacAddress> for [u8; 6] {
    fn from(MacAddress(addr): MacAddress) -> Self {
        let bytes = u64::to_le_bytes(addr);
        bytes[0..6].try_into().unwrap()
    }
}

impl From<&[u8; 6]> for MacAddress {
    fn from(bytes: &[u8; 6]) -> Self {
        Self(u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], 0, 0]))
    }
}

#[repr(C, packed)]
struct Ipv4Header {
    version_ihl: u8, // 4 bits Version, 4 bits Internet Header Length
    dscp_ecn: u8, // 6 bits Differentiated Services Code Point, 2 bits Explicit Congestion Notification
    total_length: u16,
    identification: u16,
    flags_fragment_offset: u16, // 3 bits Flags, 13 bits Fragment Offset
    time_to_live: u8,
    protocol: u8,
    header_checksum: u16,
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
}

macro_rules! be_vec {
    ( $( $x:expr ),* ) => {
         Vec::<u8>::new().iter().copied()
         $( .chain($x.to_be_bytes()) )*
         .collect()
       };
    }

impl Ipv4Header {
    fn calculate_checksum(&self) -> u16 {
        let mut sum: u32 = 0;

        // Process fixed-size fields (first 20 bytes)
        let fixed_bytes: [u8; 20] = self.to_be_bytes();
        for i in 0..10 {
            let word = ((fixed_bytes[i * 2] as u16) << 8) | (fixed_bytes[i * 2 + 1] as u16);
            sum += word as u32;
        }

        // Handle carries (fold the carry into the sum)
        while (sum >> 16) > 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }

        // One's complement
        !sum as u16
    }

    fn update_checksum(&mut self) {
        self.header_checksum = 0; // Reset checksum before calculation
        self.header_checksum = self.calculate_checksum();
    }

    fn to_be_bytes(&self) -> [u8; 20] {
        let mut v: Vec<u8> = be_vec![
            self.version_ihl,
            self.dscp_ecn,
            self.total_length,
            self.identification,
            self.flags_fragment_offset,
            self.time_to_live,
            self.protocol,
            self.header_checksum
        ];
        v.extend(Ipv4Addr::from(self.source_ip).octets());
        v.extend(Ipv4Addr::from(self.destination_ip).octets());
        v.try_into().unwrap()
    }
}

#[repr(C, packed)]
struct UdpHeader {
    source_port: u16,
    destination_port: u16,
    length: u16,
    checksum: u16,
}

impl UdpHeader {
    fn to_be_bytes(&self) -> [u8; 8] {
        let v: Vec<u8> =
            be_vec![self.source_port, self.destination_port, self.length, self.checksum];
        v.try_into().unwrap()
    }
}

/* 10Mb/s ethernet header */

#[repr(C, packed)]
struct EtherHeader {
    ether_dhost: [u8; 6],
    ether_shost: [u8; 6],
    ether_type: u16,
}

/* Ethernet protocol ID's */
const ETHER_TYPE_IP: u16 = 0x0800;

impl EtherHeader {
    fn to_be_bytes(&self) -> [u8; 14] {
        let v: Vec<u8> = be_vec![
            MacAddress::from(&self.ether_dhost),
            MacAddress::from(&self.ether_shost),
            self.ether_type
        ];
        v.try_into().unwrap()
    }
}

// Define constants for header sizes (bytes)
const UDP_HEADER_LEN: usize = std::mem::size_of::<UdpHeader>();
const IPV4_HEADER_LEN: usize = std::mem::size_of::<Ipv4Header>();
const ETHER_HEADER_LEN: usize = std::mem::size_of::<EtherHeader>();

/// Creates a new UDP socket to bind to `port` with REUSEPORT option.
/// `non_block` indicates whether to set O_NONBLOCK for the socket.
fn new_socket(addr: SocketAddr, non_block: bool) -> WifiResult<Socket> {
    let domain = match addr {
        SocketAddr::V4(_) => socket2::Domain::IPV4,
        SocketAddr::V6(_) => socket2::Domain::IPV6,
    };

    let socket = Socket::new(domain, socket2::Type::DGRAM, Some(Protocol::UDP))
        .map_err(|e| WifiError::Network(format!("create socket failed: {:?}", e)))?;

    socket
        .set_reuse_address(true)
        .map_err(|e| WifiError::Network(format!("set ReuseAddr failed: {:?}", e)))?;
    #[cfg(not(windows))]
    socket.set_reuse_port(true)?;

    #[cfg(unix)] // this is currently restricted to Unix's in socket2
    socket
        .set_reuse_port(true)
        .map_err(|e| WifiError::Network(format!("set ReusePort failed: {:?}", e)))?;

    if non_block {
        socket
            .set_nonblocking(true)
            .map_err(|e| WifiError::Network(format!("set O_NONBLOCK: {:?}", e)))?;
    }

    socket
        .join_multicast_v4(&MDNS_IP, &Ipv4Addr::UNSPECIFIED)
        .map_err(|e| WifiError::Network(format!("join_multicast_v4 failed: {:?}", e)))?;
    socket.set_multicast_loop_v4(false).expect("set_multicast_loop_v4 call failed");

    socket
        .bind(&addr.into())
        .map_err(|e| WifiError::Network(format!("socket bind to {} failed: {:?}", &addr, e)))?;

    Ok(socket)
}

fn create_ethernet_frame(packet: &[u8], ip_addr: &Ipv4Addr) -> WifiResult<Vec<u8>> {
    // TODO: Use the etherparse crate
    let ether_header = EtherHeader {
        // mDNS multicast IP address
        ether_dhost: [0x01, 0x00, 0x5e, 0x00, 0x00, 0xfb],
        ether_shost: [0x01, 0x00, 0x5e, 0x00, 0x00, 0xfb],
        ether_type: ETHER_TYPE_IP,
    };

    // Create UDP Header
    let udp_header = UdpHeader {
        source_port: MDNS_PORT,
        destination_port: MDNS_PORT,
        length: (packet.len() + UDP_HEADER_LEN) as u16,
        // Usually 0 for mDNS
        checksum: 0,
    };

    // Create IPv4 Header
    let mut ipv4_header = Ipv4Header {
        version_ihl: 0x45,
        dscp_ecn: 0,
        total_length: (packet.len() + UDP_HEADER_LEN + IPV4_HEADER_LEN) as u16,
        identification: 0,
        flags_fragment_offset: 0,
        time_to_live: 64,
        protocol: 17,
        header_checksum: 0,
        source_ip: ip_addr.octets(),
        // mDNS multicast
        destination_ip: MDNS_IP.octets(),
    };
    ipv4_header.update_checksum();

    // Combine Headers and Payload (Safely using Vec)
    let mut response_packet =
        Vec::with_capacity(ETHER_HEADER_LEN + IPV4_HEADER_LEN + UDP_HEADER_LEN + packet.len());
    response_packet.extend_from_slice(&ether_header.to_be_bytes());
    response_packet.extend_from_slice(&ipv4_header.to_be_bytes());
    response_packet.extend_from_slice(&udp_header.to_be_bytes());
    response_packet.extend_from_slice(packet);

    Ok(response_packet)
}

pub fn run_mdns_forwarder(tx: mpsc::Sender<Bytes>) -> WifiResult<()> {
    let addr = SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), MDNS_PORT);
    let socket = new_socket(addr.into(), false)?;

    // Typical max mDNS packet size
    let mut buf: [MaybeUninit<u8>; 1500] = [MaybeUninit::new(0_u8); 1500];
    loop {
        let (size, src_addr) = socket
            .recv_from(&mut buf[..])
            .map_err(|e| WifiError::Network(format!("recv_from failed: {:?}", e)))?;
        // SAFETY: `recv_from` implementation promises not to write uninitialized bytes to `buf`.
        // Documentation: https://docs.rs/socket2/latest/socket2/struct.Socket.html#method.recv_from
        let packet = unsafe { &*(&buf[..size] as *const [MaybeUninit<u8>] as *const [u8]) };
        if let Some(socket_addr_v4) = src_addr.as_socket_ipv4() {
            debug!("Received {} bytes from {:?}", packet.len(), socket_addr_v4);
            match create_ethernet_frame(packet, socket_addr_v4.ip()) {
                Ok(ethernet_frame) => {
                    if let Err(e) = tx.send(ethernet_frame.into()) {
                        warn!("Failed to send packet: {}", e);
                    }
                }
                Err(e) => warn!("Failed to create ethernet frame from UDP payload: {}", e),
            };
        } else {
            warn!("Forwarding mDNS from IPv6 is not supported: {:?}", src_addr);
        }
    }
}
