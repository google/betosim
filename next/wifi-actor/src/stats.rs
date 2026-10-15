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

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use netsim_proto::stats::{
    WifiApiStats, WifiAwareDiscoverySessionStats, WifiAwareNdpStats, WifiAwareSessionStats,
    WifiEasyConnectStats, WifiP2pManagerStats, WifiRttManagerStats,
};
use tracing::{debug, warn};

use crate::error::WifiError;

pub trait Clock: Send + Sync + std::fmt::Debug {
    fn now_millis(&self) -> u64;
}

#[derive(Debug, Clone)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
    }
}

#[derive(Debug, Clone)]
pub struct MockClock {
    time_millis: Arc<std::sync::atomic::AtomicU64>,
}

impl Default for MockClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MockClock {
    pub fn new() -> Self {
        Self { time_millis: Arc::new(std::sync::atomic::AtomicU64::new(0)) }
    }

    pub fn store(&self, time: u64) {
        self.time_millis.store(time, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Clock for MockClock {
    fn now_millis(&self) -> u64 {
        self.time_millis.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Stats for the Wifi Actor.
///
/// Tracks error counts, packet counts, and throughput metrics.
/// Designed for single-threaded usage within the actor (no internal locking).
#[derive(Clone, Debug)]
pub struct WifiStats {
    counts: WifiCounts,
    values: WifiValues,
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiApi {
    // Wi-Fi Aware (NAN)
    WifiAwareSessionPublishUnsolicited = 0,
    WifiAwareSessionPublishSolicited = 1,
    WifiAwareSessionSubscribeActive = 2,
    WifiAwareSessionSubscribePassive = 3,
    WifiAwareDiscoverySessionSendMessage = 4,
    WifiAwareNdpRequest = 5,
    WifiAwareNdpResponse = 6,
    WifiAwareNdpConfirm = 7,
    WifiAwareNdpTerminate = 8,

    // Wi-Fi Direct (P2P)
    WifiP2pManagerCreateGroup = 9,
    WifiP2pManagerConnect = 10,
    WifiP2pManagerDiscoverPeers = 11,
    WifiP2pManagerDiscoverServices = 12,

    // Wi-Fi RTT (FTM)
    WifiRttManagerStartRanging = 13,
    WifiRttManagerOnRangingResults = 14,

    // Wi-Fi Easy Connect (DPP)
    WifiEasyConnectAuthRequest = 15,
    WifiEasyConnectAuthResponse = 16,
    WifiEasyConnectAuthConfirm = 17,

    Count = 18,
}

#[derive(Clone, Debug, Default)]
struct WifiApiArray {
    data: [u64; WifiApi::Count as usize],
}

impl WifiApiArray {
    fn get(&self, index: WifiApi) -> u64 {
        self.data.get(index as usize).copied().unwrap_or(0)
    }

    fn get_mut(&mut self, index: WifiApi) -> Option<&mut u64> {
        self.data.get_mut(index as usize)
    }
}

#[derive(Clone, Debug, Default)]
struct WifiCounts {
    hostapd_error: u64,
    network_error: u64,
    client_error: u64,
    frame_error: u64,
    transmission_error: u64,
    other_error: u64,

    hwsim_frames_rx: u64,
    hwsim_frames_tx: u64,
    network_packets_tx: u64,
    network_packets_rx: u64,
    hostapd_frames_tx: u64,
    hostapd_frames_rx: u64,
    wmedium_frames_tx: u64,
    wmedium_unicast_frames_tx: u64,
    mgmt_frames_rx: u64,
    mdns_count: u64,

    wifi_apis: WifiApiArray,
}

#[derive(Clone, Debug)]
struct ThroughputValues {
    clock: Arc<dyn Clock>,
    max_throughput: u64,
    window_start: u64,
    window_bytes: u64,
}

impl ThroughputValues {
    fn new(clock: Arc<dyn Clock>) -> Self {
        Self { clock, max_throughput: 0, window_start: 0, window_bytes: 0 }
    }
}

#[derive(Clone, Debug)]
struct WifiValues {
    download_throughput: ThroughputValues,
    upload_throughput: ThroughputValues,
}

impl WifiValues {
    fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            download_throughput: ThroughputValues::new(clock.clone()),
            upload_throughput: ThroughputValues::new(clock),
        }
    }
}

impl WifiStats {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { counts: Default::default(), values: WifiValues::new(clock) }
    }
    pub fn record_download_bytes(&mut self, bytes: usize) {
        self.values.download_throughput.record(bytes, "Download");
    }

    pub fn record_upload_bytes(&mut self, bytes: usize) {
        self.values.upload_throughput.record(bytes, "Upload");
    }

    pub fn log_and_incr_err_count(&mut self, error: &WifiError) {
        warn!("{error}");
        match error {
            WifiError::Hostapd(_) => self.counts.hostapd_error += 1,
            WifiError::Network(_) => self.counts.network_error += 1,
            WifiError::Client(_) => self.counts.client_error += 1,
            WifiError::Frame(_) => self.counts.frame_error += 1,
            WifiError::Transmission(_) => self.counts.transmission_error += 1,
            WifiError::Chip(_) => self.counts.other_error += 1,
            WifiError::Other(_) => self.counts.other_error += 1,
            WifiError::Internal(_) => self.counts.other_error += 1,
        }
    }

    /// Handles the "log on error, increment on success" pattern.
    pub fn log_outcome<T, E, F>(&mut self, result: Result<T, E>, success_op: F)
    where
        E: Into<WifiError>,
        F: FnOnce(&mut Self, T),
    {
        match result {
            Ok(val) => success_op(self, val),
            Err(e) => self.log_and_incr_err_count(&e.into()),
        }
    }

    pub fn incr_hwsim_frames_rx(&mut self) {
        self.counts.hwsim_frames_rx += 1;
    }

    pub fn incr_hwsim_frames_tx(&mut self) {
        self.counts.hwsim_frames_tx += 1;
    }

    pub fn incr_network_packets_tx(&mut self) {
        self.counts.network_packets_tx += 1;
    }

    pub fn incr_network_packets_rx(&mut self) {
        self.counts.network_packets_rx += 1;
    }

    pub fn incr_hostapd_frames_tx(&mut self) {
        self.counts.hostapd_frames_tx += 1;
    }

    pub fn incr_hostapd_frames_rx(&mut self) {
        self.counts.hostapd_frames_rx += 1;
    }

    pub fn incr_wmedium_frames_tx(&mut self) {
        self.counts.wmedium_frames_tx += 1;
    }

    pub fn incr_wmedium_unicast_frames_tx(&mut self) {
        self.counts.wmedium_unicast_frames_tx += 1;
    }

    pub fn incr_mgmt_frames_rx(&mut self) {
        self.counts.mgmt_frames_rx += 1;
    }

    pub fn incr_mdns_count(&mut self) {
        self.counts.mdns_count += 1;
    }

    pub fn incr_wifi_api(&mut self, api: WifiApi) {
        if let Some(count) = self.counts.wifi_apis.get_mut(api) {
            *count += 1;
        }
    }

    pub fn bytes_ps_to_mbps(bytes_per_second: u64) -> f32 {
        ((bytes_per_second as f64 * 8.0) / (1_000_000.0)) as f32
    }

    pub fn to_proto(&self) -> netsim_proto::stats::WifiIpcStats {
        let mut wifi_stats = netsim_proto::stats::WifiStats::new();

        let saturate = |val: u64| -> i32 { std::cmp::min(val, i32::MAX as u64) as i32 };

        wifi_stats.set_hostapd_errors(saturate(self.counts.hostapd_error));
        wifi_stats.set_network_errors(saturate(self.counts.network_error));
        wifi_stats.set_client_errors(saturate(self.counts.client_error));
        wifi_stats.set_frame_errors(saturate(self.counts.frame_error));
        wifi_stats.set_transmission_errors(saturate(self.counts.transmission_error));
        wifi_stats.set_other_errors(saturate(self.counts.other_error));

        wifi_stats.set_hwsim_frames_rx(saturate(self.counts.hwsim_frames_rx));
        wifi_stats.set_hwsim_frames_tx(saturate(self.counts.hwsim_frames_tx));
        wifi_stats.set_network_packets_tx(saturate(self.counts.network_packets_tx));
        wifi_stats.set_network_packets_rx(saturate(self.counts.network_packets_rx));
        wifi_stats.set_hostapd_frames_tx(saturate(self.counts.hostapd_frames_tx));
        wifi_stats.set_hostapd_frames_rx(saturate(self.counts.hostapd_frames_rx));
        wifi_stats.set_wmedium_frames_tx(saturate(self.counts.wmedium_frames_tx));
        wifi_stats.set_wmedium_unicast_frames_tx(saturate(self.counts.wmedium_unicast_frames_tx));
        wifi_stats.set_mgmt_frames_rx(saturate(self.counts.mgmt_frames_rx));
        wifi_stats.set_mdns_count(saturate(self.counts.mdns_count));

        let mut wifi = WifiApiStats::new();
        let get_api = |api: WifiApi| -> i32 { saturate(self.counts.wifi_apis.get(api)) };

        let mut wifi_aware_session = WifiAwareSessionStats::new();
        wifi_aware_session
            .set_publish_unsolicited(get_api(WifiApi::WifiAwareSessionPublishUnsolicited));
        wifi_aware_session
            .set_publish_solicited(get_api(WifiApi::WifiAwareSessionPublishSolicited));
        wifi_aware_session.set_subscribe_active(get_api(WifiApi::WifiAwareSessionSubscribeActive));
        wifi_aware_session
            .set_subscribe_passive(get_api(WifiApi::WifiAwareSessionSubscribePassive));
        wifi.wifi_aware_session = netsim_proto::protobuf::MessageField::some(wifi_aware_session);

        let mut wifi_aware_discovery_session = WifiAwareDiscoverySessionStats::new();
        wifi_aware_discovery_session
            .set_send_message(get_api(WifiApi::WifiAwareDiscoverySessionSendMessage));
        wifi.wifi_aware_discovery_session =
            netsim_proto::protobuf::MessageField::some(wifi_aware_discovery_session);

        let mut wifi_aware_ndp = WifiAwareNdpStats::new();
        wifi_aware_ndp.set_request(get_api(WifiApi::WifiAwareNdpRequest));
        wifi_aware_ndp.set_response(get_api(WifiApi::WifiAwareNdpResponse));
        wifi_aware_ndp.set_confirm(get_api(WifiApi::WifiAwareNdpConfirm));
        wifi_aware_ndp.set_terminate(get_api(WifiApi::WifiAwareNdpTerminate));
        wifi.wifi_aware_ndp = netsim_proto::protobuf::MessageField::some(wifi_aware_ndp);

        let mut wifi_p2p_manager = WifiP2pManagerStats::new();
        wifi_p2p_manager.set_create_group(get_api(WifiApi::WifiP2pManagerCreateGroup));
        wifi_p2p_manager.set_connect(get_api(WifiApi::WifiP2pManagerConnect));
        wifi_p2p_manager.set_discover_peers(get_api(WifiApi::WifiP2pManagerDiscoverPeers));
        wifi_p2p_manager.set_discover_services(get_api(WifiApi::WifiP2pManagerDiscoverServices));
        wifi.wifi_p2p_manager = netsim_proto::protobuf::MessageField::some(wifi_p2p_manager);

        let mut wifi_rtt_manager = WifiRttManagerStats::new();
        wifi_rtt_manager.set_start_ranging(get_api(WifiApi::WifiRttManagerStartRanging));
        wifi_rtt_manager.set_on_ranging_results(get_api(WifiApi::WifiRttManagerOnRangingResults));
        wifi.wifi_rtt_manager = netsim_proto::protobuf::MessageField::some(wifi_rtt_manager);

        let mut wifi_easy_connect = WifiEasyConnectStats::new();
        wifi_easy_connect.set_auth_request(get_api(WifiApi::WifiEasyConnectAuthRequest));
        wifi_easy_connect.set_auth_response(get_api(WifiApi::WifiEasyConnectAuthResponse));
        wifi_easy_connect.set_auth_confirm(get_api(WifiApi::WifiEasyConnectAuthConfirm));
        wifi.wifi_easy_connect = netsim_proto::protobuf::MessageField::some(wifi_easy_connect);

        wifi_stats.set_max_download_throughput(Self::bytes_ps_to_mbps(
            self.values.download_throughput.max_throughput,
        ));
        wifi_stats.set_max_upload_throughput(Self::bytes_ps_to_mbps(
            self.values.upload_throughput.max_throughput,
        ));

        let mut ipc_stats = netsim_proto::stats::WifiIpcStats::new();
        ipc_stats.wifi_stats = netsim_proto::protobuf::MessageField::some(wifi_stats);
        ipc_stats.wifi_api_stats = netsim_proto::protobuf::MessageField::some(wifi);

        ipc_stats
    }
}

impl ThroughputValues {
    fn record(&mut self, bytes: usize, name: &str) {
        const WINDOW_MILLIS: u64 = Duration::from_secs(5).as_millis() as u64;
        let now = self.clock.now_millis();
        let start_time = self.window_start;

        if start_time == 0 {
            self.window_start = now;
            self.window_bytes = bytes as u64;
        } else if now.saturating_sub(start_time) < WINDOW_MILLIS {
            self.window_bytes += bytes as u64;
        } else {
            let window_bytes = self.window_bytes;
            let elapsed = now.saturating_sub(start_time) as f64 / 1000.0;
            let current_throughput = (window_bytes as f64 / elapsed) as u64;
            debug!(
                "{} Throughput: {:.1} Mbits/sec",
                name,
                WifiStats::bytes_ps_to_mbps(current_throughput)
            );
            if current_throughput > self.max_throughput {
                self.max_throughput = current_throughput;
            }
            self.window_start = now;
            self.window_bytes = bytes as u64;
        }
    }
}
