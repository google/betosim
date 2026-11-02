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

use std::net::SocketAddr;

use bytes::Bytes;

use crate::{SlirpResponse, UdpManager};

#[test]
fn test_udp_manager_creation() {
    let _udp_manager = UdpManager::new();
}

#[test]
fn test_udp_packet_handling() {
    let mut udp_manager = UdpManager::new();
    let mut responses = Vec::new();

    let payload = b"hello";
    let source: SocketAddr = "10.0.2.15:1234".parse().unwrap();
    let dest: SocketAddr = "8.8.8.8:53".parse().unwrap();

    udp_manager.handle_packet(&mut responses, payload, source, dest, None);

    assert_eq!(responses.len(), 2);
    let establish_conn = &responses[0];
    let write_to_conn = &responses[1];

    match establish_conn {
        SlirpResponse::EstablishConnection(id, _) if *id == 1 << 63 => {}
        _ => panic!("Expected EstablishConnection response"),
    }

    match write_to_conn {
        SlirpResponse::WriteToConnection(id, data) if *id == 1 << 63 => {
            assert_eq!(data, &Bytes::from_static(payload));
        }
        _ => panic!("Expected WriteToConnection response"),
    }
}

#[test]
fn test_udp_default() {
    let _default_manager = UdpManager::default();
}

#[test]
fn test_udp_flow_lifecycle() {
    let mut udp_manager = UdpManager::new();
    let mut responses = Vec::new();

    let payload = b"hello";
    let source: SocketAddr = "10.0.2.15:1234".parse().unwrap();
    let dest: SocketAddr = "8.8.8.8:53".parse().unwrap();

    // 1. Establish flow
    udp_manager.handle_packet(&mut responses, payload, source, dest, None);
    let conn_id = match responses[0] {
        SlirpResponse::EstablishConnection(id, _) => id,
        _ => panic!("Expected EstablishConnection"),
    };

    // Verify get_connections
    let conns: Vec<_> = udp_manager.get_connections().collect();
    assert_eq!(conns.len(), 1);
    assert_eq!(conns[0].local_addr, source);
    assert_eq!(conns[0].peer_addr, dest);

    // 2. Handle reply (success)
    let reply_data = b"world";
    let (orig_dest, guest_addr, data) = udp_manager.handle_reply(conn_id, reply_data).unwrap();
    assert_eq!(orig_dest, dest);
    assert_eq!(guest_addr, source);
    assert_eq!(data, reply_data);

    // 3. Handle reply with invalid conn_id (failure/None)
    assert!(udp_manager.handle_reply(999, reply_data).is_none());

    // 4. Remove flow (success)
    udp_manager.remove_flow(conn_id);
    assert_eq!(udp_manager.get_connections().count(), 0);

    // 5. Remove flow with invalid conn_id (does nothing, safe)
    udp_manager.remove_flow(999);
}
