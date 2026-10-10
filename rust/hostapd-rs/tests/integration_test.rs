// Copyright 2024 The Android Open Source Project
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

//! Integration tests for the `hostapd-rs` crate.

use bytes::Bytes;
use hostapd_rs::hostapd::Hostapd;
use log::warn;
use netsim_packets::ieee80211::Ieee80211;
use pdl_runtime::Packet;
use std::{
    env,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};
/// Initializes a `Hostapd` instance for testing.
///
/// Returns a tuple containing the `Hostapd` instance and a receiver for
/// receiving data from `hostapd`.
fn init_test_hostapd() -> (Hostapd, mpsc::Receiver<Bytes>) {
    let (tx, rx) = mpsc::channel(100);
    let config_path = env::temp_dir().join("hostapd.conf");
    (Hostapd::new(tx, true, config_path), rx)
}

/// Waits for the `Hostapd` process to terminate.
async fn terminate_hostapd(hostapd: &Hostapd) {
    hostapd.terminate().await;
    let max_wait_time = Duration::from_secs(30);
    let start_time = Instant::now();
    while start_time.elapsed() < max_wait_time {
        if !hostapd.is_running().await {
            break;
        }
        sleep(Duration::from_millis(250)).await; // Using tokio::time::sleep now
    }
    warn!("Hostapd failed to terminate successfully within 30s");
}

/// Hostapd integration test.
///
/// A single test is used to avoid conflicts when multiple `hostapd` instances
/// run in parallel.
///
/// Multi threaded tokio runtime is required for hostapd.
///
/// TODO: Split up tests once feasible with `serial_test` crate or other methods.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_hostapd() {
    // Initialize a single Hostapd instance to share across tests to avoid >5s startup &
    // shutdown overhead for every test
    let (mut hostapd, mut receiver) = init_test_hostapd();
    test_start(&mut hostapd).await;
    test_receive_beacon_frame(&mut receiver).await;
    test_get_and_set_ssid(&mut hostapd, &mut receiver).await;
    test_terminate(&hostapd).await;
}

/// Tests that `Hostapd` starts successfully.
async fn test_start(hostapd: &mut Hostapd) {
    hostapd.run().await;
    assert!(hostapd.is_running().await);
}

/// Tests that `Hostapd` terminates successfully.
async fn test_terminate(hostapd: &Hostapd) {
    terminate_hostapd(&hostapd).await;
    assert!(!hostapd.is_running().await);
}

/// Tests whether a beacon frame packet is received after `Hostapd` starts up.
async fn test_receive_beacon_frame(receiver: &mut mpsc::Receiver<Bytes>) {
    let timeout_duration = Duration::from_secs(10);
    match timeout(timeout_duration, receiver.recv()).await {
        // Using tokio::time::timeout
        Ok(Some(packet)) if Ieee80211::decode_full(&packet).unwrap().is_beacon() => {}
        Ok(Some(_)) => assert!(false, "Received a non beacon packet within timeout"),
        Ok(None) => {
            assert!(false, "Sender closed unexpectedly before beacon received within timeout")
        }
        Err(_timeout_err) => assert!(false, "Did not receive beacon frame in 10s timeout"),
    }
}

/// Checks if the receiver receives a beacon frame with the specified SSID within 10 seconds.
async fn verify_beacon_frame_ssid(receiver: &mut mpsc::Receiver<Bytes>, ssid: &str) {
    let timeout_duration = Duration::from_secs(10);
    match timeout(timeout_duration, receiver.recv()).await {
        // Using tokio::time::timeout
        Ok(Some(packet)) => {
            if let Ok(beacon_ssid) =
                Ieee80211::decode_full(&packet).unwrap().get_ssid_from_beacon_frame()
            {
                if beacon_ssid == ssid {
                    return; // Found expected beacon frame
                }
            }
            assert!(false, "Received non-matching beacon frame within timeout");
        }
        Ok(None) => {
            assert!(false, "Sender closed before expected beacon frame received within timeout")
        }
        Err(_timeout_err) => assert!(false, "No Beacon frame received within 10s timeout"),
    }
}

/// Tests various ways to configure `Hostapd` SSID and password.
async fn test_get_and_set_ssid(hostapd: &mut Hostapd, receiver: &mut mpsc::Receiver<Bytes>) {
    // Check default ssid is set
    let default_ssid = "AndroidWifi";
    assert_eq!(hostapd.get_ssid(), default_ssid);

    let mut test_ssid = String::new();
    let mut test_password = String::new();
    // Verify set_ssid fails if SSID is empty
    assert!(hostapd.set_ssid(&test_ssid, &test_password).await.is_err());

    // Verify set_ssid succeeds if SSID is not empty
    test_ssid = "TestSsid".to_string();
    assert!(hostapd.set_ssid(&test_ssid, &test_password).await.is_ok());
    // Verify hostapd sends new beacon frame with updated SSID
    verify_beacon_frame_ssid(receiver, &test_ssid).await;

    // Verify ssid was set successfully
    assert_eq!(hostapd.get_ssid(), test_ssid);

    // Verify setting same ssid again succeeds
    assert!(hostapd.set_ssid(&test_ssid, &test_password).await.is_ok());

    test_ssid = "EncryptedSsid".to_string();
    test_password = "TestPassword".to_string();

    // Verify set_ssid with password succeeds
    assert!(hostapd.set_ssid(&test_ssid, &test_password).await.is_ok());

    // Verify ssid was set successfully
    assert_eq!(hostapd.get_ssid(), test_ssid);

    // Verify hostapd sends new beacon frame with updated SSID
    verify_beacon_frame_ssid(receiver, &test_ssid).await;
}
