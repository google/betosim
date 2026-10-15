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

use std::time::Instant;

use netsim_proto::stats::NetsimStats as ProtoNetsimStats;
use protobuf_json_mapping::{PrintOptions, print_to_string_with_options};
use tracing::warn;

const STATS_PRINT_OPTIONS: PrintOptions = PrintOptions {
    enum_values_int: false,
    proto_field_name: true,
    always_output_default_values: true,
    _future_options: (),
};

const DEFAULT_STATS_FILENAME: &str = "netsim_session_stats.json";

pub(crate) struct CombinedStatsInputs {
    pub(crate) active_stats: Vec<netsim_proto::stats::NetsimRadioStats>,
    pub(crate) wifi_stats: Option<netsim_proto::stats::WifiStats>,
    pub(crate) wifi_api: Option<netsim_proto::stats::WifiApiStats>,
    pub(crate) uwb_api: Option<netsim_proto::stats::UwbApiStats>,
    pub(crate) nfc_api: Option<netsim_proto::stats::NfcApiStats>,
    pub(crate) nfc_stats: Option<netsim_proto::stats::NfcStats>,
    pub(crate) nfc_service_stats: Option<netsim_proto::stats::NfcServiceStats>,
}

#[derive(Debug)]
pub(crate) struct Stats {
    proto: ProtoNetsimStats,
    start_time: Option<Instant>,
    // History of stats for deleted chips.
    // Note: This vector grows indefinitely during the session.
    // For typical usage (< 10,000 devices/session), memory overhead is negligible (~1MB).
    // For long-running stress tests, this may need a cap or disk-offload.
    archived_radio_stats: Vec<netsim_proto::stats::NetsimRadioStats>,
    pub(crate) stats_path: std::path::PathBuf,
    frontend_stats: std::sync::Arc<netsim_model::FrontendStats>,
}

impl Default for Stats {
    fn default() -> Self {
        Self::new(
            "0.0.0".to_string(),
            None,
            std::sync::Arc::new(netsim_model::FrontendStats::default()),
        )
    }
}

impl Stats {
    pub(crate) fn new(
        version: String,
        path: Option<std::path::PathBuf>,
        frontend_stats: std::sync::Arc<netsim_model::FrontendStats>,
    ) -> Self {
        let mut proto = ProtoNetsimStats::default();
        proto.set_version(version);
        proto.set_device_count(0);
        proto.set_peak_concurrent_devices(0);

        let stats_path = path.unwrap_or_else(|| {
            let mut p = common::system::netsimd_temp_dir();
            p.push(DEFAULT_STATS_FILENAME);
            p
        });

        Self {
            proto,
            start_time: Some(Instant::now()),
            archived_radio_stats: Vec::new(),
            stats_path,
            frontend_stats,
        }
    }

    pub(crate) fn update_device_count(&mut self, current_count: usize, created: bool) {
        if created {
            let new_total = self.proto.device_count() + 1;
            self.proto.set_device_count(new_total);
        }

        let current_count = i32::try_from(current_count).unwrap_or(i32::MAX);
        if current_count > self.proto.peak_concurrent_devices() {
            self.proto.set_peak_concurrent_devices(current_count);
        }
    }

    pub(crate) fn archive(&mut self, radio_stats: netsim_proto::stats::NetsimRadioStats) {
        self.archived_radio_stats.push(radio_stats);
    }

    pub(crate) fn add_device_stats(
        &mut self,
        device_stats: netsim_proto::stats::NetsimDeviceStats,
    ) {
        self.proto.device_stats.push(device_stats);
    }

    pub(crate) fn get_combined_stats(&mut self, inputs: CombinedStatsInputs) -> ProtoNetsimStats {
        let CombinedStatsInputs {
            mut active_stats,
            wifi_stats,
            wifi_api,
            uwb_api,
            nfc_api,
            nfc_stats,
            nfc_service_stats,
        } = inputs;
        if let Some(start) = self.start_time {
            self.proto.set_duration_secs(start.elapsed().as_secs());
        }
        let mut combined = self.proto.clone();
        combined.radio_stats.extend(self.archived_radio_stats.clone());
        combined.radio_stats.append(&mut active_stats);
        if wifi_api.is_some() || uwb_api.is_some() || nfc_api.is_some() {
            let mut api_stats = combined.api_stats.take().unwrap_or_default();
            if let Some(wa) = wifi_api {
                api_stats.wifi = netsim_proto::protobuf::MessageField::some(wa);
            }
            if let Some(ua) = uwb_api {
                api_stats.uwb = netsim_proto::protobuf::MessageField::some(ua);
            }
            if let Some(na) = nfc_api {
                let mut total_calls = 0;
                if na.nfc_adapter.is_some() || na.tag.is_some() || na.card_emulation.is_some() {
                    if let Some(adapter) = na.nfc_adapter.as_ref() {
                        total_calls += [
                            adapter.enable,
                            adapter.disable,
                            adapter.enable_reader_mode,
                            adapter.disable_reader_mode,
                            adapter.set_listen_mode_routing,
                        ]
                        .into_iter()
                        .flatten()
                        .sum::<i32>();
                    }
                    if let Some(tag) = na.tag.as_ref() {
                        total_calls += [tag.connect, tag.close, tag.transceive]
                            .into_iter()
                            .flatten()
                            .sum::<i32>();
                    }
                    if let Some(ce) = na.card_emulation.as_ref() {
                        total_calls += [ce.process_command_apdu, ce.send_response_apdu]
                            .into_iter()
                            .flatten()
                            .sum::<i32>();
                    }
                } else {
                    if let Some(core) = na.nci_core.as_ref() {
                        total_calls += [core.reset, core.init].into_iter().flatten().sum::<i32>();
                    }
                    if let Some(rf) = na.nci_rf.as_ref() {
                        total_calls += [
                            rf.discover,
                            rf.discover_select,
                            rf.deactivate,
                            rf.rf_set_listen_mode_routing,
                        ]
                        .into_iter()
                        .flatten()
                        .sum::<i32>();
                    }
                    if let Some(data) = na.nci_data.as_ref() {
                        total_calls += [data.send, data.receive].into_iter().flatten().sum::<i32>();
                    }
                }
                api_stats.nfc_total_calls = Some(total_calls);
                api_stats.nfc = netsim_proto::protobuf::MessageField::some(na);
            }
            combined.api_stats = netsim_proto::protobuf::MessageField::some(api_stats);
        }

        if let Some(ws) = wifi_stats {
            combined.wifi_stats = Some(ws).into();
        }
        if let Some(ns) = nfc_stats {
            combined.nfc_stats = Some(ns).into();
        }
        if let Some(nss) = nfc_service_stats {
            combined.nfc_service_stats = Some(nss).into();
        }

        let frontend_snap = self.frontend_stats.snapshot();
        let mut frontend_proto = netsim_proto::stats::NetsimFrontendStats::new();
        frontend_proto.set_get_version(frontend_snap.get_version);
        frontend_proto.set_create_device(frontend_snap.create_device);
        frontend_proto.set_delete_chip(frontend_snap.delete_chip);
        frontend_proto.set_patch_device(frontend_snap.patch_device);
        frontend_proto.set_reset(frontend_snap.reset);
        frontend_proto.set_list_device(frontend_snap.list_device);
        frontend_proto.set_subscribe_device(frontend_snap.subscribe_device);
        frontend_proto.set_patch_capture(frontend_snap.patch_capture);
        frontend_proto.set_list_capture(frontend_snap.list_capture);
        frontend_proto.set_get_capture(frontend_snap.get_capture);
        frontend_proto.set_delete_device(frontend_snap.delete_device);
        combined.frontend_stats = netsim_proto::protobuf::MessageField::some(frontend_proto);

        combined
    }

    fn write_to_disk(proto: &ProtoNetsimStats, path: &std::path::Path) -> std::io::Result<()> {
        let mut tmp_path = path.to_path_buf();
        if let Some(filename) = tmp_path.file_name() {
            let mut new_filename = filename.to_os_string();
            new_filename.push(".tmp");
            tmp_path.set_file_name(new_filename);
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid stats path (no filename): {:?}", path),
            ));
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if let Err(e) = Self::write_json_to_file(proto, &tmp_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }

        let mut rename_res = std::fs::rename(&tmp_path, path);
        if rename_res.is_err() {
            // On Windows, renaming to an existing file can fail if the destination file
            // handle is momentarily held or locked. Retry after removing destination.
            for _ in 0..5 {
                std::thread::sleep(std::time::Duration::from_millis(10));
                let _ = std::fs::remove_file(path);
                rename_res = std::fs::rename(&tmp_path, path);
                if rename_res.is_ok() {
                    break;
                }
            }
        }

        if let Err(error) = rename_res {
            warn!(%error, "Failed to replace stats file {:?} with {:?}", tmp_path, path);
            let _ = std::fs::remove_file(&tmp_path);
            return Err(error);
        }
        Ok(())
    }

    fn write_json_to_file(proto: &ProtoNetsimStats, path: &std::path::Path) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;

        let json = print_to_string_with_options(proto, &STATS_PRINT_OPTIONS)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        use std::io::Write;
        file.write_all(json.as_bytes())?;
        file.flush()?;
        Ok(())
    }
}

pub(crate) fn write_combined_stats(
    stats_proto: ProtoNetsimStats,
    path: std::path::PathBuf,
) -> Result<(), std::io::Error> {
    Stats::write_to_disk(&stats_proto, &path)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::Ordering};

    use netsim_model::FrontendStats;

    use super::*;

    #[test]
    fn test_frontend_stats_translation() {
        let frontend_stats = Arc::new(FrontendStats::default());

        // Simulate API calls
        frontend_stats.get_version.store(1, Ordering::SeqCst);
        frontend_stats.create_device.store(2, Ordering::SeqCst);
        frontend_stats.delete_chip.store(3, Ordering::SeqCst);
        frontend_stats.patch_device.store(4, Ordering::SeqCst);
        frontend_stats.reset.store(5, Ordering::SeqCst);
        frontend_stats.list_device.store(6, Ordering::SeqCst);
        frontend_stats.subscribe_device.store(7, Ordering::SeqCst);
        frontend_stats.patch_capture.store(8, Ordering::SeqCst);
        frontend_stats.list_capture.store(9, Ordering::SeqCst);
        frontend_stats.get_capture.store(10, Ordering::SeqCst);
        frontend_stats.delete_device.store(11, Ordering::SeqCst);

        let mut stats = Stats::new("1.0.0".to_string(), None, frontend_stats);
        let proto = stats.get_combined_stats(CombinedStatsInputs {
            active_stats: vec![],
            wifi_stats: None,
            wifi_api: None,
            uwb_api: None,
            nfc_api: None,
            nfc_stats: None,
            nfc_service_stats: None,
        });

        let frontend_proto = proto.frontend_stats.as_ref().expect("Frontend stats missing");
        assert_eq!(frontend_proto.get_version(), 1);
        assert_eq!(frontend_proto.create_device(), 2);
        assert_eq!(frontend_proto.delete_chip(), 3);
        assert_eq!(frontend_proto.patch_device(), 4);
        assert_eq!(frontend_proto.reset(), 5);
        assert_eq!(frontend_proto.list_device(), 6);
        assert_eq!(frontend_proto.subscribe_device(), 7);
        assert_eq!(frontend_proto.patch_capture(), 8);
        assert_eq!(frontend_proto.list_capture(), 9);
        assert_eq!(frontend_proto.get_capture(), 10);
        assert_eq!(frontend_proto.delete_device(), 11);
    }

    #[test]
    fn test_nfc_stats_translation() {
        let mut stats = Stats::new("1.0.0".to_string(), None, Arc::new(FrontendStats::default()));

        let mut nfc_api = netsim_proto::stats::NfcApiStats::new();
        let mut adapter = netsim_proto::stats::NfcAdapterStats::new();
        adapter.enable = Some(1);
        adapter.disable = Some(2);
        adapter.enable_reader_mode = Some(3);
        adapter.disable_reader_mode = Some(4);
        adapter.set_listen_mode_routing = Some(5);
        nfc_api.nfc_adapter = netsim_proto::protobuf::MessageField::some(adapter);

        let mut tag = netsim_proto::stats::TagStats::new();
        tag.connect = Some(6);
        tag.close = Some(7);
        tag.transceive = Some(8);
        nfc_api.tag = netsim_proto::protobuf::MessageField::some(tag);

        let mut ce = netsim_proto::stats::CardEmulationStats::new();
        ce.process_command_apdu = Some(9);
        ce.send_response_apdu = Some(10);
        nfc_api.card_emulation = netsim_proto::protobuf::MessageField::some(ce);

        let proto = stats.get_combined_stats(CombinedStatsInputs {
            active_stats: vec![],
            wifi_stats: None,
            wifi_api: None,
            uwb_api: None,
            nfc_api: Some(nfc_api),
            nfc_stats: None,
            nfc_service_stats: None,
        });

        let api_stats = proto.api_stats.as_ref().expect("Api stats missing");
        assert_eq!(api_stats.nfc_total_calls(), 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10);
    }

    #[test]
    fn test_nfc_stats_legacy_translation() {
        let mut stats = Stats::new("1.0.0".to_string(), None, Arc::new(FrontendStats::default()));

        let mut nfc_api = netsim_proto::stats::NfcApiStats::new();
        let mut core = netsim_proto::stats::NciCoreStats::new();
        core.reset = Some(1);
        core.init = Some(2);
        nfc_api.nci_core = netsim_proto::protobuf::MessageField::some(core);

        let mut rf = netsim_proto::stats::NciRfStats::new();
        rf.discover = Some(3);
        rf.discover_select = Some(4);
        rf.deactivate = Some(5);
        rf.rf_set_listen_mode_routing = Some(6);
        nfc_api.nci_rf = netsim_proto::protobuf::MessageField::some(rf);

        let mut data = netsim_proto::stats::NciDataStats::new();
        data.send = Some(7);
        data.receive = Some(8);
        nfc_api.nci_data = netsim_proto::protobuf::MessageField::some(data);

        let proto = stats.get_combined_stats(CombinedStatsInputs {
            active_stats: vec![],
            wifi_stats: None,
            wifi_api: None,
            uwb_api: None,
            nfc_api: Some(nfc_api),
            nfc_stats: None,
            nfc_service_stats: None,
        });

        let api_stats = proto.api_stats.as_ref().expect("Api stats missing");
        assert_eq!(api_stats.nfc_total_calls(), 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8);
    }

    #[test]
    fn test_nfc_stats_partial_framework() {
        let mut stats = Stats::new("1.0.0".to_string(), None, Arc::new(FrontendStats::default()));

        // Test with only tag present
        let mut nfc_api = netsim_proto::stats::NfcApiStats::new();
        let mut tag = netsim_proto::stats::TagStats::new();
        tag.connect = Some(3);
        nfc_api.tag = netsim_proto::protobuf::MessageField::some(tag);

        let proto = stats.get_combined_stats(CombinedStatsInputs {
            active_stats: vec![],
            wifi_stats: None,
            wifi_api: None,
            uwb_api: None,
            nfc_api: Some(nfc_api),
            nfc_stats: None,
            nfc_service_stats: None,
        });
        assert_eq!(proto.api_stats.as_ref().unwrap().nfc_total_calls(), 3);

        // Test with only card_emulation present
        let mut nfc_api2 = netsim_proto::stats::NfcApiStats::new();
        let mut ce = netsim_proto::stats::CardEmulationStats::new();
        ce.process_command_apdu = Some(4);
        nfc_api2.card_emulation = netsim_proto::protobuf::MessageField::some(ce);

        let proto2 = stats.get_combined_stats(CombinedStatsInputs {
            active_stats: vec![],
            wifi_stats: None,
            wifi_api: None,
            uwb_api: None,
            nfc_api: Some(nfc_api2),
            nfc_stats: None,
            nfc_service_stats: None,
        });
        assert_eq!(proto2.api_stats.as_ref().unwrap().nfc_total_calls(), 4);
    }

    #[test]
    fn test_nfc_stats_partial_legacy() {
        let mut stats = Stats::new("1.0.0".to_string(), None, Arc::new(FrontendStats::default()));

        // Test with only core present
        let mut nfc_api = netsim_proto::stats::NfcApiStats::new();
        let mut core = netsim_proto::stats::NciCoreStats::new();
        core.init = Some(5);
        nfc_api.nci_core = netsim_proto::protobuf::MessageField::some(core);

        let proto = stats.get_combined_stats(CombinedStatsInputs {
            active_stats: vec![],
            wifi_stats: None,
            wifi_api: None,
            uwb_api: None,
            nfc_api: Some(nfc_api),
            nfc_stats: None,
            nfc_service_stats: None,
        });
        assert_eq!(proto.api_stats.as_ref().unwrap().nfc_total_calls(), 5);

        // Test with empty legacy stats (all None)
        let nfc_api_empty = netsim_proto::stats::NfcApiStats::new();
        let proto_empty = stats.get_combined_stats(CombinedStatsInputs {
            active_stats: vec![],
            wifi_stats: None,
            wifi_api: None,
            uwb_api: None,
            nfc_api: Some(nfc_api_empty),
            nfc_stats: None,
            nfc_service_stats: None,
        });
        assert_eq!(proto_empty.api_stats.as_ref().unwrap().nfc_total_calls(), 0);
    }
}
