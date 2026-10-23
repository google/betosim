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
use netsim_packets::{IP_P_UDP, PacketBuilder, ether_type};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

const MDNS_IP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;
const MDNS_MULTICAST_MAC: [u8; 6] = [0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB];
const MDNS_SRC_MAC: [u8; 6] = [0x02, 0x15, 0xB2, 0x00, 0x00, 0x01];
const SLIRP_GATEWAY_IP: Ipv4Addr = Ipv4Addr::new(10, 0, 2, 2);

/// Rewrites host/container socket source IPv4 addresses to the SLIRP virtual
/// gateway IP (`10.0.2.2`) so guest Wi-Fi clients see an on-link sender whose
/// follow-up unicast responses route back to the host/container loopback.
fn resolve_mdns_source_ip(_src_ip: Ipv4Addr) -> Ipv4Addr {
    SLIRP_GATEWAY_IP
}

fn create_mdns_frame(packet: &[u8], ip_addr: Ipv4Addr) -> Vec<u8> {
    let src_ip = resolve_mdns_source_ip(ip_addr);
    PacketBuilder::new(MDNS_MULTICAST_MAC, MDNS_SRC_MAC, ether_type::IPV4)
        .ipv4(src_ip.octets(), MDNS_IP.octets(), IP_P_UDP)
        .udp(MDNS_PORT, MDNS_PORT)
        .payload(packet)
}

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
    socket.bind(&addr.into()).map_err(|e| format!("socket bind to {} failed: {:?}", addr, e))?;

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
                    let ethernet_frame = create_mdns_frame(packet, *socket_addr_v4.ip());
                    if tx.send(Bytes::from(ethernet_frame)).is_err() {
                        warn!("Failed to send packet to channel (receiver closed)");
                        break;
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
    use netsim_packets::{EthernetFrame, Ipv4Header, UdpHeader, ipv4_checksum, udp_checksum};

    use super::*;

    #[test]
    fn test_create_mdns_frame() {
        let packet = b"sample mdns query payload";
        let ip_addr = Ipv4Addr::new(192, 168, 1, 100);
        let frame = create_mdns_frame(packet, ip_addr);

        let (eth, ip_bytes) = EthernetFrame::parse(&frame).expect("valid ethernet frame");
        assert_eq!(eth.dst_addr.bytes, MDNS_MULTICAST_MAC);
        assert_eq!(eth.src_addr.bytes, MDNS_SRC_MAC);
        assert_eq!(eth.src_addr.bytes[0] & 0x01, 0);
        assert!(!eth.src_addr.is_multicast());
        assert_eq!(eth.ethertype.get(), ether_type::IPV4);

        let (ipv4, udp_bytes) = Ipv4Header::parse(ip_bytes).expect("valid ipv4 header");
        assert_eq!(ipv4.version(), 4);
        assert_eq!(ipv4.ihl(), 5);
        assert_eq!(ipv4.ttl, 64);
        assert_eq!(ipv4.protocol, IP_P_UDP);
        assert_eq!(ipv4.source_addr, SLIRP_GATEWAY_IP.octets());
        assert_eq!(ipv4.dest_addr, MDNS_IP.octets());
        assert_eq!(ipv4.total_length.get() as usize, ip_bytes.len());
        assert_ne!(ipv4.header_checksum.get(), 0);
        assert_eq!(ipv4_checksum(&ip_bytes[..ipv4.header_length()]), 0);

        let (udp, udp_payload) = UdpHeader::parse(udp_bytes).expect("valid udp header");
        assert_eq!(udp.source_port.get(), MDNS_PORT);
        assert_eq!(udp.dest_port.get(), MDNS_PORT);
        assert_eq!(udp.length.get() as usize, udp_bytes.len());
        assert_ne!(udp.checksum.get(), 0);
        assert_eq!(udp_checksum(udp_bytes, SLIRP_GATEWAY_IP, MDNS_IP), u16::MAX);
        assert_eq!(udp_payload, packet);
    }

    #[test]
    fn test_mdns_forwarder_isolates_container_source_ip_to_slirp_gateway() {
        let packet = b"container mdns announcement";
        for outer_ip in [
            Ipv4Addr::new(172, 17, 0, 2),
            Ipv4Addr::new(192, 168, 1, 50),
            Ipv4Addr::LOCALHOST,
            SLIRP_GATEWAY_IP,
        ] {
            let frame = create_mdns_frame(packet, outer_ip);
            let (_eth, ip_bytes) = EthernetFrame::parse(&frame).expect("valid ethernet frame");
            let (ipv4, udp_bytes) = Ipv4Header::parse(ip_bytes).expect("valid ipv4 header");
            assert_eq!(ipv4.source_addr, SLIRP_GATEWAY_IP.octets());
            assert_eq!(ipv4_checksum(&ip_bytes[..ipv4.header_length()]), 0);
            assert_eq!(udp_checksum(udp_bytes, SLIRP_GATEWAY_IP, MDNS_IP), u16::MAX);
        }
    }
}
