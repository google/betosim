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

use crate::world::TestWorld;

#[tokio::test]
async fn test_websocket_happy_path() {
    let mut world = TestWorld::new().await;

    let client_idx = world
        .given_a_websocket_client_connected(Some("test-peer"), Some("00:11:22:33:44:55"))
        .await;

    world.then_backend_receives_add_chip_request(client_idx, "test-peer", "00:11:22:33:44:55");

    let test_packet = vec![0x01, 0x02, 0x03, 0x04];
    world.when_client_sends_packet(client_idx, &test_packet).await;
    world.then_backend_receives_packet(client_idx, &test_packet).await;

    let response_packet = vec![0x05, 0x06, 0x07, 0x08];
    world.when_backend_sends_packet(client_idx, &response_packet).await;
    world.then_client_receives_packet(client_idx, &response_packet).await;

    world.when_client_closes_connection(client_idx).await;
    world.then_backend_deletes_chip(client_idx).await;
}
