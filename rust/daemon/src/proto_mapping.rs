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

//! Centralized conversion logic between Protobuf and internal types.

use crate::links::link::{Link as InternalLink, PhyKind as InternalPhyKind};
use netsim_proto::model::{Link as ProtoLink, PhyKind as ProtoPhyKind};
use protobuf::EnumOrUnknown;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoConversionError(String);

impl std::fmt::Display for ProtoConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Protobuf conversion error: {}", self.0)
    }
}
impl std::error::Error for ProtoConversionError {}

// --- PhyKind Conversions ---

impl TryFrom<ProtoPhyKind> for InternalPhyKind {
    type Error = ProtoConversionError;

    fn try_from(proto_kind: ProtoPhyKind) -> Result<Self, Self::Error> {
        match proto_kind {
            ProtoPhyKind::NONE => Ok(InternalPhyKind::None),
            ProtoPhyKind::BLUETOOTH_CLASSIC => Ok(InternalPhyKind::BluetoothClassic),
            ProtoPhyKind::BLUETOOTH_LOW_ENERGY => Ok(InternalPhyKind::BluetoothLowEnergy),
            ProtoPhyKind::WIFI => Ok(InternalPhyKind::Wifi),
            ProtoPhyKind::UWB => Ok(InternalPhyKind::Uwb),
            ProtoPhyKind::WIFI_RTT => Ok(InternalPhyKind::WifiRtt),
        }
    }
}

impl From<InternalPhyKind> for ProtoPhyKind {
    fn from(internal_kind: InternalPhyKind) -> Self {
        match internal_kind {
            InternalPhyKind::None => ProtoPhyKind::NONE,
            InternalPhyKind::BluetoothClassic => ProtoPhyKind::BLUETOOTH_CLASSIC,
            InternalPhyKind::BluetoothLowEnergy => ProtoPhyKind::BLUETOOTH_LOW_ENERGY,
            InternalPhyKind::Wifi => ProtoPhyKind::WIFI,
            InternalPhyKind::Uwb => ProtoPhyKind::UWB,
            InternalPhyKind::WifiRtt => ProtoPhyKind::WIFI_RTT,
        }
    }
}

// Helper for converting to EnumOrUnknown<ProtoPhyKind> which is used in ProtoLink
impl From<InternalPhyKind> for EnumOrUnknown<ProtoPhyKind> {
    fn from(internal_kind: InternalPhyKind) -> Self {
        EnumOrUnknown::new(ProtoPhyKind::from(internal_kind))
    }
}

// --- Link Conversions ---

impl From<InternalLink> for ProtoLink {
    fn from(internal_link: InternalLink) -> Self {
        ProtoLink {
            sender_id: internal_link.sender_id.0,
            receiver_id: internal_link.receiver_id.0,
            link_kind: internal_link.link_kind.into(), // Uses From<InternalPhyKind> for EnumOrUnknown
            rssi: internal_link.rssi as i32,
            ..Default::default()
        }
    }
}
