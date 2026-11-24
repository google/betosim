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

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};

use capture_actor::DataLinkType;
use capture_api::{CaptureCreate, CaptureSender};
use netsim_model::{ChipId, ChipKind};
use netsim_packets::{PcapHeader, PcapRecordHeader};
use tokio::time::timeout;
use zerocopy::FromBytes;

use crate::world::World;

// Feature: Live capture stream
//
//   As a client
//   I want to subscribe to a chip's packets as a PCAP stream
//   So that I can watch traffic live without enabling file capture

const RECV_TIMEOUT: Duration = Duration::from_secs(2);
const STREAM_CHIP_ID: u32 = 1;

async fn create_capture_with_flag(world: &World, chip_id: u32) -> Arc<AtomicBool> {
    let enabled_flag = Arc::new(AtomicBool::new(false));
    let create = CaptureCreate {
        chip_kind: ChipKind::BLUETOOTH,
        device_name: "stream_device".to_string(),
        enabled_flag: enabled_flag.clone(),
    };
    CaptureSender::create_capture(&world.client, ChipId(chip_id), create).await.unwrap();
    enabled_flag
}

// Scenario: Subscriber receives header and records while file capture is off
#[tokio::test]
async fn test_stream_capture_while_disabled() {
    let world = World::new().await;
    let chip_id = 10;

    // Given: a disabled capture
    let enabled_flag = create_capture_with_flag(&world, chip_id).await;
    assert!(!enabled_flag.load(Ordering::SeqCst));

    // When: a client subscribes
    let mut rx = world.client.stream_capture(ChipId(chip_id)).await.unwrap();

    // Then: the chip's packets are routed to the capture actor
    assert!(enabled_flag.load(Ordering::SeqCst));

    // And: the first message is the PCAP global header with the H4 DLT
    let header = timeout(RECV_TIMEOUT, rx.recv()).await.unwrap().unwrap();
    let header = PcapHeader::read_from_bytes(&header).unwrap();
    assert_eq!(header.magic_number.get(), 0xa1b2c3d4);
    assert_eq!(header.network.get(), 201);

    // When: a packet is sent
    world.when_dummy_packet_is_sent(chip_id).await;

    // Then: the next message is one PCAP record containing the packet
    let record = timeout(RECV_TIMEOUT, rx.recv()).await.unwrap().unwrap();
    let (record_header, payload) = PcapRecordHeader::read_from_prefix(&record).unwrap();
    assert_eq!(record_header.incl_len.get() as usize, payload.len());
    assert_eq!(payload, &[0, 0, 0, 1, 0, 1, 2, 3]);

    // And: no file capture was written
    world.then_capture_stats_are(chip_id, 0, 0).await;
    assert!(std::fs::read_dir(&world.temp_dir).unwrap().next().is_none());
}

// Scenario: Toggling file capture while streaming keeps both working independently
#[tokio::test]
async fn test_stream_and_file_capture_coexist() {
    let world = World::new().await;
    let chip_id = 13;
    let enabled_flag = create_capture_with_flag(&world, chip_id).await;

    // Given: a live stream subscriber
    let mut rx = world.client.stream_capture(ChipId(chip_id)).await.unwrap();
    let _header = timeout(RECV_TIMEOUT, rx.recv()).await.unwrap().unwrap();

    // When: file capture is enabled and a packet is sent
    world.when_update_capture(chip_id, true).await.unwrap();
    world.when_dummy_packet_is_sent(chip_id).await;

    // Then: both the stream and the file capture receive the packet
    let _record = timeout(RECV_TIMEOUT, rx.recv()).await.unwrap().unwrap();
    world.then_capture_stats_are(chip_id, 1, 8).await;

    // When: file capture is disabled while the stream subscriber remains connected
    world.when_update_capture(chip_id, false).await.unwrap();
    assert!(enabled_flag.load(Ordering::SeqCst));

    // Then: packets continue arriving on the stream without updating the file capture
    world.when_dummy_packet_is_sent(chip_id).await;
    let _record2 = timeout(RECV_TIMEOUT, rx.recv()).await.unwrap().unwrap();
    world.then_capture_stats_are(chip_id, 0, 0).await;
}

// Scenario: Dropping the last subscriber restores the capture flag
#[tokio::test]
async fn test_stream_unsubscribe_clears_flag() {
    let world = World::new().await;
    let chip_id = 11;
    let enabled_flag = create_capture_with_flag(&world, chip_id).await;

    // Given: two subscribers
    let first = world.client.stream_capture(ChipId(chip_id)).await.unwrap();
    let mut second = world.client.stream_capture(ChipId(chip_id)).await.unwrap();

    // When: one subscriber disconnects
    drop(first);
    tokio::time::sleep(Duration::from_millis(700)).await;

    // Then: the stream stays active for the other
    assert!(enabled_flag.load(Ordering::SeqCst));
    assert!(timeout(RECV_TIMEOUT, second.recv()).await.unwrap().is_some());

    // When: the last subscriber disconnects
    drop(second);

    // Then: the flag returns to the file capture state (disabled) on the next tick
    timeout(RECV_TIMEOUT, async {
        while enabled_flag.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

// Scenario: Deleting the capture ends the stream
#[tokio::test]
async fn test_stream_ends_on_delete() {
    let world = World::new().await;
    let chip_id = 12;
    create_capture_with_flag(&world, chip_id).await;
    let mut rx = world.client.stream_capture(ChipId(chip_id)).await.unwrap();
    let _header = timeout(RECV_TIMEOUT, rx.recv()).await.unwrap().unwrap();

    world.when_delete_capture(chip_id).await.unwrap();

    assert!(timeout(RECV_TIMEOUT, rx.recv()).await.unwrap().is_none());
}

// Scenario: Subscribing to an unknown chip fails
#[tokio::test]
async fn test_stream_unknown_chip() {
    let world = World::new().await;
    assert!(world.client.stream_capture(ChipId(99)).await.is_err());
}

// Scenario: Subscribing to a Wi-Fi capture emits a Radiotap PCAP global header
#[tokio::test]
async fn test_wifi_stream_emits_radiotap_dlt_header() {
    let world = World::new().await;

    // Given: a Wi-Fi capture session
    world.when_create_capture(STREAM_CHIP_ID, ChipKind::WIFI, "wifi-dev", false).await.unwrap();

    // When: subscribing to the Wi-Fi capture stream
    let header_bytes = world.when_stream_header_received(STREAM_CHIP_ID).await;

    // Then: the PCAP global header advertises the Radiotap data link type
    world.then_pcap_header_dlt_is(&header_bytes, DataLinkType::Ieee80211Radio);
}

// Scenario: Subscribing to an NFC capture emits a User0 PCAP global header
#[tokio::test]
async fn test_nfc_stream_emits_user0_dlt_header() {
    let world = World::new().await;

    // Given: an NFC capture session
    world.when_create_capture(STREAM_CHIP_ID, ChipKind::NFC, "nfc-dev", false).await.unwrap();

    // When: subscribing to the NFC capture stream
    let header_bytes = world.when_stream_header_received(STREAM_CHIP_ID).await;

    // Then: the PCAP global header advertises the User0 data link type
    world.then_pcap_header_dlt_is(&header_bytes, DataLinkType::User0);
}

// Scenario: Pre-epoch write errors do not break subsequent stream packets
#[tokio::test]
async fn test_stream_continues_after_pre_epoch_write_errors() {
    let world = World::new().await;
    let pre_epoch = SystemTime::UNIX_EPOCH - Duration::from_secs(1);

    // Given: an active Bluetooth live capture stream
    world.when_create_capture(STREAM_CHIP_ID, ChipKind::BLUETOOTH, "bt-dev", false).await.unwrap();
    let mut rx = world.when_stream_capture_started(STREAM_CHIP_ID).await;

    // When: two invalid pre-epoch packets and one valid packet are sent
    world.when_timed_packet_is_sent(STREAM_CHIP_ID, pre_epoch, b"bad1").await;
    // Second pre-epoch write exercises the `has_warned_on_write` suppression path in `lifecycle.rs`.
    world.when_timed_packet_is_sent(STREAM_CHIP_ID, pre_epoch, b"bad2").await;
    world.when_timed_packet_is_sent(STREAM_CHIP_ID, SystemTime::now(), b"good").await;

    // Then: the valid packet still arrives on the stream
    world.then_next_stream_bt_payload_is(&mut rx, b"good").await;
}
