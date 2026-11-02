// Copyright 2023 The Android Open Source Project
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

//! Test for verifying the exchange of link layer packets with the rootcanal controller.

use rootcanal::{controller::Id, types::Phy};

use crate::world::World;

const SNIFFER_ID: Id = 1;
const SNIFFER_ADDR: &str = "00:00:00:00:00:01";
const SENDER_ID: Id = 2;
const SENDER_ADDR: &str = "00:00:00:00:00:02";
const ADV_DATA: &[u8] = &[
    0x02, 0x01, 0x06, // Flags
    0x03, 0x03, 0xaa, 0xfe, // Service UUID
];

#[tokio::test]
async fn test_ll_exchange() {
    let mut world = World::new();

    world.given_sniffer_controller(SNIFFER_ID, SNIFFER_ADDR);
    world.given_dut_controller(SENDER_ID, SENDER_ADDR);

    world.when_inject_legacy_advertising_pdu(SENDER_ID, ADV_DATA, Phy::LowEnergy, -80);

    world.then_sniffer_receives_advertising_pdu(SENDER_ID, ADV_DATA).await;
}
