// Copyright 2026 The Android Open Source Project
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

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use bytes::Bytes;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

const MDNS_IP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;
const MDNS_MULTICAST_MAC: [u8; 6] = [0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB];
const ETHER_TYPE_IPV4: u16 = 0x0800;
const IPV4_VERSION_IHL: u8 = 0x45;
const DEFAULT_TTL: u8 = 64;
const IPPROTO_UDP: u8 = 17;

#[repr(C, packed)]
struct Ipv4Header {
    version_ihl: u8,
    dscp_ecn: u8,
    total_length: u16,
    identification: u16,
    flags_fragment_offset: u16,
    time_to_live: u8,
    protocol: u8,
    header_checksum: u16,
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
}

impl Ipv4Header {
    fn new_mdns(payload_len: usize, ip_addr: &Ipv4Addr) -> Self {
        let mut header = Self {
            version_ihl: IPV4_VERSION_IHL,
            dscp_ecn: 0,
            total_length: (payload_len + UDP_HEADER_LEN + IPV4_HEADER_LEN) as u16,
            identification: 0,
            flags_fragment_offset: 0,
            time_to_live: DEFAULT_TTL,
            protocol: IPPROTO_UDP,
            header_checksum: 0,
            source_ip: ip_addr.octets(),
            destination_ip: MDNS_IP.octets(),
        };
        header.update_checksum();
        header
    }

    fn calculate_checksum(&self) -> u16 {
        let [s0, s1, s2, s3] = self.source_ip;
        let [d0, d1, d2, d3] = self.destination_ip;
        let mut sum: u32 = [
            u16::from_be_bytes([self.version_ihl, self.dscp_ecn]),
            self.total_length,
            self.identification,
            self.flags_fragment_offset,
            u16::from_be_bytes([self.time_to_live, self.protocol]),
            self.header_checksum,
            u16::from_be_bytes([s0, s1]),
            u16::from_be_bytes([s2, s3]),
            u16::from_be_bytes([d0, d1]),
            u16::from_be_bytes([d2, d3]),
        ]
        .into_iter()
        .map(u32::from)
        .sum();

        while sum > u32::from(u16::MAX) {
            sum = (sum >> 16) + u32::from(sum as u16);
        }
        !(sum as u16)
    }

    fn update_checksum(&mut self) {
        self.header_checksum = 0;
        self.header_checksum = self.calculate_checksum();
    }

    fn bytes(&self) -> impl Iterator<Item = u8> {
        [self.version_ihl, self.dscp_ecn]
            .into_iter()
            .chain(self.total_length.to_be_bytes())
            .chain(self.identification.to_be_bytes())
            .chain(self.flags_fragment_offset.to_be_bytes())
            .chain([self.time_to_live, self.protocol])
            .chain(self.header_checksum.to_be_bytes())
            .chain(self.source_ip)
            .chain(self.destination_ip)
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
    fn new_mdns(payload_len: usize) -> Self {
        Self {
            source_port: MDNS_PORT,
            destination_port: MDNS_PORT,
            length: (payload_len + UDP_HEADER_LEN) as u16,
            checksum: 0,
        }
    }

    fn bytes(&self) -> impl Iterator<Item = u8> {
        self.source_port
            .to_be_bytes()
            .into_iter()
            .chain(self.destination_port.to_be_bytes())
            .chain(self.length.to_be_bytes())
            .chain(self.checksum.to_be_bytes())
    }
}

#[repr(C, packed)]
struct EtherHeader {
    ether_dhost: [u8; 6],
    ether_shost: [u8; 6],
    ether_type: u16,
}

impl EtherHeader {
    fn new_mdns() -> Self {
        Self {
            ether_dhost: MDNS_MULTICAST_MAC,
            ether_shost: MDNS_MULTICAST_MAC,
            ether_type: ETHER_TYPE_IPV4,
        }
    }

    fn create_mdns_frame(packet: &[u8], ip_addr: &Ipv4Addr) -> Result<Vec<u8>, String> {
        let ether_header = Self::new_mdns();
        let udp_header = UdpHeader::new_mdns(packet.len());
        let ipv4_header = Ipv4Header::new_mdns(packet.len(), ip_addr);

        Ok(ether_header
            .bytes()
            .chain(ipv4_header.bytes())
            .chain(udp_header.bytes())
            .chain(packet.iter().copied())
            .collect())
    }

    fn bytes(&self) -> impl Iterator<Item = u8> {
        self.ether_dhost.into_iter().chain(self.ether_shost).chain(self.ether_type.to_be_bytes())
    }
}

const UDP_HEADER_LEN: usize = std::mem::size_of::<UdpHeader>();
const IPV4_HEADER_LEN: usize = std::mem::size_of::<Ipv4Header>();

pub(crate) async fn run_mdns_forwarder(tx: UnboundedSender<Bytes>) -> Result<(), String> {
    // Setup socket using socket2 for REUSEPORT/REUSEADDR and JoinMulticast
    let domain = socket2::Domain::IPV4;
    let socket = socket2::Socket::new(domain, socket2::Type::DGRAM, Some(socket2::Protocol::UDP))
        .map_err(|e| format!("create socket failed: {:?}", e))?;

    socket.set_reuse_address(true).map_err(|e| format!("set ReuseAddr failed: {:?}", e))?;
    #[cfg(not(windows))]
    socket.set_reuse_port(true).map_err(|e| format!("set ReusePort failed: {:?}", e))?;

    socket
        .join_multicast_v4(&MDNS_IP, &Ipv4Addr::UNSPECIFIED)
        .map_err(|e| format!("join_multicast_v4 failed: {:?}", e))?;
    socket
        .set_multicast_loop_v4(false)
        .map_err(|e| format!("set_multicast_loop_v4 failed: {:?}", e))?;

    let addr: SocketAddr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MDNS_PORT).into();
    socket.bind(&addr.into()).map_err(|e| format!("socket bind to {} failed: {:?}", &addr, e))?;

    // Convert to tokio socket
    let socket = std::net::UdpSocket::from(socket);
    let socket = tokio::net::UdpSocket::from_std(socket)
        .map_err(|e| format!("Failed to convert to tokio socket: {:?}", e))?;

    let mut buf = [0u8; 1500];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((size, src_addr)) => {
                let Some(packet) = buf.get(..size) else {
                    continue;
                };
                if let SocketAddr::V4(socket_addr_v4) = src_addr {
                    debug!("Received {} bytes from {:?}", packet.len(), socket_addr_v4);
                    match EtherHeader::create_mdns_frame(packet, socket_addr_v4.ip()) {
                        Ok(ethernet_frame) => {
                            if tx.send(Bytes::from(ethernet_frame)).is_err() {
                                warn!("Failed to send packet to channel (receiver closed)");
                                break;
                            }
                        }
                        Err(error) => warn!(%error, "Failed to create ethernet frame"),
                    }
                } else {
                    warn!("Forwarding mDNS from IPv6 is not supported: {:?}", src_addr);
                }
            }
            Err(error) => {
                warn!(?error, "recv_from failed");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv4_header_checksum_calculation() {
        let mut header = Ipv4Header {
            version_ihl: IPV4_VERSION_IHL,
            dscp_ecn: 0,
            total_length: 64,
            identification: 0x1c46,
            flags_fragment_offset: 0,
            time_to_live: DEFAULT_TTL,
            protocol: IPPROTO_UDP,
            header_checksum: 0,
            source_ip: [172, 16, 0, 1],
            destination_ip: [224, 0, 0, 251],
        };
        header.update_checksum();
        let verify_sum = header.calculate_checksum();
        assert_eq!(verify_sum, 0);
    }

    #[test]
    fn test_create_mdns_frame() {
        let packet = b"sample mdns query payload";
        let ip_addr = Ipv4Addr::new(192, 168, 1, 100);
        let frame = EtherHeader::create_mdns_frame(packet, &ip_addr).expect("valid frame creation");

        // Ethernet header: 14 bytes (6 dhost + 6 shost + 2 ethertype)
        assert_eq!(&frame[0..6], &MDNS_MULTICAST_MAC);
        assert_eq!(&frame[6..12], &MDNS_MULTICAST_MAC);
        assert_eq!(&frame[12..14], &ETHER_TYPE_IPV4.to_be_bytes());

        // IPv4 header: 20 bytes
        assert_eq!(frame[14], IPV4_VERSION_IHL);
        assert_eq!(frame[22], DEFAULT_TTL);
        assert_eq!(frame[23], IPPROTO_UDP);
        assert_eq!(&frame[26..30], &ip_addr.octets());
        assert_eq!(&frame[30..34], &MDNS_IP.octets());

        // UDP header: 8 bytes
        assert_eq!(&frame[34..36], &MDNS_PORT.to_be_bytes());
        assert_eq!(&frame[36..38], &MDNS_PORT.to_be_bytes());
        assert_eq!(&frame[38..40], &((packet.len() + UDP_HEADER_LEN) as u16).to_be_bytes());

        // Payload: follows 14 + 20 + 8 = 42 bytes
        assert_eq!(&frame[42..], packet);
    }
}
