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

//! Netsim Common Types
//!
//! This crate contains common, low-level data structures used across Netsim
//! crates. By separating these types into their own crate, we avoid circular
//! dependencies and reduce compilation times.
//!
//! Major types included:
//! - `ChipInfo`: Information about a chip and its device.
//! - `ChipKind`: Enumeration of supported network technologies.

use netsim_proto::{
    configuration::{
        Controller, ControllerFeatures, ControllerPreset, ControllerQuirks, VendorFeatures,
    },
    protobuf::{Enum, EnumFull, Message, MessageField},
};
use serde::{Deserialize, Serialize};

/// Default manufacturer string for virtual chips.
const DEFAULT_MANUFACTURER: &str = "Netsim";

/// Default product name for virtual chips.
const DEFAULT_PRODUCT_NAME: &str = "Virtual Chip";

/// Default fallback name for devices when device information is unavailable.
const UNKNOWN_DEVICE_NAME: &str = "Unknown";

/// Information about the chip and device provided during connection setup.
///
/// This struct is used during the handshake phase to identify the connecting
/// chip and its parent device.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChipInfo {
    /// The name of the chip (e.g., "bt-main").
    pub name: String,
    /// Detailed chip information, if available.
    pub chip: Option<Chip>,
    /// Detailed device information, if available.
    pub device_info: Option<DeviceInfo>,
}

impl ChipInfo {
    /// Creates a new ChipInfo for a specific chip kind.
    pub fn new<S: Into<String>>(name: S, kind: ChipKind) -> Self {
        let name = name.into();
        ChipInfo {
            device_info: Some(DeviceInfo::new(name.clone(), name.clone())),
            chip: Some(Chip::new(kind, name.clone())),
            name,
        }
    }

    /// Returns the device name from the device_info, or a default value.
    pub fn device_name(&self) -> String {
        self.device_info
            .as_ref()
            .map_or_else(|| UNKNOWN_DEVICE_NAME.to_string(), |d| d.name.clone())
    }

    /// Returns the kind of the chip, or `ChipKind::UNSPECIFIED` if not
    /// configured.
    pub fn chip_kind(&self) -> ChipKind {
        self.chip.as_ref().map_or(ChipKind::UNSPECIFIED, |c| c.kind)
    }
}

/// Details about a specific chip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chip {
    /// The kind of chip (Bluetooth, Wifi, etc.).
    pub kind: ChipKind,
    /// Unique identifier for the chip.
    pub id: String,
    /// Human-readable name of the chip.
    pub name: String,
    /// Manufacturer of the chip.
    pub manufacturer: String,
    /// Product name of the chip.
    pub product_name: String,
    /// Address of the chip (e.g. MAC address).
    pub address: String,
    /// SIM card type (0 = No SIM, 1 = Normal SIM, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sim_type: Option<i32>,
    /// XML SIM ICC profile content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sim_profile: Option<String>,
    /// Echo every packet back to the sender instead of attaching a chip.
    /// For transport tests and benchmarks (`startup.Chip.loopback`).
    #[serde(default)]
    pub loopback: bool,
    /// Rootcanal controller properties for Bluetooth chips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bt_properties: Option<BtControllerProperties>,
}

impl Chip {
    /// Creates a new chip with default virtual metadata.
    pub fn new<S: Into<String>>(kind: ChipKind, name: S) -> Self {
        let name = name.into();
        Chip {
            kind,
            id: name.clone(),
            name,
            manufacturer: DEFAULT_MANUFACTURER.to_string(),
            product_name: DEFAULT_PRODUCT_NAME.to_string(),
            address: String::new(),
            sim_type: None,
            sim_profile: None,
            loopback: false,
            bt_properties: None,
        }
    }
}

/// Details about the device hosting the chip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DeviceInfo {
    /// Human-readable name of the device.
    pub name: String,
    /// Unique identifier for the device.
    pub id: String,
    /// Identifier for device kind e.g. EMULATOR, CUTTLEFISH, BUMBLE, etc
    pub kind: String,
    /// Version info as applicable e.g. Android emulator version 34.1.15.0, etc
    pub version: String,
    /// SDK version info as applicable e.g. 33, 34, etc
    pub sdk_version: String,
    /// Build ID e.g. TE1A.220922.034, UQ1A.231205.015, etc
    pub build_id: String,
    /// Model/variant e.g. sdk_gphone_x86_64-userdebug, cf_x86_64_phone-user,
    /// etc
    pub variant: String,
    /// CPU architecture e.g. x86_64, arm64-v8a, etc
    pub arch: String,
    /// Path to the AVD directory, if applicable.
    pub avd_path: String,
}

impl DeviceInfo {
    /// Creates a new `DeviceInfo` with specified name and identifier.
    pub fn new<S1: Into<String>, S2: Into<String>>(name: S1, id: S2) -> Self {
        DeviceInfo { name: name.into(), id: id.into(), ..Default::default() }
    }
}

/// Bluetooth controller feature flags corresponding to Rootcanal's
/// `rootcanal.configuration.ControllerFeatures`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtControllerFeatures {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le_extended_advertising: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le_periodic_advertising: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ll_privacy: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le_2m_phy: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le_coded_phy: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le_connected_isochronous_stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le_connection_subrating: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le_channel_sounding: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le_power_control_request: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le_isochronous_broadcaster: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub le_synchronized_receiver: Option<bool>,
}

impl BtControllerFeatures {
    /// Returns `true` if all flags are unconfigured.
    fn is_empty(&self) -> bool {
        let &Self {
            le_extended_advertising,
            le_periodic_advertising,
            ll_privacy,
            le_2m_phy,
            le_coded_phy,
            le_connected_isochronous_stream,
            le_connection_subrating,
            le_channel_sounding,
            le_power_control_request,
            le_isochronous_broadcaster,
            le_synchronized_receiver,
        } = self;
        [
            le_extended_advertising,
            le_periodic_advertising,
            ll_privacy,
            le_2m_phy,
            le_coded_phy,
            le_connected_isochronous_stream,
            le_connection_subrating,
            le_channel_sounding,
            le_power_control_request,
            le_isochronous_broadcaster,
            le_synchronized_receiver,
        ]
        .iter()
        .all(Option::is_none)
    }

    fn apply(&self, target: &mut Self) {
        let &Self {
            le_extended_advertising,
            le_periodic_advertising,
            ll_privacy,
            le_2m_phy,
            le_coded_phy,
            le_connected_isochronous_stream,
            le_connection_subrating,
            le_channel_sounding,
            le_power_control_request,
            le_isochronous_broadcaster,
            le_synchronized_receiver,
        } = self;
        target.le_extended_advertising = le_extended_advertising.or(target.le_extended_advertising);
        target.le_periodic_advertising = le_periodic_advertising.or(target.le_periodic_advertising);
        target.ll_privacy = ll_privacy.or(target.ll_privacy);
        target.le_2m_phy = le_2m_phy.or(target.le_2m_phy);
        target.le_coded_phy = le_coded_phy.or(target.le_coded_phy);
        target.le_connected_isochronous_stream =
            le_connected_isochronous_stream.or(target.le_connected_isochronous_stream);
        target.le_connection_subrating = le_connection_subrating.or(target.le_connection_subrating);
        target.le_channel_sounding = le_channel_sounding.or(target.le_channel_sounding);
        target.le_power_control_request =
            le_power_control_request.or(target.le_power_control_request);
        target.le_isochronous_broadcaster =
            le_isochronous_broadcaster.or(target.le_isochronous_broadcaster);
        target.le_synchronized_receiver =
            le_synchronized_receiver.or(target.le_synchronized_receiver);
    }

    fn from_proto(p: &ControllerFeatures) -> Option<Self> {
        let out = Self {
            le_extended_advertising: p.le_extended_advertising,
            le_periodic_advertising: p.le_periodic_advertising,
            ll_privacy: p.ll_privacy,
            le_2m_phy: p.le_2m_phy,
            le_coded_phy: p.le_coded_phy,
            le_connected_isochronous_stream: p.le_connected_isochronous_stream,
            le_connection_subrating: p.le_connection_subrating,
            le_channel_sounding: p.le_channel_sounding,
            le_power_control_request: p.le_power_control_request,
            le_isochronous_broadcaster: p.le_isochronous_broadcaster,
            le_synchronized_receiver: p.le_synchronized_receiver,
        };
        (!out.is_empty()).then_some(out)
    }

    fn to_proto(self) -> Option<ControllerFeatures> {
        if self.is_empty() {
            return None;
        }
        let Self {
            le_extended_advertising,
            le_periodic_advertising,
            ll_privacy,
            le_2m_phy,
            le_coded_phy,
            le_connected_isochronous_stream,
            le_connection_subrating,
            le_channel_sounding,
            le_power_control_request,
            le_isochronous_broadcaster,
            le_synchronized_receiver,
        } = self;
        let mut out = ControllerFeatures::new();
        out.le_extended_advertising = le_extended_advertising;
        out.le_periodic_advertising = le_periodic_advertising;
        out.ll_privacy = ll_privacy;
        out.le_2m_phy = le_2m_phy;
        out.le_coded_phy = le_coded_phy;
        out.le_connected_isochronous_stream = le_connected_isochronous_stream;
        out.le_connection_subrating = le_connection_subrating;
        out.le_channel_sounding = le_channel_sounding;
        out.le_power_control_request = le_power_control_request;
        out.le_isochronous_broadcaster = le_isochronous_broadcaster;
        out.le_synchronized_receiver = le_synchronized_receiver;
        Some(out)
    }
}

/// Controller quirk flags corresponding to Rootcanal's
/// `rootcanal.configuration.ControllerQuirks`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtControllerQuirks {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_acl_data_before_connection_complete: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_default_random_address: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_error_before_reset: Option<bool>,
}

impl BtControllerQuirks {
    /// Returns `true` if all flags are unconfigured.
    fn is_empty(&self) -> bool {
        let &Self {
            send_acl_data_before_connection_complete,
            has_default_random_address,
            hardware_error_before_reset,
        } = self;
        [
            send_acl_data_before_connection_complete,
            has_default_random_address,
            hardware_error_before_reset,
        ]
        .iter()
        .all(Option::is_none)
    }

    fn apply(&self, target: &mut Self) {
        let &Self {
            send_acl_data_before_connection_complete,
            has_default_random_address,
            hardware_error_before_reset,
        } = self;
        target.send_acl_data_before_connection_complete = send_acl_data_before_connection_complete
            .or(target.send_acl_data_before_connection_complete);
        target.has_default_random_address =
            has_default_random_address.or(target.has_default_random_address);
        target.hardware_error_before_reset =
            hardware_error_before_reset.or(target.hardware_error_before_reset);
    }

    fn from_proto(p: &ControllerQuirks) -> Option<Self> {
        let out = Self {
            send_acl_data_before_connection_complete: p.send_acl_data_before_connection_complete,
            has_default_random_address: p.has_default_random_address,
            hardware_error_before_reset: p.hardware_error_before_reset,
        };
        (!out.is_empty()).then_some(out)
    }

    fn to_proto(self) -> Option<ControllerQuirks> {
        if self.is_empty() {
            return None;
        }
        let Self {
            send_acl_data_before_connection_complete,
            has_default_random_address,
            hardware_error_before_reset,
        } = self;
        let mut out = ControllerQuirks::new();
        out.send_acl_data_before_connection_complete = send_acl_data_before_connection_complete;
        out.has_default_random_address = has_default_random_address;
        out.hardware_error_before_reset = hardware_error_before_reset;
        Some(out)
    }
}

/// Vendor-specific feature flags corresponding to Rootcanal's
/// `rootcanal.configuration.VendorFeatures`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtControllerVendorFeatures {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csr: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android: Option<bool>,
}

impl BtControllerVendorFeatures {
    /// Returns `true` if all flags are unconfigured.
    fn is_empty(&self) -> bool {
        let &Self { csr, android } = self;
        [csr, android].iter().all(Option::is_none)
    }

    fn apply(&self, target: &mut Self) {
        let &Self { csr, android } = self;
        target.csr = csr.or(target.csr);
        target.android = android.or(target.android);
    }

    fn from_proto(p: &VendorFeatures) -> Option<Self> {
        let out = Self { csr: p.csr, android: p.android };
        (!out.is_empty()).then_some(out)
    }

    fn to_proto(self) -> Option<VendorFeatures> {
        if self.is_empty() {
            return None;
        }
        let Self { csr, android } = self;
        let mut out = VendorFeatures::new();
        out.csr = csr;
        out.android = android;
        Some(out)
    }
}

fn parse_controller_preset(preset: &str) -> Result<ControllerPreset, String> {
    <ControllerPreset as Enum>::from_str(&preset.to_ascii_uppercase())
        .ok_or_else(|| format!("Invalid preset: {preset}"))
}

/// Rootcanal Bluetooth controller configuration properties.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtControllerProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<BtControllerFeatures>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quirks: Option<BtControllerQuirks>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<BtControllerVendorFeatures>,
}

impl BtControllerProperties {
    /// Returns `true` if all controller properties are unconfigured or empty.
    pub fn is_empty(&self) -> bool {
        let Self { preset, features, quirks, strict, vendor } = self;
        preset.is_none()
            && features.as_ref().is_none_or(BtControllerFeatures::is_empty)
            && quirks.as_ref().is_none_or(BtControllerQuirks::is_empty)
            && strict.is_none()
            && vendor.as_ref().is_none_or(BtControllerVendorFeatures::is_empty)
    }

    /// Applies configured properties from `self` into `target`.
    pub fn apply(&self, target: &mut Self) {
        let Self { preset, features, quirks, strict, vendor } = self;
        if let Some(preset) = preset {
            target.preset = Some(preset.clone());
        }
        if let Some(features) = features {
            features.apply(target.features.get_or_insert_default());
        }
        if let Some(quirks) = quirks {
            quirks.apply(target.quirks.get_or_insert_default());
        }
        target.strict = strict.or(target.strict);
        if let Some(vendor) = vendor {
            vendor.apply(target.vendor.get_or_insert_default());
        }
    }

    /// Converts a Rootcanal `Controller` protobuf message into controller properties.
    ///
    /// Returns `None` if the protobuf message is empty or unconfigured.
    pub fn from_proto(ctrl: &Controller) -> Option<Self> {
        let preset =
            ctrl.has_preset().then(|| ctrl.preset().descriptor().name().to_ascii_lowercase());
        let features = ctrl.features.as_ref().and_then(BtControllerFeatures::from_proto);
        let quirks = ctrl.quirks.as_ref().and_then(BtControllerQuirks::from_proto);
        let vendor = ctrl.vendor.as_ref().and_then(BtControllerVendorFeatures::from_proto);

        let props = Self { preset, features, quirks, strict: ctrl.strict, vendor };
        (!props.is_empty()).then_some(props)
    }

    /// Converts controller properties into a Rootcanal `Controller` protobuf message.
    ///
    /// Returns `None` if all properties are unconfigured or empty.
    pub fn to_proto(&self) -> Option<Controller> {
        if self.is_empty() {
            return None;
        }
        let mut ctrl = Controller::new();
        if let Some(preset) = self.preset.as_deref().and_then(|p| parse_controller_preset(p).ok()) {
            ctrl.set_preset(preset);
        }
        ctrl.features =
            MessageField::from_option(self.features.and_then(BtControllerFeatures::to_proto));
        ctrl.quirks = MessageField::from_option(self.quirks.and_then(BtControllerQuirks::to_proto));
        if let Some(strict) = self.strict {
            ctrl.set_strict(strict);
        }
        ctrl.vendor =
            MessageField::from_option(self.vendor.and_then(BtControllerVendorFeatures::to_proto));
        Some(ctrl)
    }

    fn to_controller_config(&self, is_device: bool) -> Result<Controller, String> {
        let preset = match self.preset.as_deref() {
            Some(p) => parse_controller_preset(p)?,
            None => ControllerPreset::DEFAULT,
        };
        let mut cfg = self.to_proto().unwrap_or_default();
        cfg.set_preset(preset);
        // Default hardware_error_before_reset to true for emulator devices so
        // Rootcanal rejects post-init commands from uninitialized hosts, causing
        // the guest Bluetooth HAL to restart and unmask LE Meta events. Beacons
        // and scanners do not run a guest Bluetooth HAL.
        if is_device && self.quirks.as_ref().and_then(|q| q.hardware_error_before_reset).is_none() {
            cfg.quirks.mut_or_insert_default().set_hardware_error_before_reset(true);
        }
        Ok(cfg)
    }

    /// Serializes the Rootcanal controller configuration into protobuf bytes.
    pub fn serialize_config(&self, is_device: bool) -> Result<Vec<u8>, String> {
        self.to_controller_config(is_device)?
            .write_to_bytes()
            .map_err(|e| format!("Failed to serialize bt config: {e}"))
    }
}

/// The kind of network technology the chip supports.
///
/// This enumeration is used to distinguish between different types of simulated
/// radios and to route packets to the correct handlers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[allow(non_camel_case_types)]
pub enum ChipKind {
    #[default]
    UNSPECIFIED,
    BLUETOOTH,
    WIFI,
    UWB,
    NFC,
    CELLULAR,
    CELLULAR_DATA,
    ETHERNET,
}

impl ChipKind {
    /// Returns the canonical uppercase string representation.
    pub const fn as_str(&self) -> &'static str {
        match self {
            ChipKind::UNSPECIFIED => "UNSPECIFIED",
            ChipKind::BLUETOOTH => "BLUETOOTH",
            ChipKind::WIFI => "WIFI",
            ChipKind::UWB => "UWB",
            ChipKind::NFC => "NFC",
            ChipKind::CELLULAR => "CELLULAR",
            ChipKind::CELLULAR_DATA => "CELLULAR_DATA",
            ChipKind::ETHERNET => "ETHERNET",
        }
    }

    /// Returns `true` if this radio technology supports wireless peer-to-peer
    /// or broadcast medium.
    pub const fn is_wireless(&self) -> bool {
        matches!(self, ChipKind::BLUETOOTH | ChipKind::WIFI | ChipKind::UWB | ChipKind::NFC)
    }

    /// Returns `true` if this radio technology is cellular/telephony.
    pub const fn is_cellular(&self) -> bool {
        matches!(self, ChipKind::CELLULAR | ChipKind::CELLULAR_DATA)
    }
}

impl std::str::FromStr for ChipKind {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "UNSPECIFIED" => Ok(ChipKind::UNSPECIFIED),
            "BLUETOOTH" | "BT" => Ok(ChipKind::BLUETOOTH),
            "WIFI" => Ok(ChipKind::WIFI),
            "UWB" => Ok(ChipKind::UWB),
            "NFC" => Ok(ChipKind::NFC),
            "CELLULAR" | "MODEM" => Ok(ChipKind::CELLULAR),
            "CELLULAR_DATA" => Ok(ChipKind::CELLULAR_DATA),
            "ETHERNET" => Ok(ChipKind::ETHERNET),
            _ => Err(format!("invalid chip kind: {}", s)),
        }
    }
}

impl std::fmt::Display for ChipKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Display for ChipInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} on device {}", self.chip_kind(), self.device_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bt_controller_properties_is_empty() {
        let props = BtControllerProperties::default();
        assert!(props.is_empty());
    }

    #[test]
    fn test_bt_controller_properties_apply() {
        let mut base = BtControllerProperties {
            preset: Some("default".to_string()),
            features: Some(BtControllerFeatures {
                le_2m_phy: Some(true),
                le_coded_phy: Some(true),
                ..Default::default()
            }),
            quirks: Some(BtControllerQuirks {
                hardware_error_before_reset: Some(true),
                ..Default::default()
            }),
            strict: Some(false),
            vendor: None,
        };

        let override_props = BtControllerProperties {
            preset: Some("laird_bl654".to_string()),
            features: Some(BtControllerFeatures {
                le_coded_phy: Some(false),
                le_connected_isochronous_stream: Some(false),
                ..Default::default()
            }),
            quirks: Some(BtControllerQuirks {
                send_acl_data_before_connection_complete: Some(true),
                ..Default::default()
            }),
            strict: Some(true),
            vendor: Some(BtControllerVendorFeatures { android: Some(true), csr: None }),
        };

        override_props.apply(&mut base);

        assert_eq!(base.preset.as_deref(), Some("laird_bl654"));
        let feats = base.features.expect("features applied");
        assert_eq!(feats.le_2m_phy, Some(true));
        assert_eq!(feats.le_coded_phy, Some(false));
        assert_eq!(feats.le_connected_isochronous_stream, Some(false));
        let quirks = base.quirks.expect("quirks applied");
        assert_eq!(quirks.hardware_error_before_reset, Some(true));
        assert_eq!(quirks.send_acl_data_before_connection_complete, Some(true));
        assert_eq!(base.strict, Some(true));
        let vendor = base.vendor.expect("vendor applied");
        assert_eq!(vendor.android, Some(true));
    }

    #[test]
    fn test_parse_controller_preset_valid() {
        assert_eq!(parse_controller_preset("default").unwrap(), ControllerPreset::DEFAULT);
        assert_eq!(parse_controller_preset("DEFAULT").unwrap(), ControllerPreset::DEFAULT);
        assert_eq!(parse_controller_preset("laird_bl654").unwrap(), ControllerPreset::LAIRD_BL654);
        assert_eq!(parse_controller_preset("LAIRD_BL654").unwrap(), ControllerPreset::LAIRD_BL654);
        assert_eq!(
            parse_controller_preset("csr_rck_pts_dongle").unwrap(),
            ControllerPreset::CSR_RCK_PTS_DONGLE
        );
        assert_eq!(parse_controller_preset("intel_be200").unwrap(), ControllerPreset::INTEL_BE200);
    }

    #[test]
    fn test_parse_controller_preset_invalid() {
        assert!(parse_controller_preset("nonexistent").is_err());
        assert!(parse_controller_preset("").is_err());
    }

    #[test]
    fn test_build_controller_config_device_defaults_hardware_error_quirk() {
        let props = BtControllerProperties::default();
        let cfg = props.to_controller_config(true).expect("build config");
        assert_eq!(cfg.quirks.hardware_error_before_reset, Some(true));
    }

    #[test]
    fn test_build_controller_config_non_device_no_hardware_error_quirk() {
        let props = BtControllerProperties::default();
        let cfg = props.to_controller_config(false).expect("build config");
        assert_eq!(cfg.quirks.hardware_error_before_reset, None);
        assert!(cfg.has_preset());
        assert_eq!(cfg.preset(), ControllerPreset::DEFAULT);
        assert!(!props.serialize_config(false).expect("serialize").is_empty());
    }

    #[test]
    fn test_build_controller_config_device_explicit_hardware_error_preserved() {
        let props = BtControllerProperties {
            quirks: Some(BtControllerQuirks {
                hardware_error_before_reset: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = props.to_controller_config(true).expect("build config");
        assert_eq!(cfg.quirks.hardware_error_before_reset, Some(false));
    }

    #[test]
    fn test_bt_controller_properties_proto_roundtrip() {
        let props = BtControllerProperties {
            preset: Some("laird_bl654".to_string()),
            features: Some(BtControllerFeatures {
                le_extended_advertising: Some(true),
                ..Default::default()
            }),
            quirks: Some(BtControllerQuirks {
                hardware_error_before_reset: Some(true),
                ..Default::default()
            }),
            strict: Some(true),
            vendor: Some(BtControllerVendorFeatures { android: Some(true), ..Default::default() }),
        };
        let proto = props.to_proto().expect("to_proto");
        assert_eq!(proto.preset(), ControllerPreset::LAIRD_BL654);
        assert_eq!(proto.features.le_extended_advertising, Some(true));
        assert_eq!(proto.quirks.hardware_error_before_reset, Some(true));
        assert_eq!(proto.strict, Some(true));
        assert_eq!(proto.vendor.android, Some(true));

        let decoded = BtControllerProperties::from_proto(&proto).expect("from_proto");
        assert_eq!(decoded, props);
    }

    #[test]
    fn test_bt_properties_to_proto_empty_is_none() {
        let props = BtControllerProperties::default();
        assert!(props.to_proto().is_none());
        assert!(BtControllerProperties::from_proto(&Controller::new()).is_none());
    }
}
