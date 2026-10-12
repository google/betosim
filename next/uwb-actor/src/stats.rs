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

// UWB API stats tracking rebase.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use netsim_proto::stats::{RangingSessionStats, UwbApiStats, UwbManagerStats};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UwbApi {
    Open,
    Close,
    Start,
    Stop,
    Reconfigure,
    SessionGetAppConfig,
    SendData,
    OnDataReceived,
    AddControlee,
    RemoveControlee,
    Count,
}

pub(crate) const UWB_API_COUNT: usize = UwbApi::Count as usize;

#[derive(Debug)]
struct UwbApiArray {
    data: [AtomicU32; UWB_API_COUNT],
}

impl Default for UwbApiArray {
    fn default() -> Self {
        Self { data: std::array::from_fn(|_| AtomicU32::new(0)) }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct UwbStats {
    uwb_apis: Arc<UwbApiArray>,
}

impl UwbStats {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn incr(&self, api: UwbApi) {
        if let Some(counter) = self.uwb_apis.data.get(api as usize) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn get(&self, api: UwbApi) -> u32 {
        self.uwb_apis.data.get(api as usize).map_or(0, |counter| counter.load(Ordering::Relaxed))
    }

    pub(crate) fn to_proto(&self) -> netsim_proto::stats::UwbApiStats {
        let saturate = |val: u32| -> i32 { val.try_into().unwrap_or(i32::MAX) };

        let mut uwb = UwbApiStats::new();

        let mut uwb_manager = UwbManagerStats::new();
        uwb_manager.open_ranging_session = Some(saturate(self.get(UwbApi::Open)));
        uwb.uwb_manager = netsim_proto::protobuf::MessageField::some(uwb_manager);

        let mut ranging_session = RangingSessionStats::new();
        ranging_session.close = Some(saturate(self.get(UwbApi::Close)));
        ranging_session.start = Some(saturate(self.get(UwbApi::Start)));
        ranging_session.stop = Some(saturate(self.get(UwbApi::Stop)));
        ranging_session.reconfigure = Some(saturate(self.get(UwbApi::Reconfigure)));
        ranging_session.session_get_app_config =
            Some(saturate(self.get(UwbApi::SessionGetAppConfig)));
        ranging_session.send_data = Some(saturate(self.get(UwbApi::SendData)));
        ranging_session.on_data_received = Some(saturate(self.get(UwbApi::OnDataReceived)));
        ranging_session.add_controlee = Some(saturate(self.get(UwbApi::AddControlee)));
        ranging_session.remove_controlee = Some(saturate(self.get(UwbApi::RemoveControlee)));
        uwb.ranging_session = netsim_proto::protobuf::MessageField::some(ranging_session);

        uwb
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;

    #[test]
    fn test_uwb_stats_initial_state() {
        let stats = UwbStats::new();
        for api in [
            UwbApi::Open,
            UwbApi::Close,
            UwbApi::Start,
            UwbApi::Stop,
            UwbApi::Reconfigure,
            UwbApi::SessionGetAppConfig,
            UwbApi::SendData,
            UwbApi::OnDataReceived,
            UwbApi::AddControlee,
            UwbApi::RemoveControlee,
            UwbApi::Count,
        ] {
            assert_eq!(stats.get(api), 0);
        }

        let proto = stats.to_proto();
        let mgr = proto.uwb_manager.as_ref().expect("uwb_manager exists");
        assert_eq!(mgr.open_ranging_session(), 0);

        let session = proto.ranging_session.as_ref().expect("ranging_session exists");
        assert_eq!(session.close(), 0);
        assert_eq!(session.start(), 0);
        assert_eq!(session.stop(), 0);
        assert_eq!(session.reconfigure(), 0);
        assert_eq!(session.session_get_app_config(), 0);
        assert_eq!(session.send_data(), 0);
        assert_eq!(session.on_data_received(), 0);
        assert_eq!(session.add_controlee(), 0);
        assert_eq!(session.remove_controlee(), 0);
    }

    #[test]
    fn test_uwb_stats_increment_all_apis() {
        let stats = UwbStats::new();

        stats.incr(UwbApi::Open);
        stats.incr(UwbApi::Open);
        stats.incr(UwbApi::Close);
        stats.incr(UwbApi::Start);
        stats.incr(UwbApi::Stop);
        stats.incr(UwbApi::Reconfigure);
        stats.incr(UwbApi::SessionGetAppConfig);
        stats.incr(UwbApi::SendData);
        stats.incr(UwbApi::OnDataReceived);
        stats.incr(UwbApi::AddControlee);
        stats.incr(UwbApi::RemoveControlee);

        assert_eq!(stats.get(UwbApi::Open), 2);
        assert_eq!(stats.get(UwbApi::Close), 1);
        assert_eq!(stats.get(UwbApi::Start), 1);
        assert_eq!(stats.get(UwbApi::Stop), 1);
        assert_eq!(stats.get(UwbApi::Reconfigure), 1);
        assert_eq!(stats.get(UwbApi::SessionGetAppConfig), 1);
        assert_eq!(stats.get(UwbApi::SendData), 1);
        assert_eq!(stats.get(UwbApi::OnDataReceived), 1);
        assert_eq!(stats.get(UwbApi::AddControlee), 1);
        assert_eq!(stats.get(UwbApi::RemoveControlee), 1);
        assert_eq!(stats.get(UwbApi::Count), 0);

        let proto = stats.to_proto();
        let mgr = proto.uwb_manager.as_ref().expect("uwb_manager exists");
        assert_eq!(mgr.open_ranging_session(), 2);

        let session = proto.ranging_session.as_ref().expect("ranging_session exists");
        assert_eq!(session.close(), 1);
        assert_eq!(session.start(), 1);
        assert_eq!(session.stop(), 1);
        assert_eq!(session.reconfigure(), 1);
        assert_eq!(session.session_get_app_config(), 1);
        assert_eq!(session.send_data(), 1);
        assert_eq!(session.on_data_received(), 1);
        assert_eq!(session.add_controlee(), 1);
        assert_eq!(session.remove_controlee(), 1);
    }

    #[test]
    fn test_uwb_stats_clone_shares_counters() {
        let stats1 = UwbStats::new();
        let stats2 = stats1.clone();

        stats1.incr(UwbApi::Start);
        assert_eq!(stats2.get(UwbApi::Start), 1);

        stats2.incr(UwbApi::Start);
        assert_eq!(stats1.get(UwbApi::Start), 2);
    }

    #[test]
    fn test_uwb_stats_saturation_to_i32_max() {
        let stats = UwbStats::new();
        // Manually write u32::MAX into the underlying atomic to test saturation
        let open_idx = UwbApi::Open as usize;
        stats.uwb_apis.data[open_idx].store(u32::MAX, Ordering::Relaxed);

        assert_eq!(stats.get(UwbApi::Open), u32::MAX);
        let proto = stats.to_proto();
        let mgr = proto.uwb_manager.as_ref().expect("uwb_manager exists");
        assert_eq!(mgr.open_ranging_session(), i32::MAX);
    }
}
