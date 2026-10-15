// Copyright 2023 The Android Open Source Project
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

use crate::devices::chip::ChipIdentifier;
use crate::wireless::{WirelessChip, WirelessChipImpl};

use bytes::Bytes;
use netsim_proto::common::ChipKind as ProtoChipKind;
use netsim_proto::model::Chip as ProtoChip;
use netsim_proto::stats::{NetsimRadioStats as ProtoRadioStats, netsim_radio_stats};
use protobuf::EnumOrUnknown;

/// Parameters for creating Mocked chips
pub struct CreateParams {
    pub chip_kind: ProtoChipKind,
}

/// Mock struct is remained empty.
pub struct Mock {
    chip_kind: ProtoChipKind,
}

impl WirelessChip for Mock {
    fn handle_request(&self, _packet: &Bytes) {}

    fn reset(&self) {}

    fn get(&self) -> ProtoChip {
        let mut proto_chip = ProtoChip::new();
        proto_chip.kind = EnumOrUnknown::new(self.chip_kind);
        proto_chip
    }

    fn patch(&self, _chip: &ProtoChip) {}

    fn get_stats(&self, _duration_secs: u64) -> Vec<ProtoRadioStats> {
        let mut stats = ProtoRadioStats::new();
        stats.kind = Some(EnumOrUnknown::new(match self.chip_kind {
            ProtoChipKind::BLUETOOTH => netsim_radio_stats::Kind::BLUETOOTH_LOW_ENERGY,
            ProtoChipKind::WIFI => netsim_radio_stats::Kind::WIFI,
            ProtoChipKind::UWB => netsim_radio_stats::Kind::UWB,
            ProtoChipKind::BLUETOOTH_BEACON => netsim_radio_stats::Kind::BLE_BEACON,
            ProtoChipKind::NFC => netsim_radio_stats::Kind::NFC,
            ProtoChipKind::CELLULAR => netsim_radio_stats::Kind::UNSPECIFIED,
            _ => netsim_radio_stats::Kind::UNSPECIFIED,
        }));
        vec![stats]
    }
}

/// Create a new MockedChip
pub fn add_chip(create_params: &CreateParams, _chip_id: ChipIdentifier) -> WirelessChipImpl {
    Box::new(Mock { chip_kind: create_params.chip_kind })
}
