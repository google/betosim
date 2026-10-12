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

// Feature: Delete Device
//
//   As a user
//   I want to delete a device
//   So that I can remove it from the simulation

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use link_api::MockLinkClient;
use netsim_model::{ChipClient, ChipError, ChipId, ChipKind, ClientError, MockChipClient};

use crate::world::World;

// Scenario: Delete a device added via AddChip
//   Given a running Device Actor
//   When I add a chip (which creates a device)
//   And I delete the device
//   Then the operation succeeds
#[tokio::test]
async fn test_delete_add_chip_device_fails() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I add a chip (which creates a device)
    let device_guid = "guid-1";
    let device_id = world.when_add_chip(device_guid, "beacon").await;

    // And I delete the device
    world.when_delete_device(device_id).await;

    // Then the operation succeeds (implied by unwrap in when_delete_device)
}

// Scenario: Delete a device with chips
//   Given a running Device Actor
//   When I create a device with a chip
//   And I delete the device
//   Then the device is no longer retrievable
#[tokio::test]
async fn test_delete_device_removes_chips() {
    // Given a running Device Actor
    let world = World::new().await;

    // When I create a device with a chip
    let device_name = "test-dev";
    let device_id = world.when_create_device(device_name).await;

    // And I delete the device
    world.when_delete_device(device_id).await;

    // Then the device is no longer retrievable
    let device = world.client.get(device_id).await.unwrap();
    assert!(device.is_none());
}

// Scenario: Delete a device whose chip actor errors
//   Given a running Device Actor whose Bluetooth chip actor fails to delete
//   And the link actor rejects the first chip removal notification
//   When I delete a device with a Bluetooth chip and a Wi-Fi chip
//   Then the Wi-Fi chip is still deleted
//   And the link actor is notified about both chips
//   And the device is no longer retrievable
#[tokio::test]
async fn test_delete_device_continues_after_chip_error() {
    // Given a running Device Actor whose Bluetooth chip actor fails to delete
    let mut bt_mock = MockChipClient::new();
    bt_mock.expect_create().returning(|_, _| Ok(()));
    bt_mock.expect_read_statistics().returning(|| Ok(Box::new([])));
    bt_mock.expect_read().returning(|id| Err(ClientError::Chip(ChipError::ChipNotFound(id))));
    bt_mock.expect_delete().returning(|id| Err(ClientError::Chip(ChipError::ChipNotFound(id))));

    let wifi_deleted = Arc::new(AtomicBool::new(false));
    let mut wifi_mock = MockChipClient::new();
    wifi_mock.expect_create().returning(|_, _| Ok(()));
    wifi_mock.expect_read_statistics().returning(|| Ok(Box::new([])));
    wifi_mock.expect_get_global_stats().returning(|| Ok(None));
    wifi_mock.expect_delete().returning({
        let wifi_deleted = wifi_deleted.clone();
        move |_| {
            wifi_deleted.store(true, Ordering::SeqCst);
            Ok(())
        }
    });

    let mut chip_clients: HashMap<ChipKind, Box<dyn ChipClient>> = HashMap::new();
    chip_clients.insert(ChipKind::BLUETOOTH, Box::new(bt_mock));
    chip_clients.insert(ChipKind::WIFI, Box::new(wifi_mock));

    // And the link actor rejects the first chip removal notification
    let removed_chips = Arc::new(Mutex::new(Vec::new()));
    let mut link_mock = MockLinkClient::new();
    link_mock.expect_notify_chip_added().returning(|_, _| Ok(()));
    link_mock.expect_notify_chip_removed().returning({
        let removed_chips = removed_chips.clone();
        move |chip_id| {
            let mut removed = removed_chips.lock().unwrap();
            removed.push(chip_id);
            if removed.len() == 1 {
                Err(ClientError::Chip(ChipError::ChipNotFound(chip_id)))
            } else {
                Ok(())
            }
        }
    });

    let world = World::with_clients(chip_clients, link_mock).await;

    // When I delete a device with a Bluetooth chip and a Wi-Fi chip
    let device_guid = "guid-multi-chip";
    let bt_params = World::create_device_add_chip_params(
        device_guid.to_string(),
        "bt-chip".to_string(),
        "00:11:22:33:44:55".to_string(),
    );
    let device_id = world.client.add_chip(bt_params).await.unwrap();

    let mut wifi_params = World::create_device_add_chip_params(
        device_guid.to_string(),
        "wifi-chip".to_string(),
        String::new(),
    );
    wifi_params.chip.kind = ChipKind::WIFI;
    wifi_params.chip.variant = None;
    world.client.add_chip(wifi_params).await.unwrap();

    world.when_delete_device(device_id).await;

    // Then the Wi-Fi chip is still deleted
    assert!(wifi_deleted.load(Ordering::SeqCst));

    // And the link actor is notified about both chips
    assert_eq!(*removed_chips.lock().unwrap(), vec![ChipId(0), ChipId(1)]);

    // And the device is no longer retrievable
    world.then_device_does_not_exist(device_id).await;
}
