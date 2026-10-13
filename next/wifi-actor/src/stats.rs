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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wifi_stats_initial_state() {
        let clock = Arc::new(MockClock::new());
        let stats = WifiStats::new(clock);

        let ipc = stats.to_proto();
        let wifi = ipc.wifi_stats.as_ref().expect("wifi_stats present");
        assert_eq!(wifi.hostapd_errors(), 0);
        assert_eq!(wifi.network_errors(), 0);
        assert_eq!(wifi.client_errors(), 0);
        assert_eq!(wifi.frame_errors(), 0);
        assert_eq!(wifi.transmission_errors(), 0);
        assert_eq!(wifi.other_errors(), 0);

        assert_eq!(wifi.hwsim_frames_rx(), 0);
        assert_eq!(wifi.hwsim_frames_tx(), 0);
        assert_eq!(wifi.network_packets_tx(), 0);
        assert_eq!(wifi.network_packets_rx(), 0);
        assert_eq!(wifi.hostapd_frames_tx(), 0);
        assert_eq!(wifi.hostapd_frames_rx(), 0);
        assert_eq!(wifi.wmedium_frames_tx(), 0);
        assert_eq!(wifi.wmedium_unicast_frames_tx(), 0);
        assert_eq!(wifi.mgmt_frames_rx(), 0);
        assert_eq!(wifi.mdns_count(), 0);

        let api = ipc.wifi_api_stats.as_ref().expect("wifi_api_stats present");
        let aware = api.wifi_aware_session.as_ref().expect("wifi_aware_session present");
        assert_eq!(aware.publish_unsolicited(), 0);
        assert_eq!(aware.publish_solicited(), 0);
        assert_eq!(aware.subscribe_active(), 0);
        assert_eq!(aware.subscribe_passive(), 0);
    }

    #[test]
    fn test_wifi_stats_frame_counter_increments() {
        let clock = Arc::new(MockClock::new());
        let mut stats = WifiStats::new(clock);

        stats.incr_hwsim_frames_rx();
        stats.incr_hwsim_frames_tx();
        stats.incr_hwsim_frames_tx();
        stats.incr_network_packets_tx();
        stats.incr_network_packets_rx();
        stats.incr_hostapd_frames_tx();
        stats.incr_hostapd_frames_rx();
        stats.incr_wmedium_frames_tx();
        stats.incr_wmedium_unicast_frames_tx();
        stats.incr_mgmt_frames_rx();
        stats.incr_mdns_count();

        let ipc = stats.to_proto();
        let wifi = ipc.wifi_stats.as_ref().expect("wifi_stats present");
        assert_eq!(wifi.hwsim_frames_rx(), 1);
        assert_eq!(wifi.hwsim_frames_tx(), 2);
        assert_eq!(wifi.network_packets_tx(), 1);
        assert_eq!(wifi.network_packets_rx(), 1);
        assert_eq!(wifi.hostapd_frames_tx(), 1);
        assert_eq!(wifi.hostapd_frames_rx(), 1);
        assert_eq!(wifi.wmedium_frames_tx(), 1);
        assert_eq!(wifi.wmedium_unicast_frames_tx(), 1);
        assert_eq!(wifi.mgmt_frames_rx(), 1);
        assert_eq!(wifi.mdns_count(), 1);
    }

    #[test]
    fn test_wifi_stats_error_increments() {
        let clock = Arc::new(MockClock::new());
        let mut stats = WifiStats::new(clock);

        stats.log_and_incr_err_count(&WifiError::Hostapd("test hostapd".into()));
        stats.log_and_incr_err_count(&WifiError::Network("test network".into()));
        stats.log_and_incr_err_count(&WifiError::Client("test client".into()));
        stats.log_and_incr_err_count(&WifiError::Frame("test frame".into()));
        stats.log_and_incr_err_count(&WifiError::Transmission("test tx".into()));
        stats.log_and_incr_err_count(&WifiError::Other("test other".into()));
        stats.log_and_incr_err_count(&WifiError::Internal("test internal".into()));
        stats.log_and_incr_err_count(&WifiError::Chip(netsim_model::ChipError::Internal(
            "test chip".into(),
        )));

        let ipc = stats.to_proto();
        let wifi = ipc.wifi_stats.as_ref().expect("wifi_stats present");
        assert_eq!(wifi.hostapd_errors(), 1);
        assert_eq!(wifi.network_errors(), 1);
        assert_eq!(wifi.client_errors(), 1);
        assert_eq!(wifi.frame_errors(), 1);
        assert_eq!(wifi.transmission_errors(), 1);
        // Other, Internal, and Chip map to other_error
        assert_eq!(wifi.other_errors(), 3);
    }

    #[test]
    fn test_wifi_stats_log_outcome_helper() {
        let clock = Arc::new(MockClock::new());
        let mut stats = WifiStats::new(clock);

        let ok_result: Result<u32, WifiError> = Ok(42);
        stats.log_outcome(ok_result, |s, val| {
            assert_eq!(val, 42);
            s.incr_hwsim_frames_rx();
        });

        let err_result: Result<(), WifiError> = Err(WifiError::Network("network down".into()));
        stats.log_outcome(err_result, |_s, _val| {
            panic!("Should not execute success operation on error");
        });

        let ipc = stats.to_proto();
        let wifi = ipc.wifi_stats.as_ref().expect("wifi_stats present");
        assert_eq!(wifi.hwsim_frames_rx(), 1);
        assert_eq!(wifi.network_errors(), 1);
    }

    #[test]
    fn test_wifi_stats_api_increments_and_mapping() {
        let clock = Arc::new(MockClock::new());
        let mut stats = WifiStats::new(clock);

        stats.incr_wifi_api(WifiApi::WifiAwareSessionPublishUnsolicited);
        stats.incr_wifi_api(WifiApi::WifiAwareSessionPublishSolicited);
        stats.incr_wifi_api(WifiApi::WifiAwareSessionSubscribeActive);
        stats.incr_wifi_api(WifiApi::WifiAwareSessionSubscribePassive);
        stats.incr_wifi_api(WifiApi::WifiAwareDiscoverySessionSendMessage);
        stats.incr_wifi_api(WifiApi::WifiAwareNdpRequest);
        stats.incr_wifi_api(WifiApi::WifiAwareNdpResponse);
        stats.incr_wifi_api(WifiApi::WifiAwareNdpConfirm);
        stats.incr_wifi_api(WifiApi::WifiAwareNdpTerminate);

        stats.incr_wifi_api(WifiApi::WifiP2pManagerCreateGroup);
        stats.incr_wifi_api(WifiApi::WifiP2pManagerConnect);
        stats.incr_wifi_api(WifiApi::WifiP2pManagerDiscoverPeers);
        stats.incr_wifi_api(WifiApi::WifiP2pManagerDiscoverServices);

        stats.incr_wifi_api(WifiApi::WifiRttManagerStartRanging);
        stats.incr_wifi_api(WifiApi::WifiRttManagerOnRangingResults);

        stats.incr_wifi_api(WifiApi::WifiEasyConnectAuthRequest);
        stats.incr_wifi_api(WifiApi::WifiEasyConnectAuthResponse);
        stats.incr_wifi_api(WifiApi::WifiEasyConnectAuthConfirm);

        // Out of bounds check: incrementing Count should not panic
        stats.incr_wifi_api(WifiApi::Count);

        let ipc = stats.to_proto();
        let api = ipc.wifi_api_stats.as_ref().expect("wifi_api_stats present");

        let aware_session = api.wifi_aware_session.as_ref().expect("aware_session present");
        assert_eq!(aware_session.publish_unsolicited(), 1);
        assert_eq!(aware_session.publish_solicited(), 1);
        assert_eq!(aware_session.subscribe_active(), 1);
        assert_eq!(aware_session.subscribe_passive(), 1);

        let discovery =
            api.wifi_aware_discovery_session.as_ref().expect("discovery_session present");
        assert_eq!(discovery.send_message(), 1);

        let ndp = api.wifi_aware_ndp.as_ref().expect("ndp present");
        assert_eq!(ndp.request(), 1);
        assert_eq!(ndp.response(), 1);
        assert_eq!(ndp.confirm(), 1);
        assert_eq!(ndp.terminate(), 1);

        let p2p = api.wifi_p2p_manager.as_ref().expect("p2p_manager present");
        assert_eq!(p2p.create_group(), 1);
        assert_eq!(p2p.connect(), 1);
        assert_eq!(p2p.discover_peers(), 1);
        assert_eq!(p2p.discover_services(), 1);

        let rtt = api.wifi_rtt_manager.as_ref().expect("rtt_manager present");
        assert_eq!(rtt.start_ranging(), 1);
        assert_eq!(rtt.on_ranging_results(), 1);

        let dpp = api.wifi_easy_connect.as_ref().expect("easy_connect present");
        assert_eq!(dpp.auth_request(), 1);
        assert_eq!(dpp.auth_response(), 1);
        assert_eq!(dpp.auth_confirm(), 1);
    }

    #[test]
    fn test_wifi_stats_saturation_to_i32_max() {
        let clock = Arc::new(MockClock::new());
        let mut stats = WifiStats::new(clock);

        stats.counts.hostapd_error = (i32::MAX as u64) + 500;
        stats.counts.hwsim_frames_rx = (i32::MAX as u64) + 1000;
        if let Some(count) =
            stats.counts.wifi_apis.get_mut(WifiApi::WifiAwareSessionPublishUnsolicited)
        {
            *count = (i32::MAX as u64) + 2000;
        }

        let ipc = stats.to_proto();
        let wifi = ipc.wifi_stats.as_ref().expect("wifi_stats present");
        assert_eq!(wifi.hostapd_errors(), i32::MAX);
        assert_eq!(wifi.hwsim_frames_rx(), i32::MAX);

        let api = ipc.wifi_api_stats.as_ref().expect("wifi_api_stats present");
        let aware = api.wifi_aware_session.as_ref().expect("aware present");
        assert_eq!(aware.publish_unsolicited(), i32::MAX);
    }

    #[test]
    fn test_wifi_stats_throughput_recording_and_mbps_conversion() {
        let clock = Arc::new(MockClock::new());
        let mut stats = WifiStats::new(clock.clone());

        // 1,000,000 bytes per second = 8,000,000 bits per second = 8.0 Mbps
        assert_eq!(WifiStats::bytes_ps_to_mbps(1_000_000), 8.0);
        assert_eq!(WifiStats::bytes_ps_to_mbps(0), 0.0);

        // Window start at t = 1000 ms
        clock.store(1000);
        stats.record_download_bytes(500_000);
        stats.record_upload_bytes(250_000);

        // Within 5 second window (t = 4000 ms, elapsed = 3000 ms < 5000 ms)
        clock.store(4000);
        stats.record_download_bytes(500_000);
        stats.record_upload_bytes(250_000);

        // Window flush at t = 7000 ms (elapsed = 6000 ms >= 5000 ms)
        // Previous window had 1,000,000 bytes download over 6.0s = ~166,666 bytes/sec
        // Previous window had 500,000 bytes upload over 6.0s = ~83,333 bytes/sec
        clock.store(7000);
        stats.record_download_bytes(100_000);
        stats.record_upload_bytes(50_000);

        let ipc = stats.to_proto();
        let wifi = ipc.wifi_stats.as_ref().expect("wifi_stats present");
        assert!(wifi.max_download_throughput() > 0.0);
        assert!(wifi.max_upload_throughput() > 0.0);
    }
}
