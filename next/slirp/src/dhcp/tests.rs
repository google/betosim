// Copyright 2023-2025 The Android Open Source Project
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

use std::{collections::HashMap, net::Ipv4Addr};

use netsim_packets::{EthernetFrame, Ipv4Header, MacAddr, UdpHeader};
use zerocopy::IntoBytes;

use crate::{
    Config, SlirpResponse,
    clock::MockClock,
    dhcp::dhcp_impl::{DHCP_MAGIC_COOKIE, DhcpManager, DhcpMessageType, DhcpPacket, DhcpPacketRef},
};

fn parse_dhcp_reply_payload(packet: &[u8]) -> DhcpPacketRef<'_> {
    let (_, ip_payload) = EthernetFrame::parse(packet).unwrap();
    let (_, udp_payload) = Ipv4Header::parse(ip_payload).unwrap();
    let (_, dhcp_payload) = UdpHeader::parse(udp_payload).unwrap();
    DhcpPacket::parse(dhcp_payload).unwrap().0
}

fn create_discover_packet() -> DhcpPacket {
    let mut options = [0; 308];
    options[0] = 53; // DHCP Message Type
    options[1] = 1;
    options[2] = DhcpMessageType::Discover as u8;
    options[3] = 255; // End
    DhcpPacket {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: [1, 2, 3, 4],
        secs: [0; 2],
        flags: [0; 2],
        ciaddr: [0; 4],
        yiaddr: [0; 4],
        siaddr: [0; 4],
        giaddr: [0; 4],
        chaddr: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        sname: [0; 64],
        file: [0; 128],
        magic_cookie: DHCP_MAGIC_COOKIE,
        options,
    }
}

fn create_request_packet(requested_ip: Ipv4Addr) -> DhcpPacket {
    let mut options = [0; 308];
    options[0] = 53; // DHCP Message Type
    options[1] = 1;
    options[2] = DhcpMessageType::Request as u8;
    options[3] = 50; // Requested IP address
    options[4] = 4;
    options[5..9].copy_from_slice(&requested_ip.octets());
    options[9] = 255; // End
    DhcpPacket {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: [1, 2, 3, 5],
        secs: [0; 2],
        flags: [0; 2],
        ciaddr: [0; 4],
        yiaddr: [0; 4],
        siaddr: [0; 4],
        giaddr: [0; 4],
        chaddr: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        sname: [0; 64],
        file: [0; 128],
        magic_cookie: DHCP_MAGIC_COOKIE,
        options,
    }
}

fn parse_dhcp_options(options: &[u8]) -> HashMap<u8, Vec<u8>> {
    let mut map = HashMap::new();
    let mut i = 0;
    while i < options.len() {
        let option_code = options[i];
        if option_code == 0 {
            i += 1;
            continue;
        }
        if option_code == 255 {
            break;
        }
        if i + 1 >= options.len() {
            break;
        }
        let len = options[i + 1] as usize;
        if i + 1 + len >= options.len() {
            break;
        }
        let data = options[i + 2..i + 2 + len].to_vec();
        map.insert(option_code, data);
        i += len + 2;
    }
    map
}

#[test]
fn test_dhcp_discover_offer() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let clock = MockClock::new();
    let discover_packet = create_discover_packet();
    let mut responses = Vec::new();

    dhcp_manager.handle_packet(&mut responses, &config, &clock, discover_packet.as_bytes());

    assert_eq!(responses.len(), 1);
    let offer_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let (eth_frame, _) = EthernetFrame::parse(&offer_packet).unwrap();
    assert_eq!(eth_frame.src_addr, config.gateway_mac);
    assert_eq!(eth_frame.dst_addr, MacAddr { bytes: [0xff; 6] });
    let offer_dhcp = parse_dhcp_reply_payload(&offer_packet);

    assert_eq!(offer_dhcp.op, 2); // BOOTREPLY
    let options = parse_dhcp_options(&offer_dhcp.options);
    assert_eq!(options.get(&53).unwrap(), &vec![DhcpMessageType::Offer as u8]);
}

#[test]
fn test_dhcp_offer_includes_config_options() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config {
        boot_file: Some("my_boot_file".to_string()),
        domain_name: Some("example.com".to_string()),
        tftp_server_name: Some("tftp.example.com".to_string()),
        ..Default::default()
    };

    let clock = MockClock::new();
    let discover_packet = create_discover_packet();
    let mut responses = Vec::new();

    dhcp_manager.handle_packet(&mut responses, &config, &clock, discover_packet.as_bytes());

    assert_eq!(responses.len(), 1);
    let offer_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let offer_dhcp = parse_dhcp_reply_payload(&offer_packet);

    let options = parse_dhcp_options(&offer_dhcp.options);

    // Assert Boot File (Option 67)
    assert_eq!(options.get(&67).unwrap(), &config.boot_file.unwrap().into_bytes());

    // Assert Domain Name (Option 15)
    assert_eq!(options.get(&15).unwrap(), &config.domain_name.unwrap().into_bytes());

    // Assert TFTP Server Name (Option 66)
    assert_eq!(options.get(&66).unwrap(), &config.tftp_server_name.unwrap().into_bytes());
}

#[test]
fn test_dhcp_request_invalid_ip_sends_nak() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let clock = MockClock::new();
    let mut responses = Vec::new();

    // Request an IP that is outside the pool
    let request_packet = create_request_packet(Ipv4Addr::new(192, 168, 1, 100));

    dhcp_manager.handle_packet(&mut responses, &config, &clock, request_packet.as_bytes());

    assert_eq!(responses.len(), 1);
    let nak_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let nak_dhcp = parse_dhcp_reply_payload(&nak_packet);

    assert_eq!(nak_dhcp.op, 2); // BOOTREPLY
    let options = parse_dhcp_options(&nak_dhcp.options);
    assert_eq!(options.get(&53).unwrap(), &vec![DhcpMessageType::Nak as u8]);
}

#[test]
fn test_dhcp_offer_includes_dns_search_list() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config {
        dns_search: Some(vec!["domain1.com".to_string(), "domain2.com".to_string()]),
        ..Default::default()
    };

    let clock = MockClock::new();
    let discover_packet = create_discover_packet();
    let mut responses = Vec::new();

    dhcp_manager.handle_packet(&mut responses, &config, &clock, discover_packet.as_bytes());

    assert_eq!(responses.len(), 1);
    let offer_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let offer_dhcp = parse_dhcp_reply_payload(&offer_packet);

    let options = parse_dhcp_options(&offer_dhcp.options);

    let mut expected_dns_search = Vec::new();
    for domain in config.dns_search.unwrap() {
        for label in domain.split('.') {
            expected_dns_search.push(label.len() as u8);
            expected_dns_search.extend_from_slice(label.as_bytes());
        }
        expected_dns_search.push(0);
    }

    assert_eq!(options.get(&119).unwrap(), &expected_dns_search);
}

#[test]
fn test_dhcp_offer_includes_hostname() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config { client_hostname: Some("my-guest".to_string()), ..Default::default() };

    let clock = MockClock::new();
    let discover_packet = create_discover_packet();
    let mut responses = Vec::new();

    dhcp_manager.handle_packet(&mut responses, &config, &clock, discover_packet.as_bytes());

    assert_eq!(responses.len(), 1);
    let offer_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let offer_dhcp = parse_dhcp_reply_payload(&offer_packet);

    let options = parse_dhcp_options(&offer_dhcp.options);

    assert_eq!(options.get(&12).unwrap(), &config.client_hostname.unwrap().into_bytes());
}

fn create_inform_packet(ciaddr: Ipv4Addr) -> DhcpPacket {
    let mut options = [0; 308];
    options[0] = 53; // DHCP Message Type
    options[1] = 1;
    options[2] = DhcpMessageType::Inform as u8;
    options[3] = 255; // End
    DhcpPacket {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: [1, 2, 3, 6],
        secs: [0; 2],
        flags: [0; 2],
        ciaddr: ciaddr.octets(),
        yiaddr: [0; 4],
        siaddr: [0; 4],
        giaddr: [0; 4],
        chaddr: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        sname: [0; 64],
        file: [0; 128],
        magic_cookie: DHCP_MAGIC_COOKIE,
        options,
    }
}

fn create_release_packet(ciaddr: Ipv4Addr) -> DhcpPacket {
    let mut options = [0; 308];
    options[0] = 53; // DHCP Message Type
    options[1] = 1;
    options[2] = DhcpMessageType::Release as u8;
    options[3] = 255; // End
    DhcpPacket {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: [1, 2, 3, 7],
        secs: [0; 2],
        flags: [0; 2],
        ciaddr: ciaddr.octets(),
        yiaddr: [0; 4],
        siaddr: [0; 4],
        giaddr: [0; 4],
        chaddr: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        sname: [0; 64],
        file: [0; 128],
        magic_cookie: DHCP_MAGIC_COOKIE,
        options,
    }
}

fn create_bootp_packet() -> DhcpPacket {
    let mut options = [0; 308];
    options[0] = 255; // End (no Option 53!)
    DhcpPacket {
        op: 1, // BootRequest
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: [1, 2, 3, 8],
        secs: [0; 2],
        flags: [0; 2],
        ciaddr: [0; 4],
        yiaddr: [0; 4],
        siaddr: [0; 4],
        giaddr: [0; 4],
        chaddr: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        sname: [0; 64],
        file: [0; 128],
        magic_cookie: DHCP_MAGIC_COOKIE,
        options,
    }
}

#[test]
fn test_dhcp_inform() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let client_ip = Ipv4Addr::new(10, 0, 2, 15);
    let inform_packet = create_inform_packet(client_ip);
    let mut responses = Vec::new();

    dhcp_manager.handle_packet(
        &mut responses,
        &config,
        &MockClock::new(),
        inform_packet.as_bytes(),
    );

    assert_eq!(responses.len(), 1);
    let ack_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let ack_dhcp = parse_dhcp_reply_payload(&ack_packet);

    assert_eq!(ack_dhcp.op, 2); // BOOTREPLY
    assert_eq!(ack_dhcp.ciaddr, client_ip.octets());
    assert_eq!(ack_dhcp.yiaddr, [0; 4]); // Must be 0 for INFORM!

    let options = parse_dhcp_options(&ack_dhcp.options);
    assert_eq!(options.get(&53).unwrap(), &vec![DhcpMessageType::Ack as u8]);
}

#[test]
fn test_dhcp_release() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let clock = MockClock::new();
    let mut responses = Vec::new();

    // 1. Allocate a lease first by sending a REQUEST
    let client_ip = Ipv4Addr::new(10, 0, 2, 15);
    let request_packet = create_request_packet(client_ip);
    dhcp_manager.handle_packet(&mut responses, &config, &clock, request_packet.as_bytes());
    assert_eq!(responses.len(), 1);
    responses.clear();

    // Verify lease was created
    let mac = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34] };
    assert!(dhcp_manager.find_lease_by_mac(&mac, &clock).is_some());

    // 2. Send RELEASE
    let release_packet = create_release_packet(client_ip);
    dhcp_manager.handle_packet(&mut responses, &config, &clock, release_packet.as_bytes());

    // Verify no response is sent for RELEASE
    assert!(responses.is_empty());

    // Verify lease was removed!
    assert!(dhcp_manager.find_lease_by_mac(&mac, &clock).is_none());
}

#[test]
fn test_bootp_legacy_request() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config {
        boot_file: Some("bootx64.efi".to_string()),
        tftp_server_name: Some("tftp.server.local".to_string()),
        ..Config::default()
    };
    let clock = MockClock::new();
    let mut responses = Vec::new();

    // Send legacy BOOTP request
    let bootp_packet = create_bootp_packet();
    dhcp_manager.handle_packet(&mut responses, &config, &clock, bootp_packet.as_bytes());

    assert_eq!(responses.len(), 1);
    let reply_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!("Expected a packet response"),
    };

    let reply_dhcp = parse_dhcp_reply_payload(&reply_packet);

    assert_eq!(reply_dhcp.op, 2); // BOOTREPLY
    assert_eq!(reply_dhcp.yiaddr, config.guest_ipv4.octets()); // assigned IP

    // Verify legacy header fields are populated!
    let file_str = std::str::from_utf8(&reply_dhcp.file).unwrap().trim_matches('\0');
    assert_eq!(file_str, "bootx64.efi");

    let sname_str = std::str::from_utf8(&reply_dhcp.sname).unwrap().trim_matches('\0');
    assert_eq!(sname_str, "tftp.server.local");

    // Verify options are also populated but do NOT contain Option 53!
    let options = parse_dhcp_options(&reply_dhcp.options);
    assert!(!options.contains_key(&53)); // No DHCP Message Type!
    assert_eq!(options.get(&67).unwrap(), b"bootx64.efi"); // Boot File option
    assert_eq!(options.get(&66).unwrap(), b"tftp.server.local"); // TFTP Server option
}

#[test]
fn test_dhcp_invalid_magic_cookie_and_short_packet() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let clock = MockClock::new();
    let mut responses = Vec::new();

    // Case 1: Packet too short
    dhcp_manager.handle_packet(&mut responses, &config, &clock, &[0; 10]);
    assert!(responses.is_empty());

    // Case 2: Invalid magic cookie
    let mut discover_packet = create_discover_packet();
    discover_packet.magic_cookie = [0, 0, 0, 0];
    dhcp_manager.handle_packet(&mut responses, &config, &clock, discover_packet.as_bytes());
    assert!(responses.is_empty());
}

#[test]
fn test_dhcp_unsupported_message_type() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let clock = MockClock::new();
    let mut responses = Vec::new();

    // Create an ACK packet (which is a server-to-client packet, the server
    // should ignore it)
    let mut options = [0; 308];
    options[0] = 53;
    options[1] = 1;
    options[2] = DhcpMessageType::Ack as u8; // Unsupported incoming type
    options[3] = 255;
    let ack_packet = DhcpPacket {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: [1, 2, 3, 9],
        secs: [0; 2],
        flags: [0; 2],
        ciaddr: [0; 4],
        yiaddr: [0; 4],
        siaddr: [0; 4],
        giaddr: [0; 4],
        chaddr: [0; 16],
        sname: [0; 64],
        file: [0; 128],
        magic_cookie: DHCP_MAGIC_COOKIE,
        options,
    };

    dhcp_manager.handle_packet(&mut responses, &config, &clock, ack_packet.as_bytes());
    assert!(responses.is_empty());
}

#[test]
fn test_dhcp_lease_serialization() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let clock = MockClock::new();
    let mut responses = Vec::new();

    // 1. Allocate a lease
    let client_ip = Ipv4Addr::new(10, 0, 2, 15);
    let request_packet = create_request_packet(client_ip);
    dhcp_manager.handle_packet(&mut responses, &config, &clock, request_packet.as_bytes());
    assert_eq!(responses.len(), 1);

    // 2. Serialize DhcpManager
    let serialized = serde_json::to_string(&dhcp_manager).unwrap();

    // 3. Deserialize back
    let deserialized_manager: DhcpManager = serde_json::from_str(&serialized).unwrap();

    // 4. Verify lease was restored
    let mac = MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34] };
    let restored_lease = deserialized_manager.find_lease_by_mac(&mac, &clock).unwrap();
    assert_eq!(restored_lease.ip_addr, client_ip);
}

#[test]
fn test_dhcp_default() {
    let dhcp_manager = DhcpManager::default();
    assert_eq!(dhcp_manager.leases.len(), 0);
}

#[test]
fn test_dhcp_discover_with_existing_lease() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let clock = MockClock::new();
    let mut responses = Vec::new();

    // 1. Allocate a lease
    let client_ip = Ipv4Addr::new(10, 0, 2, 15);
    let request_packet = create_request_packet(client_ip);
    dhcp_manager.handle_packet(&mut responses, &config, &clock, request_packet.as_bytes());
    responses.clear();

    // 2. Send Discover again from the same MAC
    let discover_packet = create_discover_packet();
    dhcp_manager.handle_packet(&mut responses, &config, &clock, discover_packet.as_bytes());

    // 3. Verify it offers the SAME IP
    assert_eq!(responses.len(), 1);
    let offer_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!(),
    };
    let offer_dhcp = parse_dhcp_reply_payload(&offer_packet);
    assert_eq!(offer_dhcp.yiaddr, client_ip.octets());
}

#[test]
fn test_dhcp_request_different_ip_sends_nak() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let clock = MockClock::new();
    let mut responses = Vec::new();

    // 1. Allocate a lease for 10.0.2.15
    let client_ip = Ipv4Addr::new(10, 0, 2, 15);
    let request_packet = create_request_packet(client_ip);
    dhcp_manager.handle_packet(&mut responses, &config, &clock, request_packet.as_bytes());
    responses.clear();

    // 2. Request 10.0.2.16 instead (different from assigned 10.0.2.15)
    let request_packet_2 = create_request_packet(Ipv4Addr::new(10, 0, 2, 16));
    dhcp_manager.handle_packet(&mut responses, &config, &clock, request_packet_2.as_bytes());

    // 3. Verify NAK
    assert_eq!(responses.len(), 1);
    let nak_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!(),
    };
    let nak_dhcp = parse_dhcp_reply_payload(&nak_packet);
    let options = parse_dhcp_options(&nak_dhcp.options);
    assert_eq!(options.get(&53).unwrap(), &vec![DhcpMessageType::Nak as u8]);
}

#[test]
fn test_dhcp_ip_exhaustion_and_table_full() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let clock = MockClock::new();
    let mut responses = Vec::new();

    // 1. Fill the lease table (16 leases)
    let start_ip = u32::from_be_bytes(config.guest_ipv4.octets());
    for i in 0..16 {
        let ip = Ipv4Addr::from(start_ip + i as u32);
        let mut request = create_request_packet(ip);
        request.chaddr[0] = i as u8;
        dhcp_manager.handle_packet(&mut responses, &config, &clock, request.as_bytes());
        assert_eq!(responses.len(), 1);
        responses.clear();
    }
    assert_eq!(dhcp_manager.leases.len(), 16);

    // 2. Try to send Discover from a 17th MAC. It should fail to find an IP and
    //    return nothing
    let mut discover_packet = create_discover_packet();
    discover_packet.chaddr[0] = 99; // 17th MAC
    dhcp_manager.handle_packet(&mut responses, &config, &clock, discover_packet.as_bytes());
    assert!(responses.is_empty());

    // 3. Try to send a Request from a 17th MAC. It should send a NAK
    let mut request_packet = create_request_packet(Ipv4Addr::from(start_ip));
    request_packet.chaddr[0] = 99;
    dhcp_manager.handle_packet(&mut responses, &config, &clock, request_packet.as_bytes());
    assert_eq!(responses.len(), 1);
    let nak_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!(),
    };
    let nak_dhcp = parse_dhcp_reply_payload(&nak_packet);
    let options = parse_dhcp_options(&nak_dhcp.options);
    assert_eq!(options.get(&53).unwrap(), &vec![DhcpMessageType::Nak as u8]);
}

#[test]
fn test_dhcp_inform_with_config() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config {
        domain_name: Some("example.com".to_string()),
        dns_search: Some(vec!["search.com".to_string()]),
        ..Config::default()
    };
    let clock = MockClock::new();
    let mut responses = Vec::new();

    let inform_packet = create_inform_packet(Ipv4Addr::new(10, 0, 2, 15));
    dhcp_manager.handle_packet(&mut responses, &config, &clock, inform_packet.as_bytes());

    assert_eq!(responses.len(), 1);
    let ack_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!(),
    };
    let ack_dhcp = parse_dhcp_reply_payload(&ack_packet);
    let options = parse_dhcp_options(&ack_dhcp.options);

    assert_eq!(options.get(&15).unwrap(), b"example.com");
    assert!(options.contains_key(&119));
}

#[test]
fn test_dhcp_release_non_existent_lease() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let clock = MockClock::new();
    let mut responses = Vec::new();

    let release_packet = create_release_packet(Ipv4Addr::new(10, 0, 2, 99));
    dhcp_manager.handle_packet(&mut responses, &config, &clock, release_packet.as_bytes());
    assert!(responses.is_empty());
}

#[test]
fn test_dhcp_bootp_existing_lease() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config { boot_file: Some("boot.efi".to_string()), ..Config::default() };
    let clock = MockClock::new();
    let mut responses = Vec::new();

    // 1. Allocate a lease first
    let client_ip = config.guest_ipv4;
    let request_packet = create_request_packet(client_ip);
    dhcp_manager.handle_packet(&mut responses, &config, &clock, request_packet.as_bytes());
    responses.clear();

    // 2. Send BOOTP request from same MAC
    let bootp_packet = create_bootp_packet();
    dhcp_manager.handle_packet(&mut responses, &config, &clock, bootp_packet.as_bytes());

    assert_eq!(responses.len(), 1);
    let reply_packet = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!(),
    };
    let reply_dhcp = parse_dhcp_reply_payload(&reply_packet);
    assert_eq!(reply_dhcp.yiaddr, client_ip.octets());
}

#[test]
fn test_dhcp_parse_option_edge_cases() {
    let mut dhcp_manager = DhcpManager::new();
    let config = Config::default();
    let clock = MockClock::new();
    let mut responses = Vec::new();

    let send_custom_opt53 =
        |manager: &mut DhcpManager, opt53_val: &[u8], responses: &mut Vec<SlirpResponse>| {
            let mut options = [0; 308];
            options[0] = 53;
            options[1] = opt53_val.len() as u8;
            options[2..2 + opt53_val.len()].copy_from_slice(opt53_val);
            options[2 + opt53_val.len()] = 255;
            let pkt = DhcpPacket {
                op: 1,
                htype: 1,
                hlen: 6,
                hops: 0,
                xid: [1, 2, 3, 9],
                secs: [0; 2],
                flags: [0; 2],
                ciaddr: [0; 4],
                yiaddr: [0; 4],
                siaddr: [0; 4],
                giaddr: [0; 4],
                chaddr: [0; 16],
                sname: [0; 64],
                file: [0; 128],
                magic_cookie: DHCP_MAGIC_COOKIE,
                options,
            };
            manager.handle_packet(responses, &config, &clock, pkt.as_bytes());
        };

    // Case 1: Option 53 value is Offer (2)
    send_custom_opt53(&mut dhcp_manager, &[2], &mut responses);
    assert!(responses.is_empty());

    // Case 2: Option 53 value is Nak (6)
    send_custom_opt53(&mut dhcp_manager, &[6], &mut responses);
    assert!(responses.is_empty());

    // Case 3: Option 53 value is unknown (9) -> get_message_type returns None
    // -> falls back to BOOTP!
    send_custom_opt53(&mut dhcp_manager, &[9], &mut responses);
    assert_eq!(responses.len(), 1);
    responses.clear();

    // Case 4: Option 53 length is not 1 (length 2) -> get_message_type returns
    // None -> falls back to BOOTP!
    send_custom_opt53(&mut dhcp_manager, &[1, 2], &mut responses);
    assert_eq!(responses.len(), 1);
    responses.clear();

    // Case 5: Request packet without Option 50
    let mut options = [0; 308];
    options[0] = 53;
    options[1] = 1;
    options[2] = DhcpMessageType::Request as u8;
    options[3] = 255;
    let req_no_opt50 = DhcpPacket {
        op: 1,
        htype: 1,
        hlen: 6,
        hops: 0,
        xid: [1, 2, 3, 10],
        secs: [0; 2],
        flags: [0; 2],
        ciaddr: [0; 4],
        yiaddr: [0; 4],
        siaddr: [0; 4],
        giaddr: [0; 4],
        chaddr: [0; 16],
        sname: [0; 64],
        file: [0; 128],
        magic_cookie: DHCP_MAGIC_COOKIE,
        options,
    };
    dhcp_manager.handle_packet(&mut responses, &config, &clock, req_no_opt50.as_bytes());
    assert_eq!(responses.len(), 1);
    let reply = match responses.pop().unwrap() {
        SlirpResponse::Packet(packet) => packet,
        _ => panic!(),
    };
    let reply_dhcp = parse_dhcp_reply_payload(&reply);
    let opts = parse_dhcp_options(&reply_dhcp.options);
    assert_eq!(opts.get(&53).unwrap(), &vec![DhcpMessageType::Nak as u8]);
}
