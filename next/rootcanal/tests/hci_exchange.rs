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

//! Test for verifying the exchange of HCI packets with the rootcanal controller.

use rootcanal::controller::Id;

use crate::world::{
    HCI_RESET, LE_SET_ADVERTISING_DATA, LE_SET_ADVERTISING_ENABLE, LE_SET_ADVERTISING_PARAMETERS,
    World,
};

const SNIFFER_ID: Id = 1;
const SNIFFER_ADDR: &str = "00:00:00:00:00:01";
const DUT_ID: Id = 2;
const DUT_ADDR: &str = "00:00:00:00:00:02";
const ADV_DATA: &[u8] = &[0x02, 0x01, 0x06];

#[tokio::test]
async fn test_hci_exchange() {
    let mut world = World::new();

    world.given_sniffer_controller(SNIFFER_ID, SNIFFER_ADDR);
    world.given_dut_controller(DUT_ID, DUT_ADDR);

    world.when_send_hci_reset(DUT_ID);
    world.then_command_complete_received(HCI_RESET).await;

    world.when_set_le_advertising_parameters(DUT_ID);
    world.then_command_complete_received(LE_SET_ADVERTISING_PARAMETERS).await;

    world.when_set_le_advertising_data(DUT_ID, ADV_DATA);
    world.then_command_complete_received(LE_SET_ADVERTISING_DATA).await;

    world.when_set_le_advertising_enable(DUT_ID, true);
    world.then_command_complete_received(LE_SET_ADVERTISING_ENABLE).await;

    world.then_sniffer_receives_advertising_pdu(DUT_ID, ADV_DATA).await;
}
