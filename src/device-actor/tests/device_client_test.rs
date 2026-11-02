// Copyright (C) 2025 The Android Open Source Project
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

//! Device Client Logic Tests.
//!
//! This module uses a mock actor client to verify the `DeviceClient` wrapper
//! logic, ensuring correct serialization and state management without a full
//! actor runtime.

use device_actor::{DeviceActor, DeviceClient};
use device_api::{
    ChipCreateVariant, DeviceActionResult, DeviceChipCreate, DeviceConfig, DeviceCreate, DeviceId,
};
use netsim_model::{BleBeacon, ChipId, Pose};

// Feature: Device Client Logic
//
//   As a developer
//   I want to verify the implementation of the Device Client
//   So that I can ensure it correctly communicates with the actor
//
// Scenario: Client correctly serializes Create requests
//   Given a mock Actor Client expecting a Create call
//   When I call create_device on the client
//   Then the mock receives the Create request
#[tokio::test]
async fn test_device_client_create() {
    let mut mock = actor_framework::MockActorClient::<DeviceActor>::new();
    let device_id = DeviceId(1);

    mock.expect_create().returning(move |_| Ok(device_id));

    let client = DeviceClient::new(Box::new(mock));

    let params = DeviceCreate {
        device_config: DeviceConfig::new("test".to_string(), true, Pose::default(), false),
        chip: DeviceChipCreate {
            name: "beacon".to_string(),
            manufacturer: "Netsim".to_string(),
            product_name: "NetsimBeacon".to_string(),
            chip: ChipCreateVariant::Beacon(BleBeacon::default()),
        },
    };
    let id = client.create_device(params).await.unwrap();
    assert_eq!(id, device_id);
}

// Scenario: Client correctly serializes Reset requests
//   Given a mock Actor Client expecting a PerformAction(Reset) call with a
// specific ID   When I call reset on the client with a device ID
//   Then the mock receives the PerformAction(Reset) request with that ID
#[tokio::test]
async fn test_device_client_reset() {
    let mut mock = actor_framework::MockActorClient::<DeviceActor>::new();
    let device_id = DeviceId(1);

    mock.expect_perform_action()
        .withf(move |id, action| {
            *id == Some(device_id) && matches!(action, device_api::DeviceAction::Reset)
        })
        .returning(|_, _| Ok(DeviceActionResult::Success));

    let client = DeviceClient::new(Box::new(mock));
    client.reset(Some(device_id)).await.unwrap();
}

// Scenario: Client correctly serializes Global Reset requests
//   Given a mock Actor Client expecting a PerformAction(Reset) call with None
// ID   When I call reset on the client with None
//   Then the mock receives the PerformAction(Reset) request with None
#[tokio::test]
async fn test_device_client_global_reset() {
    let mut mock = actor_framework::MockActorClient::<DeviceActor>::new();

    mock.expect_perform_action()
        .withf(move |id, action| id.is_none() && matches!(action, device_api::DeviceAction::Reset))
        .returning(|_, _| Ok(DeviceActionResult::Success));

    let client = DeviceClient::new(Box::new(mock));
    client.reset(None).await.unwrap();
}

// Scenario: client.add_chip calls AddChipByGuid on the actor
//   Given a mock Actor Client expecting a AddChipByGuid call
//   When I call add_chip
//   Then the client calls PerformAction(AddChipByGuid) on the actor
#[tokio::test]
async fn test_device_client_add_chip() {
    let mut mock = actor_framework::MockActorClient::<DeviceActor>::new();
    let device_id = DeviceId(1);
    let chip_id = ChipId(0);

    mock.expect_perform_action()
        .withf(|id, action| {
            id.is_none() && matches!(action, device_api::DeviceAction::AddChipByGuid { .. })
        })
        .returning(move |_, _| Ok(DeviceActionResult::AddChipByGuidSuccess { device_id, chip_id }));

    let client = DeviceClient::new(Box::new(mock));
    let params = create_add_chip_params("guid-1", "chip-1");

    let result_id = client.add_chip(params).await.unwrap();
    assert_eq!(result_id, device_id);
}

// Scenario: client.add_chip handles repeated calls by sending AddChipByGuid
// each time   (The logic of "existing vs new" is handled by the actor, client
// just delegates)
#[tokio::test]
async fn test_device_client_add_chip_repeated() {
    let mut mock = actor_framework::MockActorClient::<DeviceActor>::new();
    let device_id = DeviceId(1);
    let chip_id1 = ChipId(0);
    let chip_id2 = ChipId(1);

    // Expect 2 calls
    mock.expect_perform_action()
        .withf(|id, action| {
            id.is_none() && matches!(action, device_api::DeviceAction::AddChipByGuid { .. })
        })
        .times(2)
        .returning(move |_, _| {
            Ok(DeviceActionResult::AddChipByGuidSuccess { device_id, chip_id: chip_id1 })
        });

    let client = DeviceClient::new(Box::new(mock));

    let params1 = create_add_chip_params("guid-1", "chip-1");
    client.add_chip(params1).await.unwrap();

    let params2 = create_add_chip_params("guid-1", "chip-2");
    client.add_chip(params2).await.unwrap();
}

fn create_add_chip_params(guid: &str, chip_name: &str) -> device_api::DeviceAddChip {
    device_api::DeviceAddChip {
        device_guid: guid.to_string(),
        packet_stream: None,
        packet_sink: None,
        device_config: DeviceConfig::default(),
        chip: netsim_model::Chip {
            name: chip_name.to_string(),
            manufacturer: "man-1".to_string(),
            product_name: "prod-1".to_string(),
            kind: netsim_model::ChipKind::BLUETOOTH,
            variant: Some(netsim_model::ChipVariant::Bluetooth(Box::new(
                netsim_model::Bluetooth {
                    address: "00:00:00:00:00:00".to_string(),
                    mode: netsim_model::BluetoothMode::Device(Default::default()),
                    bt_properties: Default::default(),
                    ..Default::default()
                },
            ))),
            ..Default::default()
        },
    }
}
