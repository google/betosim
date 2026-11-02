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

use bytes::Bytes;

use crate::world::TestWorld;

/// H4 HCI_Reset command: indicator 0x01, opcode 0x0C03 (little-endian), no params.
const HCI_RESET_COMMAND: &[u8] = &[0x01, 0x03, 0x0c, 0x00];
/// Larger than 65535 bytes, so the frame needs the 64-bit extended payload
/// length (RFC 6455, Section 5.2).
const LARGE_PAYLOAD_LEN: usize = 70_000;
/// Smallest non-empty packet.
const ONE_BYTE_PACKET: &[u8] = &[0x02];

// Scenario: A loopback client gets its packets echoed back
//   Given a WebSocket client connected with ?loopback=1
//   When it sends packets of several sizes
//   Then it receives each packet back unchanged, in order
//   And no chip is added or deleted in the backend
#[tokio::test]
async fn test_loopback_echoes_without_registering_a_chip() {
    let mut world = TestWorld::new().await;
    let mut client = world.given_a_loopback_client_connected().await;

    let packets =
        [HCI_RESET_COMMAND.to_vec(), vec![0xAB; LARGE_PAYLOAD_LEN], ONE_BYTE_PACKET.to_vec()];
    for packet in packets {
        let packet = Bytes::from(packet);
        client.send_binary(packet.clone()).await;
        assert_eq!(client.read_binary().await, packet);
    }

    // Wait for the server to echo the Close frame instead of sleeping; this
    // shows the server has handled the close before the backend is checked.
    client.close().await;
    assert!(client.read_binary().await.is_empty(), "expected the server's Close frame");
    world.then_backend_receives_no_add_chip_request();
    world.then_backend_deletes_nothing();
}

#[tokio::test]
async fn test_loopback_across_radio_routes() {
    let mut world = TestWorld::new().await;

    for path in [
        "/v1/websocket/wifi?loopback=1",
        "/v1/websocket/uwb?loopback=true",
        "/v1/websocket/nfc?loopback=1",
        "/v1/websocket/cellular?loopback=1",
    ] {
        let mut client = world.given_a_loopback_client_connected_at_path(path).await;
        let payload = Bytes::from_static(HCI_RESET_COMMAND);
        client.send_binary(payload.clone()).await;
        assert_eq!(client.read_binary().await, payload, "loopback failed for {path}");

        client.close().await;
        assert!(client.read_binary().await.is_empty(), "expected the server's Close frame");
        world.then_backend_receives_no_add_chip_request();
        world.then_backend_deletes_nothing();
    }
}
