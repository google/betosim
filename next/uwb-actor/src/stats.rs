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
