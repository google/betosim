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

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use netsim_proto::stats::{
    CardEmulationStats, NciCoreStats, NciDataStats, NciRfStats, NfcAdapterStats, NfcApiStats,
    NfcIpcStats, TagStats,
};

/// Stats for the NFC Actor.
///
/// Tracks error counts, packet counts, Casimir RF interactions, and NCI API
/// calls.
#[derive(Debug)]
pub struct NfcStats {
    // Telemetry fields (Bailey)
    nci_errors: AtomicU64,
    casimir_errors: AtomicU64,
    rf_errors: AtomicU64,
    other_errors: AtomicU64,

    nci_commands_rx: AtomicU64,
    nci_responses_tx: AtomicU64,
    nci_notifications_tx: AtomicU64,
    nci_data_rx: AtomicU64,
    nci_data_tx: AtomicU64,
    rf_taps_tx: AtomicU64,
    rf_taps_rx: AtomicU64,
    card_emulation_count: AtomicU64,
    tag_emulation_count: AtomicU64,

    // API fields (Ours)
    nfc_apis: NfcApiArray,
}

impl Default for NfcStats {
    fn default() -> Self {
        Self {
            nci_errors: AtomicU64::new(0),
            casimir_errors: AtomicU64::new(0),
            rf_errors: AtomicU64::new(0),
            other_errors: AtomicU64::new(0),
            nci_commands_rx: AtomicU64::new(0),
            nci_responses_tx: AtomicU64::new(0),
            nci_notifications_tx: AtomicU64::new(0),
            nci_data_rx: AtomicU64::new(0),
            nci_data_tx: AtomicU64::new(0),
            rf_taps_tx: AtomicU64::new(0),
            rf_taps_rx: AtomicU64::new(0),
            card_emulation_count: AtomicU64::new(0),
            tag_emulation_count: AtomicU64::new(0),
            nfc_apis: NfcApiArray::default(),
        }
    }
}

/// Stats for the NFC frontend gRPC service (`NfcService`).
///
/// Tracks invocation counts across the frontend gRPC service endpoints
/// (`get_status`, `set_power`, `poll`, `send_apdu`).
#[derive(Debug, Default)]
pub struct NfcServiceStats {
    /// Invocations of `NfcService.GetStatus`.
    pub get_status: AtomicU64,
    /// Invocations of `NfcService.SetPower`.
    pub set_power: AtomicU64,
    /// Invocations of `NfcService.Poll`.
    pub poll: AtomicU64,
    /// Invocations of `NfcService.SendApdu`.
    pub send_apdu: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NfcApi {
    AdapterEnable = 0,
    AdapterDisable = 1,
    AdapterEnableReaderMode = 2,
    AdapterDisableReaderMode = 3,
    AdapterSetListenModeRouting = 4,
    TagConnect = 5,
    TagClose = 6,
    TagTransceive = 7,
    CardEmulationProcessCommandApdu = 8,
    CardEmulationSendResponseApdu = 9,
}

const NFC_API_COUNT: usize = 10;

#[derive(Debug)]
struct NfcApiArray {
    data: [AtomicU32; NFC_API_COUNT],
}

impl Default for NfcApiArray {
    fn default() -> Self {
        Self { data: std::array::from_fn(|_| AtomicU32::new(0)) }
    }
}

fn saturate_i32(val: u64) -> i32 {
    std::cmp::min(val, i32::MAX as u64) as i32
}

fn saturate_u32(val: u64) -> u32 {
    std::cmp::min(val, u32::MAX as u64) as u32
}

impl NfcStats {
    pub fn new() -> Self {
        Self::default()
    }

    // Telemetry increments (Bailey)
    pub(crate) fn incr_nci_commands_rx(&self) {
        self.nci_commands_rx.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_nci_responses_tx(&self) {
        self.nci_responses_tx.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_nci_notifications_tx(&self) {
        self.nci_notifications_tx.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_nci_data_rx(&self) {
        self.nci_data_rx.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_nci_data_tx(&self) {
        self.nci_data_tx.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_rf_taps_tx(&self) {
        self.rf_taps_tx.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_rf_taps_rx(&self) {
        self.rf_taps_rx.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_card_emulation_count(&self) {
        self.card_emulation_count.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_tag_emulation_count(&self) {
        self.tag_emulation_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn incr_nci_error(&self) {
        self.nci_errors.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_casimir_error(&self) {
        self.casimir_errors.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_rf_error(&self) {
        self.rf_errors.fetch_add(1, Ordering::Relaxed);
    }
    #[cfg(test)]
    fn incr_other_error(&self) {
        self.other_errors.fetch_add(1, Ordering::Relaxed);
    }

    // API increments (Ours)
    pub(crate) fn incr(&self, api: NfcApi) {
        if let Some(counter) = self.nfc_apis.data.get(api as usize) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn get(&self, api: NfcApi) -> u32 {
        self.nfc_apis.data.get(api as usize).map_or(0, |counter| counter.load(Ordering::Relaxed))
    }

    // Export to telemetry proto (Bailey)
    pub fn to_proto(&self) -> netsim_proto::stats::NfcStats {
        let mut proto = netsim_proto::stats::NfcStats::new();

        proto.set_nci_errors(saturate_i32(self.nci_errors.load(Ordering::Relaxed)));
        proto.set_casimir_errors(saturate_i32(self.casimir_errors.load(Ordering::Relaxed)));
        proto.set_rf_errors(saturate_i32(self.rf_errors.load(Ordering::Relaxed)));
        proto.set_other_errors(saturate_i32(self.other_errors.load(Ordering::Relaxed)));

        proto.set_nci_commands_rx(saturate_i32(self.nci_commands_rx.load(Ordering::Relaxed)));
        proto.set_nci_responses_tx(saturate_i32(self.nci_responses_tx.load(Ordering::Relaxed)));
        proto.set_nci_notifications_tx(saturate_i32(
            self.nci_notifications_tx.load(Ordering::Relaxed),
        ));
        proto.set_nci_data_rx(saturate_i32(self.nci_data_rx.load(Ordering::Relaxed)));
        proto.set_nci_data_tx(saturate_i32(self.nci_data_tx.load(Ordering::Relaxed)));
        proto.set_rf_taps_tx(saturate_i32(self.rf_taps_tx.load(Ordering::Relaxed)));
        proto.set_rf_taps_rx(saturate_i32(self.rf_taps_rx.load(Ordering::Relaxed)));
        proto.set_card_emulation_count(saturate_i32(
            self.card_emulation_count.load(Ordering::Relaxed),
        ));
        proto.set_tag_emulation_count(saturate_i32(
            self.tag_emulation_count.load(Ordering::Relaxed),
        ));

        proto
    }

    // Export to API proto (Ours)
    fn to_api_proto(&self) -> NfcApiStats {
        let mut nfc = NfcApiStats::new();

        // Android Framework API mappings (aligned with Wi-Fi & UWB)
        let mut nfc_adapter = NfcAdapterStats::new();
        nfc_adapter.enable = Some(saturate_i32(self.get(NfcApi::AdapterEnable).into()));
        nfc_adapter.disable = Some(saturate_i32(self.get(NfcApi::AdapterDisable).into()));
        nfc_adapter.enable_reader_mode =
            Some(saturate_i32(self.get(NfcApi::AdapterEnableReaderMode).into()));
        nfc_adapter.disable_reader_mode =
            Some(saturate_i32(self.get(NfcApi::AdapterDisableReaderMode).into()));
        nfc_adapter.set_listen_mode_routing =
            Some(saturate_i32(self.get(NfcApi::AdapterSetListenModeRouting).into()));
        nfc.nfc_adapter = netsim_proto::protobuf::MessageField::some(nfc_adapter);

        let mut tag = TagStats::new();
        tag.connect = Some(saturate_i32(self.get(NfcApi::TagConnect).into()));
        tag.close = Some(saturate_i32(self.get(NfcApi::TagClose).into()));
        tag.transceive = Some(saturate_i32(self.get(NfcApi::TagTransceive).into()));
        nfc.tag = netsim_proto::protobuf::MessageField::some(tag);

        let mut card_emulation = CardEmulationStats::new();
        card_emulation.process_command_apdu =
            Some(saturate_i32(self.get(NfcApi::CardEmulationProcessCommandApdu).into()));
        card_emulation.send_response_apdu =
            Some(saturate_i32(self.get(NfcApi::CardEmulationSendResponseApdu).into()));
        nfc.card_emulation = netsim_proto::protobuf::MessageField::some(card_emulation);

        // Legacy protocol-level fields (retained and dual-populated for backwards
        // compatibility)
        let mut nci_core = NciCoreStats::new();
        nci_core.reset = Some(saturate_i32(self.get(NfcApi::AdapterDisable).into()));
        nci_core.init = Some(saturate_i32(self.get(NfcApi::AdapterEnable).into()));
        nfc.nci_core = netsim_proto::protobuf::MessageField::some(nci_core);

        let mut nci_rf = NciRfStats::new();
        nci_rf.discover = Some(saturate_i32(self.get(NfcApi::AdapterEnableReaderMode).into()));
        nci_rf.discover_select = Some(saturate_i32(self.get(NfcApi::TagConnect).into()));
        nci_rf.deactivate = Some(saturate_i32(self.get(NfcApi::AdapterDisableReaderMode).into()));
        nci_rf.rf_set_listen_mode_routing =
            Some(saturate_i32(self.get(NfcApi::AdapterSetListenModeRouting).into()));
        nfc.nci_rf = netsim_proto::protobuf::MessageField::some(nci_rf);

        let mut nci_data = NciDataStats::new();
        nci_data.send = Some(saturate_i32(self.nci_data_rx.load(Ordering::Relaxed)));
        nci_data.receive = Some(saturate_i32(self.nci_data_tx.load(Ordering::Relaxed)));
        nfc.nci_data = netsim_proto::protobuf::MessageField::some(nci_data);

        nfc
    }

    // Export to unified IPC proto (New)
    pub fn to_ipc_proto(&self) -> NfcIpcStats {
        let mut ipc = NfcIpcStats::new();
        ipc.nfc_stats = netsim_proto::protobuf::MessageField::some(self.to_proto());
        ipc.nfc_api_stats = netsim_proto::protobuf::MessageField::some(self.to_api_proto());
        ipc
    }
}

impl NfcServiceStats {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn incr_get_status_count(&self) {
        self.get_status.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_set_power_count(&self) {
        self.set_power.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_poll_count(&self) {
        self.poll.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn incr_send_apdu_count(&self) {
        self.send_apdu.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn to_proto(&self) -> netsim_proto::stats::NfcServiceStats {
        let mut proto = netsim_proto::stats::NfcServiceStats::new();
        proto.set_get_status(saturate_u32(self.get_status.load(Ordering::Relaxed)));
        proto.set_set_power(saturate_u32(self.set_power.load(Ordering::Relaxed)));
        proto.set_poll(saturate_u32(self.poll.load(Ordering::Relaxed)));
        proto.set_send_apdu(saturate_u32(self.send_apdu.load(Ordering::Relaxed)));
        proto
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nfc_stats_increments() {
        let stats = NfcStats::new();
        stats.incr_nci_commands_rx();
        stats.incr_nci_responses_tx();
        stats.incr_nci_responses_tx();
        stats.incr_nci_notifications_tx();
        stats.incr_nci_data_rx();
        stats.incr_nci_data_tx();
        stats.incr_rf_taps_tx();
        stats.incr_rf_taps_rx();
        stats.incr_card_emulation_count();
        stats.incr_tag_emulation_count();
        stats.incr_nci_error();
        stats.incr_casimir_error();
        stats.incr_rf_error();
        stats.incr_other_error();

        let proto = stats.to_proto();
        assert_eq!(proto.nci_commands_rx(), 1);
        assert_eq!(proto.nci_responses_tx(), 2);
        assert_eq!(proto.nci_notifications_tx(), 1);
        assert_eq!(proto.nci_data_rx(), 1);
        assert_eq!(proto.nci_data_tx(), 1);
        assert_eq!(proto.rf_taps_tx(), 1);
        assert_eq!(proto.rf_taps_rx(), 1);
        assert_eq!(proto.card_emulation_count(), 1);
        assert_eq!(proto.tag_emulation_count(), 1);
        assert_eq!(proto.nci_errors(), 1);
        assert_eq!(proto.casimir_errors(), 1);
        assert_eq!(proto.rf_errors(), 1);
        assert_eq!(proto.other_errors(), 1);
    }

    #[test]
    fn test_nfc_stats_saturation() {
        let stats = NfcStats::new();
        stats.nci_errors.store(i32::MAX as u64 + 100, Ordering::Relaxed);
        let proto = stats.to_proto();
        assert_eq!(proto.nci_errors(), i32::MAX);
    }

    #[test]
    fn test_nfc_service_stats_increments() {
        let service_stats = NfcServiceStats::new();
        service_stats.incr_get_status_count();
        service_stats.incr_set_power_count();
        service_stats.incr_poll_count();
        service_stats.incr_poll_count();

        let proto = service_stats.to_proto();
        assert_eq!(proto.get_status(), 1);
        assert_eq!(proto.set_power(), 1);
        assert_eq!(proto.poll(), 2);
        assert_eq!(proto.send_apdu(), 0);
    }

    #[test]
    fn test_nfc_api_stats_increments() {
        let stats = NfcStats::new();
        stats.incr(NfcApi::AdapterDisable);
        stats.incr(NfcApi::AdapterEnable);
        stats.incr(NfcApi::AdapterEnable);
        stats.incr(NfcApi::AdapterEnableReaderMode);
        stats.incr(NfcApi::AdapterSetListenModeRouting);
        stats.incr(NfcApi::TagConnect);
        stats.incr(NfcApi::TagClose);
        stats.incr(NfcApi::TagTransceive);
        stats.incr(NfcApi::CardEmulationProcessCommandApdu);
        stats.incr(NfcApi::CardEmulationSendResponseApdu);
        stats.incr_nci_data_rx();
        stats.incr_nci_data_tx();
        stats.incr_nci_data_tx();

        let proto = stats.to_api_proto();

        // Android Framework API checks
        let nfc_adapter = proto.nfc_adapter.as_ref().expect("nfc_adapter missing");
        assert_eq!(nfc_adapter.disable, Some(1));
        assert_eq!(nfc_adapter.enable, Some(2));
        assert_eq!(nfc_adapter.enable_reader_mode, Some(1));
        assert_eq!(nfc_adapter.disable_reader_mode, Some(0));
        assert_eq!(nfc_adapter.set_listen_mode_routing, Some(1));

        let tag = proto.tag.as_ref().expect("tag missing");
        assert_eq!(tag.connect, Some(1));
        assert_eq!(tag.close, Some(1));
        assert_eq!(tag.transceive, Some(1));

        let card_emulation = proto.card_emulation.as_ref().expect("card_emulation missing");
        assert_eq!(card_emulation.process_command_apdu, Some(1));
        assert_eq!(card_emulation.send_response_apdu, Some(1));

        // Legacy fields backwards compatibility checks
        let nci_core = proto.nci_core.as_ref().expect("nci_core missing");
        assert_eq!(nci_core.reset, Some(1));
        assert_eq!(nci_core.init, Some(2));

        let nci_rf = proto.nci_rf.as_ref().expect("nci_rf missing");
        assert_eq!(nci_rf.discover, Some(1));
        assert_eq!(nci_rf.discover_select, Some(1));
        assert_eq!(nci_rf.deactivate, Some(0));
        assert_eq!(nci_rf.rf_set_listen_mode_routing, Some(1));

        let nci_data = proto.nci_data.as_ref().expect("nci_data missing");
        assert_eq!(nci_data.send, Some(1));
        assert_eq!(nci_data.receive, Some(2));
    }

    #[test]
    fn test_to_ipc_proto() {
        let stats = NfcStats::new();
        stats.incr(NfcApi::AdapterEnable);
        stats.incr_card_emulation_count();
        stats.incr_tag_emulation_count();
        stats.incr_rf_taps_tx();
        stats.incr_rf_taps_rx();
        let ipc = stats.to_ipc_proto();
        assert!(ipc.nfc_stats.is_some());
        assert!(ipc.nfc_api_stats.is_some());
        let nfc_stats = ipc.nfc_stats.as_ref().unwrap();
        assert_eq!(nfc_stats.card_emulation_count(), 1);
        assert_eq!(nfc_stats.tag_emulation_count(), 1);
        assert_eq!(nfc_stats.rf_taps_tx(), 1);
        assert_eq!(nfc_stats.rf_taps_rx(), 1);
        let api_stats = ipc.nfc_api_stats.as_ref().unwrap();
        assert_eq!(api_stats.nfc_adapter.as_ref().unwrap().enable, Some(1));
    }

    #[test]
    fn test_nfc_api_get_out_of_bounds() {
        let stats = NfcStats::new();
        assert_eq!(stats.nfc_apis.data.len(), NFC_API_COUNT);
        assert!(stats.nfc_apis.data.get(NFC_API_COUNT).is_none());
    }

    #[test]
    fn test_nfc_api_stats_saturation() {
        let stats = NfcStats::new();
        stats.nfc_apis.data[NfcApi::AdapterEnable as usize].store(u32::MAX, Ordering::Relaxed);
        let proto = stats.to_api_proto();
        assert_eq!(proto.nfc_adapter.as_ref().unwrap().enable, Some(i32::MAX));
    }

    #[test]
    fn test_nfc_proto_accessors_and_serialization() {
        use netsim_proto::protobuf::Message;

        // 1. NfcAdapterStats getters, setters, has, clear
        let mut adapter = NfcAdapterStats::new();
        assert_eq!(adapter.enable(), 0);
        assert!(!adapter.has_enable());
        adapter.set_enable(10);
        assert_eq!(adapter.enable(), 10);
        assert!(adapter.has_enable());
        adapter.clear_enable();
        assert_eq!(adapter.enable(), 0);
        assert!(!adapter.has_enable());

        assert_eq!(adapter.disable(), 0);
        assert!(!adapter.has_disable());
        adapter.set_disable(20);
        assert_eq!(adapter.disable(), 20);
        assert!(adapter.has_disable());
        adapter.clear_disable();
        assert!(!adapter.has_disable());

        assert_eq!(adapter.enable_reader_mode(), 0);
        assert!(!adapter.has_enable_reader_mode());
        adapter.set_enable_reader_mode(30);
        assert_eq!(adapter.enable_reader_mode(), 30);
        assert!(adapter.has_enable_reader_mode());
        adapter.clear_enable_reader_mode();
        assert!(!adapter.has_enable_reader_mode());

        assert_eq!(adapter.disable_reader_mode(), 0);
        assert!(!adapter.has_disable_reader_mode());
        adapter.set_disable_reader_mode(40);
        assert_eq!(adapter.disable_reader_mode(), 40);
        assert!(adapter.has_disable_reader_mode());
        adapter.clear_disable_reader_mode();
        assert!(!adapter.has_disable_reader_mode());

        assert_eq!(adapter.set_listen_mode_routing(), 0);
        assert!(!adapter.has_set_listen_mode_routing());
        adapter.set_set_listen_mode_routing(50);
        assert_eq!(adapter.set_listen_mode_routing(), 50);
        assert!(adapter.has_set_listen_mode_routing());
        adapter.clear_set_listen_mode_routing();
        assert!(!adapter.has_set_listen_mode_routing());

        let default_adapter = NfcAdapterStats::default();
        assert_eq!(adapter, default_adapter);

        // 2. TagStats getters, setters, has, clear
        let mut tag = TagStats::new();
        assert_eq!(tag.connect(), 0);
        assert!(!tag.has_connect());
        tag.set_connect(100);
        assert_eq!(tag.connect(), 100);
        assert!(tag.has_connect());
        tag.clear_connect();
        assert!(!tag.has_connect());

        assert_eq!(tag.close(), 0);
        assert!(!tag.has_close());
        tag.set_close(200);
        assert_eq!(tag.close(), 200);
        assert!(tag.has_close());
        tag.clear_close();
        assert!(!tag.has_close());

        assert_eq!(tag.transceive(), 0);
        assert!(!tag.has_transceive());
        tag.set_transceive(300);
        assert_eq!(tag.transceive(), 300);
        assert!(tag.has_transceive());
        tag.clear_transceive();
        assert!(!tag.has_transceive());

        // 3. CardEmulationStats getters, setters, has, clear
        let mut ce = CardEmulationStats::new();
        assert_eq!(ce.process_command_apdu(), 0);
        assert!(!ce.has_process_command_apdu());
        ce.set_process_command_apdu(400);
        assert_eq!(ce.process_command_apdu(), 400);
        assert!(ce.has_process_command_apdu());
        ce.clear_process_command_apdu();
        assert!(!ce.has_process_command_apdu());

        assert_eq!(ce.send_response_apdu(), 0);
        assert!(!ce.has_send_response_apdu());
        ce.set_send_response_apdu(500);
        assert_eq!(ce.send_response_apdu(), 500);
        assert!(ce.has_send_response_apdu());
        ce.clear_send_response_apdu();
        assert!(!ce.has_send_response_apdu());

        // 4. NfcApiStats has, clear, take, and protobuf serialization/deserialization
        let mut nfc_api = NfcApiStats::new();
        nfc_api.nfc_adapter = netsim_proto::protobuf::MessageField::some(adapter.clone());
        assert!(nfc_api.nfc_adapter.is_some());
        let _ = nfc_api.nfc_adapter.as_ref();
        let _ = nfc_api.nfc_adapter.as_mut();
        let _ = nfc_api.nfc_adapter.take();
        assert!(nfc_api.nfc_adapter.is_none());

        nfc_api.tag = netsim_proto::protobuf::MessageField::some(tag.clone());
        assert!(nfc_api.tag.is_some());
        let _ = nfc_api.tag.as_ref();
        let _ = nfc_api.tag.as_mut();
        let _ = nfc_api.tag.take();
        assert!(nfc_api.tag.is_none());

        nfc_api.card_emulation = netsim_proto::protobuf::MessageField::some(ce.clone());
        assert!(nfc_api.card_emulation.is_some());
        let _ = nfc_api.card_emulation.as_ref();
        let _ = nfc_api.card_emulation.as_mut();
        let _ = nfc_api.card_emulation.take();
        assert!(nfc_api.card_emulation.is_none());

        // Serialization roundtrip
        let stats = NfcStats::new();
        stats.incr(NfcApi::AdapterEnable);
        stats.incr(NfcApi::TagConnect);
        stats.incr(NfcApi::CardEmulationProcessCommandApdu);
        let api_proto = stats.to_api_proto();
        let bytes = api_proto.write_to_bytes().unwrap();
        let parsed = NfcApiStats::parse_from_bytes(&bytes).unwrap();
        assert_eq!(api_proto, parsed);
    }
}
