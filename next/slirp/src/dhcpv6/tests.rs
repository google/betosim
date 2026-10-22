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

use std::net::Ipv6Addr;

use netsim_packets::{
    DHCPV6_CLIENT_PORT, DHCPV6_SERVER_PORT, Dhcpv6Header, Dhcpv6OptionHeader, Dhcpv6OptionIterator,
    EthernetFrame, IP_P_UDP, Ipv6Builder, Ipv6Header, MSG_INFORMATION_REQUEST, MSG_REPLY, MacAddr,
    OPTION_CLIENTID, OPTION_DNS_SERVERS, OPTION_DOMAIN_LIST, OPTION_SERVERID, UdpBuilder,
    UdpHeader,
};
use zerocopy::FromBytes;

use crate::{
    Config, SlirpResponse,
    dhcpv6::Dhcpv6Manager,
    packet::{ParsedPacket, TransportPacket},
};

#[test]
fn test_dhcpv6_information_request_reply() {
    let mut manager = Dhcpv6Manager::new();
    let guest_mac = MacAddr { bytes: [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc] };
    let config = Config {
        dns_search: Some(vec!["example.com".to_string(), "test.org".to_string()]),
        guest_mac,
        ..Default::default()
    };

    // 1. Build a DHCPv6 Information-Request packet
    let transaction_id = [0x11, 0x22, 0x33];
    let client_duid = [0, 3, 0, 1, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]; // DUID-LL based on guest MAC

    // Option Client ID: header (4) + DUID (10) = 14 bytes
    let total_dhcpv6_len = 4 + 14;
    let mut dhcpv6_payload = vec![0u8; total_dhcpv6_len];

    let (hdr_slice, opt_slice) = dhcpv6_payload.split_at_mut(4);
    let hdr = Dhcpv6Header::mut_from_bytes(hdr_slice).unwrap();
    hdr.msg_type = MSG_INFORMATION_REQUEST;
    hdr.transaction_id = transaction_id;

    let opt_hdr = Dhcpv6OptionHeader::mut_from_bytes(&mut opt_slice[..4]).unwrap();
    opt_hdr.code.set(OPTION_CLIENTID);
    opt_hdr.len.set(10);
    opt_slice[4..14].copy_from_slice(&client_duid);

    // Build Ethernet + IPv6 + UDP wrapper for parsing
    let udp_header_len = 8;
    let total_udp_len = udp_header_len + total_dhcpv6_len;
    let total_ipv6_len = 40 + total_udp_len;
    let total_len = 14 + total_ipv6_len;
    let mut eth_packet = vec![0u8; total_len];

    {
        // 1. Fill Ethernet Header
        let (eth_slice, eth_payload) = eth_packet.split_at_mut(14);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_slice).unwrap();
        eth_frame.dst_addr = config.gateway_mac;
        eth_frame.src_addr = guest_mac;
        eth_frame.ethertype = 0x86DD.into(); // IPv6

        // 2. Build IPv6 Header
        let (ipv6_slice, ipv6_payload) = eth_payload.split_at_mut(40);
        let mut ipv6_builder =
            Ipv6Builder::new(ipv6_slice, IP_P_UDP, config.guest_ipv6, config.host_ipv6).unwrap();
        ipv6_builder.hop_limit(64);

        // 3. Build UDP Header and Payload
        let mut udp_builder = UdpBuilder::new_v6(
            ipv6_payload,
            config.guest_ipv6,
            config.host_ipv6,
            DHCPV6_CLIENT_PORT,
            DHCPV6_SERVER_PORT,
        )
        .unwrap();
        udp_builder.payload(&dhcpv6_payload[..total_dhcpv6_len]).unwrap();
        udp_builder.build().unwrap();

        ipv6_builder.payload_len(total_udp_len);
        ipv6_builder.build();
    }

    let parsed_packet = ParsedPacket::parse(&eth_packet).unwrap();

    // Extract UDP header from parsed packet
    let udp_header = match &parsed_packet.transport {
        Some(TransportPacket::Udp(hdr, _)) => hdr,
        _ => panic!("Expected UDP packet"),
    };

    let mut responses = Vec::new();

    // 2. Feed it to manager
    manager.handle_packet(
        &mut responses,
        &config,
        &parsed_packet,
        config.guest_ipv6,
        udp_header,
        &dhcpv6_payload,
    );

    // 3. Verify reply
    assert_eq!(responses.len(), 1);
    let reply_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(p) => p,
        _ => panic!("Expected Packet response"),
    };

    // Parse Reply
    let (_, reply_eth_payload) = EthernetFrame::parse(&reply_packet).unwrap();
    let reply_eth_frame = EthernetFrame::parse(&reply_packet).unwrap().0;
    assert_eq!(reply_eth_frame.dst_addr, guest_mac);
    assert_eq!(reply_eth_frame.src_addr, config.gateway_mac);

    let (reply_ipv6, reply_ip_payload) = Ipv6Header::parse(reply_eth_payload).unwrap();
    assert_eq!(Ipv6Addr::from(reply_ipv6.source_addr), config.host_ipv6);
    assert_eq!(Ipv6Addr::from(reply_ipv6.dest_addr), config.guest_ipv6);

    let (_, reply_udp_payload) = UdpHeader::parse(reply_ip_payload).unwrap();
    let reply_udp = UdpHeader::parse(reply_ip_payload).unwrap().0;
    assert_eq!(reply_udp.source_port.get(), DHCPV6_SERVER_PORT);
    assert_eq!(reply_udp.dest_port.get(), DHCPV6_CLIENT_PORT);
    assert_ne!(reply_udp.checksum.get(), 0);
    assert_eq!(
        netsim_packets::udp_checksum_v6(reply_ip_payload, config.host_ipv6, config.guest_ipv6),
        u16::MAX
    );

    let (reply_dhcpv6, reply_options) = Dhcpv6Header::parse(reply_udp_payload).unwrap();
    assert_eq!(reply_dhcpv6.msg_type, MSG_REPLY);
    assert_eq!(reply_dhcpv6.transaction_id, transaction_id);

    // Parse Reply Options
    let mut has_server_id = false;
    let mut has_client_id = false;
    let mut has_dns_servers = false;
    let mut has_domain_list = false;

    let iter = Dhcpv6OptionIterator::new(reply_options);
    for (code, val) in iter {
        match code {
            OPTION_SERVERID => {
                has_server_id = true;
                assert_eq!(val.len(), 10);
                assert_eq!(val[..2], 3u16.to_be_bytes()); // Type DUID-LL (3)
                assert_eq!(val[2..4], 1u16.to_be_bytes()); // HW Type Ethernet (1)
                assert_eq!(val[4..10], config.gateway_mac.bytes);
            }
            OPTION_CLIENTID => {
                has_client_id = true;
                assert_eq!(val, &client_duid[..]);
            }
            OPTION_DNS_SERVERS => {
                has_dns_servers = true;
                assert_eq!(val.len(), 16);
                assert_eq!(val, config.host_ipv6.octets());
            }
            OPTION_DOMAIN_LIST => {
                has_domain_list = true;
                let expected_bytes =
                    crate::dns::encode_dns_search_list(config.dns_search.as_ref().unwrap());
                assert_eq!(val, &expected_bytes[..]);
            }
            _ => panic!("Unexpected option code: {code}"),
        }
    }

    assert!(has_server_id, "Missing Server ID option");
    assert!(has_client_id, "Missing Client ID option");
    assert!(has_dns_servers, "Missing DNS Servers option");
    assert!(has_domain_list, "Missing Domain List option");
}
