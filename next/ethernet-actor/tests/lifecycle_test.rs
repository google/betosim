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

use super::world::World;

#[tokio::test]
async fn test_create_ethernet_chip_stores_chip_under_requested_id() {
    let mut world = World::new().await;
    world.when_create_chip(11, ChipKind::ETHERNET).await;
    world.then_chip_matches(11, ChipKind::ETHERNET, "eth0", true, None).await;
    world.then_chip_count_is(1).await;
}

#[tokio::test]
async fn test_create_cellular_data_chip_is_accepted() {
    let mut world = World::new().await;
    world.when_create_chip(12, ChipKind::CELLULAR_DATA).await;
    world.then_chip_matches(12, ChipKind::CELLULAR_DATA, "eth0", true, None).await;
    world.then_chip_count_is(1).await;
}

#[tokio::test]
async fn test_create_rejects_unsupported_chip_kinds() {
    let mut world = World::new().await;
    for kind in [ChipKind::BLUETOOTH, ChipKind::WIFI, ChipKind::UWB] {
        world.when_create_chip(13, kind).await;
        world.then_last_error_contains("Invalid ChipKind");
    }
    world.then_chip_count_is(0).await;
    world.then_chip_not_found(13).await;
}

#[tokio::test]
async fn test_unknown_chip_operations_rejected_without_side_effects() {
    let mut world = World::new().await;

    world.when_read_chip(99).await;
    world.then_last_error_is_chip_not_found(99);

    world.when_update_chip(99, Some("eth1"), Some(false), Some(false)).await;
    world.then_last_error_contains("Chip 99 not found");

    world.when_reset_chip(99).await;
    world.then_last_error_contains("Chip 99 not found");

    world.when_delete_chip(99).await;
    world.then_last_operation_succeeded();

    world.then_chip_count_is(0).await;
    world.then_no_device_notification().await;
}

#[tokio::test]
async fn test_update_persists_name_enabled_and_radio_state() {
    let mut world = World::new().await;
    world.given_chip(14, ChipKind::ETHERNET).await;
    world.when_update_chip(14, Some("eth1"), Some(false), Some(false)).await;
    world.then_chip_matches(14, ChipKind::ETHERNET, "eth1", false, Some(false)).await;
}

#[tokio::test]
async fn test_reset_returns_active_chip() {
    let mut world = World::new().await;
    world.given_chip(15, ChipKind::ETHERNET).await;
    world.when_reset_chip(15).await;
    world.then_last_reset_chip_matches(15, ChipKind::ETHERNET, "eth0", true);
}

#[tokio::test]
async fn test_delete_removes_chip_and_notifies_device() {
    let mut world = World::new().await;
    world.given_chip(16, ChipKind::ETHERNET).await;
    world.when_delete_chip(16).await;
    world.then_chip_count_is(0).await;
    world.then_chip_not_found(16).await;
    world.then_device_notified_chip_removed(16).await;
}

#[tokio::test]
async fn test_statistics_reporting_and_shutdown() {
    let mut world = World::new().await;
    world.given_chip(17, ChipKind::ETHERNET).await;

    world.when_read_statistics().await;
    world.then_statistics_are_empty();

    world.when_get_global_stats().await;
    world.then_global_stats_reported();

    world.when_shutdown().await;
    world.then_last_operation_succeeded();
}
