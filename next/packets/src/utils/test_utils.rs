// Copyright 2025 The Android Open Source Project
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

// Test assertion utilities re-exported for integration tests.
#![allow(clippy::panic)]

use std::{
    net::{Ipv4Addr, Ipv6Addr},
    path::Path,
};

use zerocopy::{IntoBytes, U16, U32};

use crate::{
    ethernet::{
        ArpPacket, EthernetFrame, MacAddr,
        frame::{arp_hardware, arp_op, ether_type},
    },
    icmp::{
        NDP_HOP_LIMIT,
        v6::{Icmpv6Header, Icmpv6Type},
    },
    ip::frame::{IP_P_ICMPV6, Ipv4Header, Ipv6Header},
    packet,
    packet::json::to_json,
    transport::{tcp::TcpHeader, udp::UdpHeader},
    utils::checksum::{
        icmpv6_checksum, ipv4_checksum, tcp_checksum, tcp_checksum_v6, udp_checksum,
        udp_checksum_v6,
    },
};

/// Validates that the JSON output from parsing a PCAP file matches the golden
/// JSON file.
///
/// # Arguments
///
/// * `pcap_path` - Path to the PCAP file.
/// * `json_path` - Path to the golden JSON file (expected output from tshark).
/// * `layer_name` - The specific layer to validate (e.g., "icmp", "tcp").
pub fn validate_pcap_json(pcap_bytes: &'static [u8], json_str: &'static str, fields: &[&str]) {
    // Read golden JSON
    let tshark_json: serde_json::Value =
        serde_json::from_str(json_str).expect("Failed to parse JSON string");

    // Read PCAP file
    let reader_input = std::io::Cursor::new(pcap_bytes);
    let mut reader =
        crate::pcap::PcapReader::new(reader_input).expect("Failed to create PcapReader");

    // Read first record
    let (_, packet_data) =
        reader.next_record().expect("Failed to read PCAP record").expect("No records in PCAP");

    // Get LinkType (might be None if PCAPNG and no IDB yet, but usually IDB is
    // first)
    let link_type = reader
        .link_type()
        .expect("Unknown LinkType: PCAPNG file missing Interface Description Block?");

    let netsim_json = match link_type {
        crate::pcap::frame::LINKTYPE_IEEE802_11 => {
            let packet = crate::ieee80211::Ieee80211::decode(&packet_data)
                .expect("Failed to parse 802.11 packet");
            crate::ieee80211::json::to_json(&packet, packet_data.len())
        }
        crate::pcap::frame::LINKTYPE_ETHERNET => {
            let packet = packet::parse(&packet_data).expect("Failed to parse packet");
            to_json(&packet, packet_data.len())
        }
        crate::pcap::frame::LINKTYPE_RADIOTAP => {
            // Parse Radiotap header to find length
            use crate::pcap::radiotap::RadiotapHeader;

            if let Ok((header, _)) =
                zerocopy::Ref::<&[u8], RadiotapHeader>::from_prefix(&packet_data)
            {
                let len = header.len as usize;
                if let Some(inner_data) = packet_data.get(len..) {
                    // Assume inner is 802.11
                    let packet = crate::ieee80211::Ieee80211::decode(inner_data)
                        .expect("Failed to parse inner 802.11 packet from Radiotap");
                    crate::ieee80211::json::to_json(&packet, packet_data.len())
                } else {
                    panic!(
                        "Radiotap header length {} exceeds packet length {}",
                        len,
                        packet_data.len()
                    );
                }
            } else {
                panic!("Failed to parse Radiotap header");
            }
        }
        crate::pcap::frame::LINKTYPE_NETLINK => {
            // Netlink parsing
            crate::netlink::nl80211_json::packet_to_json(&packet_data)
        }
        _ => panic!("Unsupported LinkType: {}", link_type),
    };

    let Some(netsim_source) = netsim_json.pointer("/0/_source/layers") else {
        panic!("netsim_json missing _source.layers");
    };
    let Some(tshark_source) =
        tshark_json.get(0).and_then(|v| v.get("_source")).and_then(|v| v.get("layers"))
    else {
        panic!("tshark_json missing _source.layers");
    };

    // Compare specific fields
    for field in fields {
        let n_val = find_value(netsim_source, field);
        let t_val = find_value(tshark_source, field);

        match (n_val, t_val) {
            (Some(n), Some(t)) => {
                if !compare_values(n, t) {
                    panic!("Field mismatch: {} (netsim: {:?}, tshark: {:?})", field, n, t);
                }
            }
            (None, Some(_)) => panic!("Field missing in netsim: {}", field),
            (Some(_), None) => panic!("Field missing in tshark: {}", field),
            (None, None) => panic!("Field missing in both: {}", field),
        }
    }
}

fn find_value<'a>(layers: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    if let Some(obj) = layers.as_object() {
        if let Some(v) = obj.get(key) {
            return Some(v);
        }
        for v in obj.values() {
            if let Some(res) = find_value(v, key) {
                return Some(res);
            }
        }
    }
    None
}

fn compare_json_objects(netsim: &serde_json::Value, tshark: &serde_json::Value, path: &Path) {
    match netsim {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                // Skip "frame" layer metadata that we can't reproduce exactly (time, numbering)
                // But we DO want to validate frame.len, frame.cap_len, frame.protocols
                if key == "frame" {
                    if let Some(t_frame) = tshark.get("frame") {
                        // Validate specific fields
                        let fields_to_check = ["frame.len", "frame.cap_len", "frame.protocols"];
                        for field in fields_to_check {
                            if let Some(n_val) = val.get(field)
                                && let Some(t_val) = t_frame.get(field)
                                && !compare_values(n_val, t_val)
                            {
                                panic!(
                                    "Frame field mismatch: {} (netsim: {:?}, tshark: {:?})",
                                    field, n_val, t_val
                                );
                            }
                        }
                    }
                    continue;
                }

                // Special handling for keys that might be nested in tshark output
                // or named slightly differently.
                // We search for the key recursively in tshark object.
                let found = find_value(tshark, key); // Use the new find_value
                if let Some(t_val) = found {
                    if !compare_values(val, t_val) {
                        panic!("Field '{}' mismatch in tshark output. Path: {:?}", key, path);
                    }
                } else {
                    // If not found, it's a failure unless it's a known extra field
                    if key.ends_with("_hex") {
                        // Skip hex fields if not found (internal representation)
                        continue;
                    }
                    panic!("Field '{}' not found in tshark output. Path: {:?}", key, path);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            // For arrays, we expect tshark to have a matching array?
            // Or maybe netsim array corresponds to something else.
            // Currently netsim output is mostly objects.
            // If we have an array, we try to match by index.
            if let serde_json::Value::Array(t_arr) = tshark {
                assert_eq!(arr.len(), t_arr.len(), "Array length mismatch at {:?}", path);
                for (val, t_val) in arr.iter().zip(t_arr) {
                    compare_json_objects(val, t_val, path);
                }
            } else {
                // Tshark might represent single-element array as object or vice versa?
                // For now, strict type check.
                panic!("Type mismatch at {:?}: expected Array", path);
            }
        }
        _ => {
            // Primitive values are compared in find_and_compare usually,
            // but if we reached here via array iteration:
            assert_eq!(netsim, tshark, "Value mismatch at {:?}", path);
        }
    }
}

fn compare_values(n_val: &serde_json::Value, t_val: &serde_json::Value) -> bool {
    match (n_val, t_val) {
        (serde_json::Value::String(n_str), serde_json::Value::String(t_str)) => {
            n_str.eq_ignore_ascii_case(t_str)
        }
        (serde_json::Value::Number(n_num), serde_json::Value::String(t_str)) => {
            // Tshark often stores numbers as strings (e.g. "0")
            // Try to parse t_str as number or convert n_num to string
            if let Ok(t_num) = t_str.parse::<f64>()
                && let Some(n_f64) = n_num.as_f64()
            {
                return (n_f64 - t_num).abs() < f64::EPSILON;
            }
            // Handle hex strings (e.g. "0x0003")
            if let Some(stripped) = t_str.strip_prefix("0x")
                && let Ok(t_int) = u64::from_str_radix(stripped, 16)
                && let Some(n_u64) = n_num.as_u64()
            {
                return n_u64 == t_int;
            }
            // Fallback: compare as strings
            n_num.to_string() == *t_str
        }
        (serde_json::Value::Bool(n_bool), serde_json::Value::String(t_str)) => {
            // Tshark might use "1"/"0" for bools?
            if *n_bool {
                t_str == "1" || t_str.eq_ignore_ascii_case("true")
            } else {
                t_str == "0" || t_str.eq_ignore_ascii_case("false")
            }
        }
        (serde_json::Value::Object(_), serde_json::Value::Object(_)) => {
            // If both are objects, we recurse
            compare_json_objects(n_val, t_val, Path::new(""));
            true
        }
        (n, t) => n == t,
    }
}

const IPV4_HEADER_LEN: usize = std::mem::size_of::<crate::ip::frame::Ipv4Header>();
const IPV6_HEADER_LEN: usize = std::mem::size_of::<crate::ip::frame::Ipv6Header>();

/// A chained builder for whole packets, intended for tests and control-plane
/// traffic.
///
/// Layers are appended in order and their length and checksum fields are filled
/// in by [`PacketBuilder::payload`], once the total size is known. That means a
/// caller cannot forget to set a length, and cannot leave a mandatory checksum
/// zeroed -- two mistakes that are invisible in a hex dump and have each caused
/// a silently-dropped packet in this tree.
///
/// ```
/// use netsim_packets::{IP_P_UDP, PacketBuilder, ether_type};
///
/// let packet = PacketBuilder::new([0xff; 6], [0x02; 6], ether_type::IPV4)
///     .ipv4([10, 0, 2, 15], [10, 0, 2, 2], IP_P_UDP)
///     .udp(68, 67)
///     .payload(b"hello");
/// // 14 Ethernet + 20 IPv4 + 8 UDP + 5 payload
/// assert_eq!(packet.len(), 47);
/// ```
///
/// This allocates, so it is not meant for the forwarding hot path; the
/// zero-copy `*Builder` types write into a caller-provided buffer instead.
#[derive(Default)]
pub struct PacketBuilder {
    buffer: Vec<u8>,
    /// Offset of the IPv4 header and its addresses, for length and checksums.
    ipv4: Option<(usize, Ipv4Addr, Ipv4Addr)>,
    /// Offset of the IPv6 header and its addresses, for length and checksums.
    ipv6: Option<(usize, Ipv6Addr, Ipv6Addr)>,
    /// Offset of the UDP header, whose length and checksum span the payload.
    udp: Option<usize>,
    /// Offset of the TCP header, whose checksum spans the payload.
    tcp: Option<usize>,
    /// Offset of the ICMPv6 header, whose checksum spans the payload.
    icmpv6: Option<usize>,
}

impl PacketBuilder {
    /// Creates a new PacketBuilder with Ethernet header.
    pub fn new(dst_mac: [u8; 6], src_mac: [u8; 6], ethertype: u16) -> Self {
        let eth_header =
            EthernetFrame::new(MacAddr::new(dst_mac), MacAddr::new(src_mac), ethertype);
        let mut buffer = Vec::new();
        buffer.extend_from_slice(eth_header.as_bytes());
        Self { buffer, ..Default::default() }
    }

    /// Adds an IPv4 header. `total_length` and the header checksum are filled
    /// in by [`Self::payload`].
    pub fn ipv4(mut self, src_ip: [u8; 4], dst_ip: [u8; 4], protocol: u8) -> Self {
        self.ipv4 = Some((self.buffer.len(), Ipv4Addr::from(src_ip), Ipv4Addr::from(dst_ip)));

        let ip_header = Ipv4Header {
            version_ihl: 0x45, // Ver 4, IHL 5
            dscp_ecn: 0,
            total_length: U16::new(0),
            identification: U16::new(1),
            flags_fragment_offset: U16::new(0),
            ttl: 64,
            protocol,
            header_checksum: U16::new(0),
            source_addr: src_ip,
            dest_addr: dst_ip,
        };
        self.buffer.extend_from_slice(ip_header.as_bytes());
        self
    }

    /// Adds an IPv6 header. `payload_length` is filled in by [`Self::payload`].
    pub fn ipv6(mut self, src_ip: [u8; 16], dst_ip: [u8; 16], next_header: u8) -> Self {
        self.ipv6 = Some((self.buffer.len(), Ipv6Addr::from(src_ip), Ipv6Addr::from(dst_ip)));

        let ip_header = Ipv6Header::new(src_ip, dst_ip, next_header);
        self.buffer.extend_from_slice(ip_header.as_bytes());
        self
    }

    /// Overrides the IPv6 hop limit, which NDP requires to be 255.
    ///
    /// Panics if no IPv6 header has been added.
    pub fn hop_limit(mut self, hop_limit: u8) -> Self {
        let (offset, ..) = self.ipv6.expect("hop_limit() requires an IPv6 header");
        // hop_limit is the 8th byte of the IPv6 header.
        let Some(byte) = self.buffer.get_mut(offset + 7) else {
            panic!("PacketBuilder buffer out of bounds");
        };
        *byte = hop_limit;
        self
    }

    /// Adds a UDP header. The length and checksum are filled in by
    /// [`Self::payload`].
    pub fn udp(mut self, src_port: u16, dst_port: u16) -> Self {
        self.udp = Some(self.buffer.len());

        let udp_header = UdpHeader {
            source_port: U16::new(src_port),
            dest_port: U16::new(dst_port),
            length: U16::new(0),
            checksum: U16::new(0),
        };
        self.buffer.extend_from_slice(udp_header.as_bytes());
        self
    }

    /// Adds a TCP header. The checksum is filled in by [`Self::payload`].
    pub fn tcp(
        mut self,
        src_port: u16,
        dst_port: u16,
        seq: u32,
        ack: u32,
        flags: u16,
        window: u16,
    ) -> Self {
        self.tcp = Some(self.buffer.len());

        let data_offset = 5; // 5 * 32-bit words = 20 bytes
        let data_offset_reserved_flags = (data_offset << 12) | (flags & 0x1FF);

        let tcp_header = TcpHeader {
            source_port: U16::new(src_port),
            dest_port: U16::new(dst_port),
            sequence_num: U32::new(seq),
            ack_num: U32::new(ack),
            data_offset_reserved_flags: U16::new(data_offset_reserved_flags),
            window_size: U16::new(window),
            checksum: U16::new(0),
            urgent_ptr: U16::new(0),
        };

        self.buffer.extend_from_slice(tcp_header.as_bytes());
        self
    }

    /// Adds an ICMPv6 header. `rest` is the four bytes following the checksum,
    /// which the message type defines: the flags for a Neighbor Advertisement,
    /// or the identifier and sequence number for an Echo.
    ///
    /// The checksum is mandatory for ICMPv6 and is filled in by
    /// [`Self::payload`].
    pub fn icmpv6(mut self, icmpv6_type: u8, icmpv6_code: u8, rest: [u8; 4]) -> Self {
        self.icmpv6 = Some(self.buffer.len());

        let icmpv6_header =
            Icmpv6Header { icmpv6_type, icmpv6_code, icmpv6_checksum: U16::new(0), rest };
        self.buffer.extend_from_slice(icmpv6_header.as_bytes());
        self
    }

    /// Adds an ARP header.
    pub fn arp(
        mut self,
        opcode: u16,
        sender_mac: [u8; 6],
        sender_ip: [u8; 4],
        target_mac: [u8; 6],
        target_ip: [u8; 4],
    ) -> Self {
        let arp_header = ArpPacket {
            hardware_type: U16::new(arp_hardware::ETHERNET),
            protocol_type: U16::new(ether_type::IPV4),
            hardware_addr_len: 6,
            protocol_addr_len: 4,
            opcode: U16::new(opcode),
            sender_hardware_addr: MacAddr::new(sender_mac),
            sender_protocol_addr: sender_ip,
            target_hardware_addr: MacAddr::new(target_mac),
            target_protocol_addr: target_ip,
        };
        self.buffer.extend_from_slice(arp_header.as_bytes());
        self
    }

    fn write_u16_be(&mut self, offset: usize, val: u16) {
        let Some(dest) = self.buffer.get_mut(offset..offset + 2) else {
            panic!("PacketBuilder buffer out of bounds");
        };
        dest.copy_from_slice(&val.to_be_bytes());
    }

    /// Appends `payload` and finalizes the packet, deriving every length and
    /// checksum field from the bytes that were actually written.
    pub fn payload(mut self, payload: &[u8]) -> Vec<u8> {
        self.buffer.extend_from_slice(payload);

        let total = self.buffer.len();

        // Transport layer first: the IPv4 header checksum covers total_length,
        // so the IP headers have to be patched last.
        if let Some(offset) = self.udp {
            let length = (total - offset) as u16;
            self.write_u16_be(offset + 4, length);

            let Some(udp_buf) = self.buffer.get(offset..) else {
                panic!("PacketBuilder buffer out of bounds");
            };
            let checksum = match (self.ipv4, self.ipv6) {
                (Some((_, src, dst)), _) => Some(udp_checksum(udp_buf, src, dst)),
                (_, Some((_, src, dst))) => Some(udp_checksum_v6(udp_buf, src, dst)),
                _ => None,
            };
            if let Some(checksum) = checksum {
                self.write_u16_be(offset + 6, checksum);
            }
        }

        if let Some(offset) = self.tcp {
            let Some(tcp_buf) = self.buffer.get(offset..) else {
                panic!("PacketBuilder buffer out of bounds");
            };
            let checksum = match (self.ipv4, self.ipv6) {
                (Some((_, src, dst)), _) => Some(tcp_checksum(tcp_buf, src, dst)),
                (_, Some((_, src, dst))) => Some(tcp_checksum_v6(tcp_buf, src, dst)),
                _ => None,
            };
            if let Some(checksum) = checksum {
                self.write_u16_be(offset + 16, checksum);
            }
        }

        if let Some(offset) = self.icmpv6 {
            // ICMPv6 checksums are mandatory; a zero one is silently dropped.
            let (_, src, dst) = self.ipv6.expect("icmpv6() requires an IPv6 header");
            let Some(icmpv6_buf) = self.buffer.get(offset..) else {
                panic!("PacketBuilder buffer out of bounds");
            };
            let checksum = icmpv6_checksum(icmpv6_buf, src, dst);
            self.write_u16_be(offset + 2, checksum);
        }

        if let Some((offset, ..)) = self.ipv4 {
            let total_length = (total - offset) as u16;
            self.write_u16_be(offset + 2, total_length);
            let Some(hdr_buf) = self.buffer.get(offset..offset + IPV4_HEADER_LEN) else {
                panic!("PacketBuilder buffer out of bounds");
            };
            let checksum = ipv4_checksum(hdr_buf);
            self.write_u16_be(offset + 10, checksum);
        }

        if let Some((offset, ..)) = self.ipv6 {
            let payload_length = (total - offset - IPV6_HEADER_LEN) as u16;
            self.write_u16_be(offset + 4, payload_length);
        }

        self.buffer
    }

    /// Finalizes a packet that carries no payload beyond its headers.
    pub fn build(self) -> Vec<u8> {
        self.payload(&[])
    }
}

pub fn build_arp_frame(
    src_mac: [u8; 6],
    sip: std::net::Ipv4Addr,
    tip: std::net::Ipv4Addr,
) -> bytes::Bytes {
    let frame = PacketBuilder::new([0xff; 6], src_mac, ether_type::ARP)
        .arp(arp_op::REQUEST, src_mac, sip.octets(), [0; 6], tip.octets())
        .build();
    bytes::Bytes::from(frame)
}

pub fn build_icmpv6_ns_frame(
    src_mac: [u8; 6],
    dst_mac: [u8; 6],
    src_ip: std::net::Ipv6Addr,
    dst_ip: std::net::Ipv6Addr,
    target_ip: std::net::Ipv6Addr,
) -> bytes::Bytes {
    let frame = PacketBuilder::new(dst_mac, src_mac, ether_type::IPV6)
        .ipv6(src_ip.octets(), dst_ip.octets(), IP_P_ICMPV6)
        .hop_limit(NDP_HOP_LIMIT)
        .icmpv6(Icmpv6Type::NeighborSolicitation as u8, 0, [0, 0, 0, 0])
        .payload(&target_ip.octets());
    bytes::Bytes::from(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ethernet::frame::ether_type,
        icmp::v6::Icmpv6Type,
        ip::frame::{IP_P_ICMPV6, IP_P_TCP, IP_P_UDP},
    };

    const DST_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 1];
    const SRC_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 2];
    const SRC_V4: [u8; 4] = [10, 0, 2, 15];
    const DST_V4: [u8; 4] = [10, 0, 2, 2];
    const SRC_V6: [u8; 16] = [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    const DST_V6: [u8; 16] = [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];

    const ETH_LEN: usize = 14;

    fn be16(bytes: &[u8]) -> u16 {
        u16::from_be_bytes([bytes[0], bytes[1]])
    }

    #[test]
    fn udp_over_ipv4_lengths_and_checksums() {
        let payload = b"hello world";
        let packet = PacketBuilder::new(DST_MAC, SRC_MAC, ether_type::IPV4)
            .ipv4(SRC_V4, DST_V4, IP_P_UDP)
            .udp(68, 67)
            .payload(payload);

        let ip = ETH_LEN;
        let udp = ip + IPV4_HEADER_LEN;
        assert_eq!(packet.len(), udp + 8 + payload.len());

        // IPv4 total_length covers the IP header and everything after it.
        assert_eq!(be16(&packet[ip + 2..]), (packet.len() - ip) as u16);
        // UDP length covers the UDP header and the payload.
        assert_eq!(be16(&packet[udp + 4..]), (packet.len() - udp) as u16);

        // A correct checksum re-checksums to zero.
        assert_eq!(ipv4_checksum(&packet[ip..ip + IPV4_HEADER_LEN]), 0);
        assert_ne!(be16(&packet[udp + 6..]), 0);
        assert_eq!(
            udp_checksum(&packet[udp..], Ipv4Addr::from(SRC_V4), Ipv4Addr::from(DST_V4)),
            0xFFFF
        );
    }

    #[test]
    fn tcp_over_ipv6_checksum_uses_the_v6_pseudo_header() {
        let packet = PacketBuilder::new(DST_MAC, SRC_MAC, ether_type::IPV6)
            .ipv6(SRC_V6, DST_V6, IP_P_TCP)
            .tcp(1234, 80, 1, 0, 0x002, 65535)
            .payload(b"GET /");

        let ip = ETH_LEN;
        let tcp = ip + IPV6_HEADER_LEN;
        // IPv6 payload_length excludes the 40-byte fixed header.
        assert_eq!(be16(&packet[ip + 4..]), (packet.len() - ip - IPV6_HEADER_LEN) as u16);
        assert_ne!(be16(&packet[tcp + 16..]), 0);
        assert_eq!(
            tcp_checksum_v6(&packet[tcp..], Ipv6Addr::from(SRC_V6), Ipv6Addr::from(DST_V6)),
            0
        );
    }

    #[test]
    fn icmpv6_checksum_is_always_filled_in() {
        let packet = PacketBuilder::new(DST_MAC, SRC_MAC, ether_type::IPV6)
            .ipv6(SRC_V6, DST_V6, IP_P_ICMPV6)
            .hop_limit(255)
            .icmpv6(Icmpv6Type::NeighborAdvertisement as u8, 0, [0x60, 0, 0, 0])
            .payload(&DST_V6);

        let ip = ETH_LEN;
        let icmpv6 = ip + IPV6_HEADER_LEN;
        // NDP requires a hop limit of 255; it is the 8th byte of the header.
        assert_eq!(packet[ip + 7], 255);
        assert_ne!(be16(&packet[icmpv6 + 2..]), 0);
        assert_eq!(
            icmpv6_checksum(&packet[icmpv6..], Ipv6Addr::from(SRC_V6), Ipv6Addr::from(DST_V6)),
            0
        );
    }

    #[test]
    fn build_finalizes_a_header_only_packet() {
        let packet = PacketBuilder::new(DST_MAC, SRC_MAC, ether_type::ARP)
            .arp(1, SRC_MAC, SRC_V4, [0; 6], DST_V4)
            .build();
        assert_eq!(packet.len(), ETH_LEN + 28);
    }
}
