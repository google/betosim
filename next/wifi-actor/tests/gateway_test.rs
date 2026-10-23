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

use crate::world::{MockGateway, World};

#[tokio::test]
async fn test_gateway_routing() {
    // 1. Setup World with MockGateway
    let mock_gateway = MockGateway::new();
    let outgoing_packets = mock_gateway.outgoing_packets.clone();
    let mut world = World::new_with_gateway(Box::new(mock_gateway)).await;

    // 2. Create AP
    world.given_an_ap().await;

    // 3. Create Chip
    let chip1_id = world.given_a_chip(1).await;
    outgoing_packets.lock().unwrap().clear();

    // 3. Chip transmits data destined for Internet (ToDS)
    // This helper sends a ToDS frame destined to a dummy "Gateway" MAC.
    // The WifiActor should route this to the GatewayTrait implementation
    // (MockGateway).
    world.when_chip_transmits_data_to_slirp(0).await;

    // 4. Verify Gateway received the packet
    // Give it a moment to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let packets = outgoing_packets.lock().unwrap();
    assert_eq!(packets.len(), 1, "Gateway should have received 1 packet");

    let (cid, _frame) = &packets[0];
    assert_eq!(cid.0, chip1_id, "Packet should be from Chip 1");

    // We can also verify contents of _frame if needed, but World helper sent
    // known payload.
}

#[tokio::test]
async fn test_gateway_routing_blocked_when_disabled() {
    let mock_gateway = MockGateway::new();
    let outgoing_packets = mock_gateway.outgoing_packets.clone();
    let mut world = World::new_with_gateway(Box::new(mock_gateway)).await;

    world.given_an_ap().await;
    world.given_a_chip(1).await;
    world.given_chip_is_disabled(0).await;
    outgoing_packets.lock().unwrap().clear();

    world.when_chip_transmits_data_to_slirp(0).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert!(outgoing_packets.lock().unwrap().is_empty());

    world.when_chip_transmits_infra_mdns(0, "\x01\x02\x03\x04").await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(outgoing_packets.lock().unwrap().len(), 1);
}
