// Copyright 2025 The Android Open Source Project
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

//! Wi-Fi Statistics Module
//! This module provides structures and functions for tracking and managing Wi-Fi related statistics.

use crate::wifi::error::WifiError;
use log::{debug, warn};
use netsim_proto::stats::WifiStats as ProtoWifiStats;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Default)]
pub struct WifiStats {
    counts: Arc<WifiCounts>,
    values: Arc<WifiValues>,
}

impl Clone for WifiStats {
    fn clone(&self) -> Self {
        WifiStats { counts: self.counts.clone(), values: self.values.clone() }
    }
}

#[derive(Debug, Default)]
struct WifiCounts {
    // === Error Counters (Proto fields 1-6) ===
    hostapd_error: AtomicU32,
    network_error: AtomicU32,
    client_error: AtomicU32,
    frame_error: AtomicU32,
    transmission_error: AtomicU32,
    other_error: AtomicU32,

    // === Core Traffic Flow & Type Counters (Proto fields 7-14) ===
    hwsim_frames_rx: AtomicU32,
    hwsim_frames_tx: AtomicU32,
    network_packets_tx: AtomicU32,
    network_packets_rx: AtomicU32,
    hostapd_frames_tx: AtomicU32,
    hostapd_frames_rx: AtomicU32,
    wmedium_frames_tx: AtomicU32,
    wmedium_unicast_frames_tx: AtomicU32,
    mgmt_frames_rx: AtomicU32,

    // === Specific Protocol Counters (Proto fields 15-16) ===
    // TODO: Identify Wi-Fi Direct (P2P) frames in medium.rs and increment this counter.
    mdns_count: AtomicU32,
}

#[derive(Debug, Default)]
struct ThroughputValues {
    max_throughput: AtomicU32, // Max throughput in Mbits/sec
    window_start: AtomicU64,   // Start of the current window as milliseconds since epoch
    window_bytes: AtomicU64,   // Total number of bytes in the current window
}

#[derive(Debug, Default)]
struct WifiValues {
    // Fields for throughput values
    download_throughput: ThroughputValues,
    upload_throughput: ThroughputValues,
}

// Define the macro to generate incrementer methods
macro_rules! impl_incr_method {
    ($method:ident, $field:ident) => {
        pub fn $method(&self) {
            self.counts.$field.fetch_add(1, Ordering::Relaxed);
        }
    };
}

impl WifiStats {
    fn record_throughput(&self, throughput_values: &ThroughputValues, bytes: usize, name: &str) {
        const WINDOW_MILLIS: u64 = Duration::from_secs(5).as_millis() as u64;
        let now =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        let start_time = throughput_values.window_start.load(Ordering::Relaxed);

        if start_time == 0 {
            // First packet, initialize the window start time and bytes
            throughput_values.window_start.store(now, Ordering::Relaxed);
            throughput_values.window_bytes.store(bytes as u64, Ordering::Relaxed);
        } else if now.saturating_sub(start_time) < WINDOW_MILLIS {
            // Within the current window, just add the bytes
            throughput_values.window_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
        } else {
            // Window expired, calculate throughput and reset window
            let window_bytes = throughput_values.window_bytes.load(Ordering::Relaxed);
            let elapsed = now.saturating_sub(start_time) as f64 / 1000.0;
            let current_throughput = (window_bytes as f64 / elapsed) as u32;
            debug!(
                "{} Throughput Result: Interval: {:.2} sec, Transfer: {:.2} MBytes, Current Throughput: {:.1} Mbits/sec, Previous Max Throughput: {:.1} Mbits/s",
                name,
                elapsed,
                (window_bytes as f64) / (1024.0 * 1024.0),
                Self::bytes_ps_to_mbps(current_throughput),
                Self::bytes_ps_to_mbps(throughput_values.max_throughput.load(Ordering::Relaxed))
            );
            // Store current max throughput
            throughput_values.max_throughput.fetch_max(current_throughput, Ordering::Relaxed);

            // Start a new window with the current packet's bytes
            throughput_values.window_start.store(now, Ordering::Relaxed);
            throughput_values.window_bytes.store(bytes as u64, Ordering::Relaxed);
        }
    }

    pub fn record_download_bytes(&self, bytes: usize) {
        self.record_throughput(&self.values.download_throughput, bytes, "Download");
    }

    pub fn record_upload_bytes(&self, bytes: usize) {
        self.record_throughput(&self.values.upload_throughput, bytes, "Upload");
    }

    /// Logs the error and increments the corresponding counter.
    pub fn log_and_incr_err_count(&self, error: &WifiError) {
        warn!("{error}");
        let counter = match error {
            WifiError::Hostapd(_) => &self.counts.hostapd_error,
            WifiError::Network(_) => &self.counts.network_error,
            WifiError::Client(_) => &self.counts.client_error,
            WifiError::Frame(_) => &self.counts.frame_error,
            WifiError::Transmission(_) => &self.counts.transmission_error,
            _ => &self.counts.other_error,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    // Use the macro to generate the incrementer methods for usage counters
    impl_incr_method!(incr_hwsim_frames_rx, hwsim_frames_rx);
    impl_incr_method!(incr_hwsim_frames_tx, hwsim_frames_tx);
    impl_incr_method!(incr_network_packets_tx, network_packets_tx);
    impl_incr_method!(incr_network_packets_rx, network_packets_rx);
    impl_incr_method!(incr_hostapd_frames_tx, hostapd_frames_tx);
    impl_incr_method!(incr_hostapd_frames_rx, hostapd_frames_rx);
    impl_incr_method!(incr_wmedium_frames_tx, wmedium_frames_tx);
    impl_incr_method!(incr_wmedium_unicast_frames_tx, wmedium_unicast_frames_tx);
    impl_incr_method!(incr_mgmt_frames_rx, mgmt_frames_rx);
    impl_incr_method!(incr_mdns_count, mdns_count);

    /// Helper function to convert bytes per second to megabits per second.
    pub fn bytes_ps_to_mbps(bytes_per_second: u32) -> f32 {
        ((bytes_per_second as f64 * 8.0) / (1_000_000.0)) as f32
    }
}

fn load_as_option_i32(counter: &AtomicU32) -> Option<i32> {
    Some(counter.load(Ordering::Relaxed) as i32)
}

impl From<&WifiStats> for ProtoWifiStats {
    fn from(wifi_stats: &WifiStats) -> Self {
        let counts = &wifi_stats.counts;
        let values = &wifi_stats.values;
        let to_mbps =
            |bytes: &AtomicU32| Some(WifiStats::bytes_ps_to_mbps(bytes.load(Ordering::Relaxed)));
        ProtoWifiStats {
            // Errors
            hostapd_errors: load_as_option_i32(&counts.hostapd_error),
            network_errors: load_as_option_i32(&counts.network_error),
            client_errors: load_as_option_i32(&counts.client_error),
            frame_errors: load_as_option_i32(&counts.frame_error),
            transmission_errors: load_as_option_i32(&counts.transmission_error),
            other_errors: load_as_option_i32(&counts.other_error),

            // Core Flow & Type
            hwsim_frames_rx: load_as_option_i32(&counts.hwsim_frames_rx),
            hwsim_frames_tx: load_as_option_i32(&counts.hwsim_frames_tx),
            network_packets_tx: load_as_option_i32(&counts.network_packets_tx),
            network_packets_rx: load_as_option_i32(&counts.network_packets_rx),
            hostapd_frames_tx: load_as_option_i32(&counts.hostapd_frames_tx),
            hostapd_frames_rx: load_as_option_i32(&counts.hostapd_frames_rx),
            wmedium_frames_tx: load_as_option_i32(&counts.wmedium_frames_tx),
            wmedium_unicast_frames_tx: load_as_option_i32(&counts.wmedium_unicast_frames_tx),
            mgmt_frames_rx: load_as_option_i32(&counts.mgmt_frames_rx),

            // Specific Protocols
            mdns_count: load_as_option_i32(&counts.mdns_count),

            // Performance data
            max_download_throughput: to_mbps(&values.download_throughput.max_throughput),
            max_upload_throughput: to_mbps(&values.upload_throughput.max_throughput),
            ..Default::default()
        }
    }
}
