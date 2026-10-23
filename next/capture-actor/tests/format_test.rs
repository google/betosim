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

use capture_actor::DataLinkType;
use netsim_model::ChipKind;

use crate::world::World;

// Feature: Capture Packet Formats
//
//   As a client
//   I want to capture packets for different chip types
//   So that PCAP files are generated with the correct format and encapsulation

// Scenario: UWB packet is captured correctly
#[tokio::test]
async fn test_uwb_capture() {
    let world = World::new().await;
    let chip_id = 4;

    // Given: an enabled UWB capture
    world.when_create_capture(chip_id, ChipKind::UWB, "test_uwb_device", true).await.unwrap();

    // When: a dummy packet is sent
    world.when_dummy_packet_is_sent(chip_id).await;

    // Then: the capture info reflects the written packet
    world.then_capture_stats_are(chip_id, 1, 4).await;

    // And: the PCAP file header has correct UWB DLT
    world.then_capture_file_has_dlt(DataLinkType::FiraUci).await;
}

// Scenario: Wi-Fi packet handles Hwsim formatting
#[tokio::test]
async fn test_wifi_capture() {
    let world = World::new().await;
    let chip_id = 5;

    // Test WIFI to ensure service routes it correctly
    for kind in [ChipKind::WIFI] {
        // Given: an enabled Wi-Fi capture
        world
            .when_create_capture(chip_id, kind, &format!("test_{:?}_device", kind), true)
            .await
            .unwrap();

        // When: an invalid dummy payload is sent
        world.when_dummy_packet_is_sent(chip_id).await;

        // Then: it is discarded during parsing, resulting in 0 records
        world.then_capture_stats_are(chip_id, 0, 0).await;

        // And: the PCAP file header has correct Wi-Fi DLT
        world.then_capture_file_has_dlt(DataLinkType::Ieee80211Radio).await;

        // Cleanup for next iteration
        world.when_delete_capture(chip_id).await.unwrap();
    }
}

// Scenario: Ethernet packet is captured correctly
#[tokio::test]
async fn test_ethernet_capture() {
    let world = World::new().await;
    let chip_id = 6;

    // Given: an enabled Ethernet capture
    world
        .when_create_capture(chip_id, ChipKind::ETHERNET, "test_ethernet_device", true)
        .await
        .unwrap();

    // When: a dummy packet is sent
    world.when_dummy_packet_is_sent(chip_id).await;

    // Then: the capture info reflects the written packet
    world.then_capture_stats_are(chip_id, 1, 4).await;

    // And: the PCAP file header has correct Ethernet DLT
    world.then_capture_file_has_dlt(DataLinkType::Ethernet).await;
}

// Scenario: Cellular Data packet is captured correctly (as Ethernet)
#[tokio::test]
async fn test_cellular_data_capture() {
    let world = World::new().await;
    let chip_id = 7;

    // Given: an enabled Cellular Data capture
    world
        .when_create_capture(chip_id, ChipKind::CELLULAR_DATA, "test_cellular_data_device", true)
        .await
        .unwrap();

    // When: a dummy packet is sent
    world.when_dummy_packet_is_sent(chip_id).await;

    // Then: the capture info reflects the written packet
    world.then_capture_stats_are(chip_id, 1, 4).await;

    // And: the PCAP file header has correct Ethernet DLT
    world.then_capture_file_has_dlt(DataLinkType::Ethernet).await;
}

// Scenario: Cellular Modem AT packet is captured correctly
#[tokio::test]
async fn test_cellular_modem_capture() {
    let world = World::new().await;
    let chip_id = 8;

    // Given: an enabled Cellular Modem capture
    world
        .when_create_capture(chip_id, ChipKind::CELLULAR, "test_cellular_modem_device", true)
        .await
        .unwrap();

    // When: a dummy packet is sent
    world.when_dummy_packet_is_sent(chip_id).await;

    // Then: the capture info reflects the written packet (4 bytes payload + 8 bytes
    // TS 27.010 framing)
    world.then_capture_stats_are(chip_id, 1, 12).await;

    // And: the PCAP file header has correct USER0 DLT (147)
    world.then_capture_file_has_dlt(DataLinkType::User0).await;
}

// Scenario: Bluetooth H4 packet is captured correctly
#[tokio::test]
async fn test_bluetooth_h4_capture() {
    let world = World::new().await;
    let chip_id = 9;

    // Given: an enabled Bluetooth capture
    world
        .when_create_capture(chip_id, ChipKind::BLUETOOTH, "test_bluetooth_device", true)
        .await
        .unwrap();

    // When: a dummy packet is sent
    world.when_dummy_packet_is_sent(chip_id).await;

    // Then: the capture info reflects the written packet
    world.then_capture_stats_are(chip_id, 1, 8).await;

    // And: the PCAP file header has correct Bluetooth H4 DLT (201)
    world.then_capture_file_has_dlt(DataLinkType::BluetoothH4).await;
}

// Scenario: NFC NCI packet is captured correctly
#[tokio::test]
async fn test_nfc_nci_capture() {
    let world = World::new().await;
    let chip_id = 10;

    // Given: an enabled NFC capture
    world.when_create_capture(chip_id, ChipKind::NFC, "test_nfc_device", true).await.unwrap();

    // When: a dummy packet is sent
    world.when_dummy_packet_is_sent(chip_id).await;

    // Then: the capture info reflects the written packet
    world.then_capture_stats_are(chip_id, 1, 4).await;

    // And: the PCAP file header has correct NFC NCI DLT (147)
    world.then_capture_file_has_dlt(DataLinkType::User0).await;
}
