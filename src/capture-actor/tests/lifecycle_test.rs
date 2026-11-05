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

use netsim_model::ChipKind;

use crate::world::World;

// Feature: Capture Entity Lifecycle
//
//   As a client
//   I want to manage the lifecycle of a capture session
//   So that I can start, stop, and retrieve information about packet captures

// Scenario: Full lifecycle of a capture entity
#[tokio::test]
async fn test_capture_entity_lifecycle() {
    let world = World::new().await;
    let chip_id = 1;

    // Given: a capture is created but disabled
    world.when_create_capture(chip_id, ChipKind::BLUETOOTH, "test_device", false).await.unwrap();
    world.then_capture_is_enabled(chip_id, false).await;
    world.then_capture_stats_are(chip_id, 0, 0).await;
    let info = world.client.get_capture(netsim_model::ChipId(chip_id)).await.unwrap().unwrap();
    assert_eq!(info.seconds, 0);
    assert_eq!(info.nanos, 0);

    // When: the capture is enabled
    world.when_update_capture(chip_id, true).await.unwrap();
    world.then_capture_is_enabled(chip_id, true).await;
    let info = world.client.get_capture(netsim_model::ChipId(chip_id)).await.unwrap().unwrap();
    assert!(info.seconds > 0);

    // And: a packet is sent
    world.when_dummy_packet_is_sent(chip_id).await;

    // Then: stats should be updated
    world.then_capture_stats_are(chip_id, 1, 8).await;

    // When: capture is disabled
    world.when_update_capture(chip_id, false).await.unwrap();
    world.then_capture_is_enabled(chip_id, false).await;

    // And: capture is deleted
    world.when_delete_capture(chip_id).await.unwrap();
    world.then_capture_is_none(chip_id).await;
}

// Scenario: Capture is enabled by default context setting
#[tokio::test]
async fn test_default_capture_enabled() {
    // Given: the actor is configured to enable captures by default
    let world = World::new_with_default(true).await;
    let chip_id = 2;

    // When: a capture is created with `enabled` = false
    world
        .when_create_capture(chip_id, ChipKind::BLUETOOTH, "test_device_default", false)
        .await
        .unwrap();

    // Then: it is actually enabled due to the context default
    world.then_capture_is_enabled(chip_id, true).await;
}

// Scenario: Capture file is created in the configured directory
#[tokio::test]
async fn test_capture_directory() {
    let world = World::new().await;
    let chip_id = 3;

    // When: an enabled capture is created
    world.when_create_capture(chip_id, ChipKind::BLUETOOTH, "test_device_dir", true).await.unwrap();

    // Then: a file is created in the temp directory
    world.then_capture_file_exists().await;
}

// Scenario: List and patch captures
#[tokio::test]
async fn test_list_and_patch_captures() {
    let world = World::new().await;

    // Given: two captures created with different initial states
    world.when_create_capture(11, ChipKind::BLUETOOTH, "bt_device", false).await.unwrap();
    world.when_create_capture(12, ChipKind::NFC, "nfc_device", true).await.unwrap();

    // When: listing captures
    let captures = world.client.list_captures().await.unwrap();

    // Then: both captures are present with matching initial states
    assert_eq!(captures.len(), 2);
    let bt_cap = captures.iter().find(|c| c.chip_id == netsim_model::ChipId(11)).unwrap();
    assert!(!bt_cap.enabled);
    let nfc_cap = captures.iter().find(|c| c.chip_id == netsim_model::ChipId(12)).unwrap();
    assert!(nfc_cap.enabled);

    // When: patching capture state via patch_capture
    world.client.patch_capture(netsim_model::ChipId(11), true).await.unwrap();
    world.client.patch_capture(netsim_model::ChipId(12), false).await.unwrap();

    // Then: states are updated accordingly
    world.then_capture_is_enabled(11, true).await;
    world.then_capture_is_enabled(12, false).await;
}

// Scenario: Device name containing illegal filesystem characters is sanitized
#[tokio::test]
async fn test_device_name_character_sanitization() {
    let world = World::new().await;
    let chip_id = 42;

    // Given: an enabled capture session created with illegal filesystem characters in device name
    world
        .when_create_capture(chip_id, ChipKind::BLUETOOTH, "radio/dev:0 test", true)
        .await
        .unwrap();

    // Then: the resulting capture file path is sanitized to avoid directory separators or colons
    let expected_path = world.temp_dir.join("netsim-42-radio_dev_0_test-BLUETOOTH.pcap");
    world.then_capture_filepath_is(chip_id, &expected_path).await;
}

/// RAII guard to clean up test temporary directory on Drop even on panic.
struct TempDirGuard(std::path::PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

// Scenario: Global SetCaptureDirectory dynamically changes destination directory
#[tokio::test]
async fn test_dynamic_set_capture_directory() {
    let world = World::new().await;
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let new_dir =
        std::env::temp_dir().join(format!("netsim-capture-test-{}-{}", std::process::id(), nanos));
    std::fs::create_dir_all(&new_dir).unwrap();
    let _guard = TempDirGuard(new_dir.clone());

    // When: the global capture directory is reconfigured
    world.when_set_capture_directory(new_dir.clone()).await;

    let chip_id = 10;
    world.when_create_capture(chip_id, ChipKind::BLUETOOTH, "test_dyn_dev", true).await.unwrap();

    // Then: subsequent captures are written to the new destination directory
    let expected_path = new_dir.join("netsim-10-test_dyn_dev-BLUETOOTH.pcap");
    world.then_capture_filepath_is(chip_id, &expected_path).await;
    assert_eq!(std::fs::read_dir(&world.temp_dir).unwrap().count(), 0);
}

// Scenario: Operations on non-existent ChipId return ChipNotFound
#[tokio::test]
async fn test_unregistered_chip_id_operations_fail() {
    let world = World::new().await;
    let invalid_id = 999;

    // When: updating an unregistered ChipId, Then: ChipNotFound error is returned
    let update_res = world.when_update_capture(invalid_id, true).await;
    world.then_capture_error_is_chip_not_found(update_res, invalid_id).await;

    // When: deleting an unregistered ChipId, Then: ChipNotFound error is returned
    let delete_res = world.when_delete_capture(invalid_id).await;
    world.then_capture_error_is_chip_not_found(delete_res, invalid_id).await;
}
