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

//! Defines structures for parsing `nl80211` attributes.
#![allow(clippy::empty_line_after_doc_comments)]

use std::fmt;

use zerocopy::{FromBytes, IntoBytes};

use crate::{
    ethernet::MacAddr,
    netlink::{
        mac80211_hwsim::HwsimAttrEnum,
        nl80211::attr_id,
        nl80211_util::{self, NetlinkError},
    },
};

/// A read-only, zero-copy view of a set of `nl80211` attributes.
///
/// This struct provides a way to access the raw bytes of a set of `nl80211`
/// attributes without parsing them immediately. This is useful for efficiently
/// passing around the attribute data. The attributes can be parsed into an
/// `Nl80211AttrSet` using the `to_owned` method.
#[derive(FromBytes, IntoBytes)]
#[repr(C)]
pub struct Nl80211AttrPacket<'a> {
    attributes: &'a [u8],
}

/// A set of parsed `nl80211` attributes.
///
/// This struct holds the parsed values of the `nl80211` attributes.
/// It is created by parsing an `Nl80211AttrPacket`.
#[derive(Debug, Default)]
pub struct Nl80211AttrSet {
    pub hw_index: Option<u32>,
    pub iface_mac: Option<MacAddr>,
    pub iface_name: Option<String>,
    pub iface_type: Option<u32>,
    pub req_iface_num: Option<u32>,
    pub max_ifaces: Option<u32>,
    pub reg_dom: Option<u32>,
    pub reg_alpha2: Option<String>,
    pub channel: Option<u32>,
    pub frequency: Option<u32>,
    pub channel_type: Option<u32>,
    pub channel_flags: Option<u32>,
    pub max_tx_power: Option<u32>,
    pub center_freq1: Option<u32>,
    pub center_freq2: Option<u32>,
    pub signal: Option<u32>,
    pub noise: Option<u32>,
    pub rx_rate: Option<u32>,
    pub key: Option<Vec<u8>>,
    pub key_idx: Option<u32>,
    pub key_data: Option<Vec<u8>>,
    pub key_seq: Option<Vec<u8>>,
    pub key_flags: Option<u32>,
    pub cipher: Option<u32>,
    pub beacon_interval: Option<u32>,
    pub dtim_period: Option<u32>,
    pub hidden_ssid: Option<u32>,
    pub supported_rates: Option<Vec<u8>>,
    pub short_preamble: Option<u32>,
    pub short_slot_time: Option<u32>,
    pub edca_params: Option<Vec<u8>>,
    pub wmm_enabled: Option<u32>,
    pub power_constraint: Option<u32>,
    pub local_power_constraint: Option<u32>,
    pub txpower: Option<u32>,
    pub supported_channels: Option<Vec<u8>>,
    pub mesh_path: Option<Vec<u8>>,
    pub mesh_id: Option<Vec<u8>>,
    pub mesh_plink_state: Option<u32>,
    pub mesh_gate_announcement: Option<Vec<u8>>,
    pub hwsim_attr_frame_data: Option<Vec<u8>>,
    pub hwsim_attr_cookie: Option<u64>,
    pub hwsim_attr_flags: Option<u32>,
    /// The raw bytes of all attributes in the set.
    pub attributes: Vec<u8>,
}

impl fmt::Display for Nl80211AttrSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{ ")?;
        if let Some(v) = self.hw_index {
            write!(f, "hw_index: {v}, ")?;
        }
        if let Some(ref v) = self.iface_mac {
            write!(f, "iface_mac: {v}, ")?;
        }
        if let Some(ref v) = self.iface_name {
            write!(f, "iface_name: {v}, ")?;
        }
        if let Some(v) = self.iface_type {
            write!(f, "iface_type: {v}, ")?;
        }
        if let Some(v) = self.req_iface_num {
            write!(f, "req_iface_num: {v}, ")?;
        }
        if let Some(v) = self.max_ifaces {
            write!(f, "max_ifaces: {v}, ")?;
        }
        if let Some(v) = self.reg_dom {
            write!(f, "reg_dom: {v}, ")?;
        }
        if let Some(ref v) = self.reg_alpha2 {
            write!(f, "reg_alpha2: {v}, ")?;
        }
        if let Some(v) = self.channel {
            write!(f, "channel: {v}, ")?;
        }
        if let Some(v) = self.frequency {
            write!(f, "frequency: {v}, ")?;
        }
        if let Some(v) = self.channel_type {
            write!(f, "channel_type: {v}, ")?;
        }
        if let Some(v) = self.channel_flags {
            write!(f, "channel_flags: {v}, ")?;
        }
        if let Some(v) = self.max_tx_power {
            write!(f, "max_tx_power: {v}, ")?;
        }
        if let Some(v) = self.center_freq1 {
            write!(f, "center_freq1: {v}, ")?;
        }
        if let Some(v) = self.center_freq2 {
            write!(f, "center_freq2: {v}, ")?;
        }
        if let Some(v) = self.signal {
            write!(f, "signal: {v}, ")?;
        }
        if let Some(v) = self.noise {
            write!(f, "noise: {v}, ")?;
        }
        if let Some(v) = self.rx_rate {
            write!(f, "rx_rate: {v}, ")?;
        }
        if let Some(ref v) = self.key {
            write!(f, "key: {v:?}, ")?;
        }
        if let Some(v) = self.key_idx {
            write!(f, "key_idx: {v}, ")?;
        }
        if let Some(ref v) = self.key_data {
            write!(f, "key_data: {v:?}, ")?;
        }
        if let Some(ref v) = self.key_seq {
            write!(f, "key_seq: {v:?}, ")?;
        }
        if let Some(v) = self.key_flags {
            write!(f, "key_flags: {v}, ")?;
        }
        if let Some(v) = self.cipher {
            write!(f, "cipher: {v}, ")?;
        }
        if let Some(v) = self.beacon_interval {
            write!(f, "beacon_interval: {v}, ")?;
        }
        if let Some(v) = self.dtim_period {
            write!(f, "dtim_period: {v}, ")?;
        }
        if let Some(v) = self.hidden_ssid {
            write!(f, "hidden_ssid: {v}, ")?;
        }
        if let Some(ref v) = self.supported_rates {
            write!(f, "supported_rates: {v:?}, ")?;
        }
        if let Some(v) = self.short_preamble {
            write!(f, "short_preamble: {v}, ")?;
        }
        if let Some(v) = self.short_slot_time {
            write!(f, "short_slot_time: {v}, ")?;
        }
        if let Some(ref v) = self.edca_params {
            write!(f, "edca_params: {v:?}, ")?;
        }
        if let Some(v) = self.wmm_enabled {
            write!(f, "wmm_enabled: {v}, ")?;
        }
        if let Some(v) = self.power_constraint {
            write!(f, "power_constraint: {v}, ")?;
        }
        if let Some(v) = self.local_power_constraint {
            write!(f, "local_power_constraint: {v}, ")?;
        }
        if let Some(v) = self.txpower {
            write!(f, "txpower: {v}, ")?;
        }
        if let Some(ref v) = self.supported_channels {
            write!(f, "supported_channels: {v:?}, ")?;
        }
        if let Some(ref v) = self.mesh_path {
            write!(f, "mesh_path: {v:?}, ")?;
        }
        if let Some(ref v) = self.mesh_id {
            write!(f, "mesh_id: {v:?}, ")?;
        }
        if let Some(v) = self.mesh_plink_state {
            write!(f, "mesh_plink_state: {v}, ")?;
        }
        if let Some(ref v) = self.mesh_gate_announcement {
            write!(f, "mesh_gate_announcement: {v:?}, ")?;
        }
        if let Some(ref v) = self.hwsim_attr_frame_data {
            write!(f, "hwsim_attr_frame_data: {v:?}, ")?;
        }
        if let Some(v) = self.hwsim_attr_cookie {
            write!(f, "hwsim_attr_cookie: {v}, ")?;
        }
        if let Some(v) = self.hwsim_attr_flags {
            write!(f, "hwsim_attr_flags: {v}, ")?;
        }
        f.write_str("}}")
    }
}

/// Builder for `Nl80211AttrSet`.
#[derive(Default)]
pub struct Nl80211AttrSetBuilder {
    set: Nl80211AttrSet,
}

impl Nl80211AttrSetBuilder {
    /// Creates a new `Nl80211AttrSetBuilder`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the `Nl80211AttrSet`.
    pub fn build(self) -> Nl80211AttrSet {
        self.set
    }

    /// Adds a `u32` attribute to the set.
    pub fn u32_attr(&mut self, attr_id: u16, value: u32) -> &mut Self {
        let payload = nl80211_util::create_u32_attr_payload(value);
        self.set.attributes.extend_from_slice(&payload);
        match attr_id {
            attr_id::HW_INDEX => self.set.hw_index = Some(value),
            attr_id::IFACE_TYPE => self.set.iface_type = Some(value),
            attr_id::REQ_IFACE_NUM => self.set.req_iface_num = Some(value),
            attr_id::MAX_IFACES => self.set.max_ifaces = Some(value),
            attr_id::REG_DOM => self.set.reg_dom = Some(value),
            attr_id::CHANNEL => self.set.channel = Some(value),
            attr_id::FREQUENCY => self.set.frequency = Some(value),
            attr_id::CHANNEL_TYPE => self.set.channel_type = Some(value),
            attr_id::CHANNEL_FLAGS => self.set.channel_flags = Some(value),
            attr_id::MAX_TX_POWER => self.set.max_tx_power = Some(value),
            attr_id::CENTER_FREQ1 => self.set.center_freq1 = Some(value),
            attr_id::CENTER_FREQ2 => self.set.center_freq2 = Some(value),
            attr_id::SIGNAL => self.set.signal = Some(value),
            attr_id::NOISE => self.set.noise = Some(value),
            attr_id::RX_RATE => self.set.rx_rate = Some(value),
            attr_id::KEY_IDX => self.set.key_idx = Some(value),
            attr_id::KEY_FLAGS => self.set.key_flags = Some(value),
            attr_id::CIPHER => self.set.cipher = Some(value),
            attr_id::BEACON_INTERVAL => self.set.beacon_interval = Some(value),
            attr_id::DTIM_PERIOD => self.set.dtim_period = Some(value),
            attr_id::HIDDEN_SSID => self.set.hidden_ssid = Some(value),
            attr_id::SHORT_PREAMBLE => self.set.short_preamble = Some(value),
            attr_id::SHORT_SLOT_TIME => self.set.short_slot_time = Some(value),
            attr_id::WMM_ENABLED => self.set.wmm_enabled = Some(value),
            attr_id::POWER_CONSTRAINT => self.set.power_constraint = Some(value),
            attr_id::LOCAL_POWER_CONSTRAINT => self.set.local_power_constraint = Some(value),
            attr_id::TXPOWER => self.set.txpower = Some(value),
            attr_id::MESH_PLINK_STATE => self.set.mesh_plink_state = Some(value),
            attr_id::HWSIM_ATTR_FLAGS => self.set.hwsim_attr_flags = Some(value),
            _ => (),
        }
        self
    }

    /// Adds a `u64` attribute to the set.
    pub fn u64_attr(&mut self, attr_id: u16, value: u64) -> &mut Self {
        let payload = value.to_le_bytes();
        self.set.attributes.extend_from_slice(&payload);
        if attr_id == attr_id::HWSIM_ATTR_COOKIE {
            self.set.hwsim_attr_cookie = Some(value);
        }
        self
    }

    /// Adds a `string` attribute to the set.
    pub fn string_attr(&mut self, attr_id: u16, value: &str) -> &mut Self {
        let payload = nl80211_util::create_string_attr_payload(value);
        self.set.attributes.extend_from_slice(&payload);
        match attr_id {
            attr_id::IFACE_NAME => self.set.iface_name = Some(value.to_string()),
            attr_id::REG_ALPHA2 => self.set.reg_alpha2 = Some(value.to_string()),
            _ => (),
        }
        self
    }

    /// Adds a `MAC address` attribute to the set.
    pub fn mac_addr_attr(&mut self, attr_id: u16, value: &MacAddr) -> &mut Self {
        let payload = nl80211_util::create_mac_addr_attr_payload(value);
        self.set.attributes.extend_from_slice(&payload);
        if attr_id == attr_id::IFACE_MAC {
            self.set.iface_mac = Some(*value);
        }
        self
    }

    /// Adds a `bytes` attribute to the set.
    pub fn bytes_attr(&mut self, attr_id: u16, value: &[u8]) -> &mut Self {
        let payload = nl80211_util::create_bytes_attr_payload(value);
        self.set.attributes.extend_from_slice(&payload);
        match attr_id {
            attr_id::KEY => self.set.key = Some(value.to_vec()),
            attr_id::KEY_DATA => self.set.key_data = Some(value.to_vec()),
            attr_id::KEY_SEQ => self.set.key_seq = Some(value.to_vec()),
            attr_id::SUPPORTED_RATES => self.set.supported_rates = Some(value.to_vec()),
            attr_id::EDCA_PARAMS => self.set.edca_params = Some(value.to_vec()),
            attr_id::SUPPORTED_CHANNELS => self.set.supported_channels = Some(value.to_vec()),
            attr_id::MESH_PATH => self.set.mesh_path = Some(value.to_vec()),
            attr_id::MESH_ID => self.set.mesh_id = Some(value.to_vec()),
            attr_id::MESH_GATE_ANNOUNCEMENT => {
                self.set.mesh_gate_announcement = Some(value.to_vec())
            }
            attr_id::HWSIM_ATTR_FRAME_DATA => self.set.hwsim_attr_frame_data = Some(value.to_vec()),
            _ => (),
        }
        self
    }
}

impl Nl80211AttrSet {
    /// Creates a new `Nl80211AttrSetBuilder`.
    pub fn builder() -> Nl80211AttrSetBuilder {
        Nl80211AttrSetBuilder::new()
    }

    /// Parses a byte slice into an `Nl80211AttrSet`.
    pub fn parse(attributes: &[u8]) -> Result<Self, NetlinkError> {
        Nl80211AttrPacket::new(attributes).to_owned()
    }

    /// Parses a byte slice into an `Nl80211AttrSet` using `mac80211_hwsim`
    /// attribute IDs.
    pub fn parse_hwsim(attributes: &[u8]) -> Result<Self, NetlinkError> {
        Nl80211AttrPacket::new(attributes).to_owned_hwsim()
    }
}

impl<'a> Nl80211AttrPacket<'a> {
    /// Creates a new `Nl80211AttrPacket` from a byte slice.
    pub fn new(attributes: &'a [u8]) -> Self {
        Self { attributes }
    }

    /// Converts the `Nl80211AttrPacket` to an owned `Nl80211AttrSet`.
    pub fn to_owned(&self) -> Result<Nl80211AttrSet, NetlinkError> {
        let mut set = Nl80211AttrSet { attributes: self.attributes.to_vec(), ..Default::default() };
        let mut attrs_to_parse = self.attributes;
        if self.attributes.len()
            >= nl80211_util::nla_align(std::mem::size_of::<crate::netlink::nl80211::GenlMsgHdr>())
        {
            let (hdr, rest) =
                zerocopy::Ref::<&[u8], crate::netlink::nl80211::GenlMsgHdr>::from_prefix(
                    self.attributes,
                )
                .map_err(|err| NetlinkError::BufferTooShort(err.to_string()))?;
            if hdr.cmd == 0 {
                attrs_to_parse = rest;
            }
        }

        for (hdr, payload) in nl80211_util::iter_nl_attrs(attrs_to_parse) {
            let attr_id = nl80211_util::get_attr_id_from_type(hdr.attr_type());
            match attr_id {
                attr_id::HW_INDEX => {
                    set.hw_index = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::IFACE_MAC => {
                    set.iface_mac = Some(nl80211_util::parse_mac_addr_from_payload(payload)?)
                }
                attr_id::IFACE_NAME => {
                    set.iface_name = nl80211_util::parse_string_from_payload(payload).ok();
                }
                attr_id::IFACE_TYPE => {
                    set.iface_type = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::REQ_IFACE_NUM => {
                    set.req_iface_num = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::MAX_IFACES => {
                    set.max_ifaces = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::REG_DOM => {
                    if payload.len() == 4 {
                        set.reg_dom = Some(nl80211_util::parse_u32_from_payload(payload)?)
                    }
                }
                attr_id::REG_ALPHA2 => {
                    set.reg_alpha2 = nl80211_util::parse_string_from_payload(payload).ok();
                }
                attr_id::CHANNEL => {
                    set.channel = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::FREQUENCY => {
                    set.frequency = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::CHANNEL_TYPE => {
                    set.channel_type = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::CHANNEL_FLAGS => {
                    set.channel_flags = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::MAX_TX_POWER => {
                    set.max_tx_power = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::CENTER_FREQ1 => {
                    set.center_freq1 = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::CENTER_FREQ2 => {
                    set.center_freq2 = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::SIGNAL => {
                    set.signal = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::NOISE => set.noise = Some(nl80211_util::parse_u32_from_payload(payload)?),
                attr_id::RX_RATE => {
                    set.rx_rate = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::KEY => set.key = Some(payload.to_vec()),
                attr_id::KEY_IDX => {
                    set.key_idx = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::KEY_DATA => set.key_data = Some(payload.to_vec()),
                attr_id::KEY_SEQ => set.key_seq = Some(payload.to_vec()),
                attr_id::KEY_FLAGS => {
                    set.key_flags = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::CIPHER => {
                    set.cipher = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::BEACON_INTERVAL => {
                    set.beacon_interval = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::DTIM_PERIOD => {
                    set.dtim_period = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::HIDDEN_SSID => {
                    set.hidden_ssid = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::SUPPORTED_RATES => set.supported_rates = Some(payload.to_vec()),
                attr_id::SHORT_PREAMBLE => {
                    set.short_preamble = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::SHORT_SLOT_TIME => {
                    set.short_slot_time = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::EDCA_PARAMS => set.edca_params = Some(payload.to_vec()),
                attr_id::WMM_ENABLED => {
                    set.wmm_enabled = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::POWER_CONSTRAINT => {
                    set.power_constraint = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::LOCAL_POWER_CONSTRAINT => {
                    set.local_power_constraint =
                        Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::TXPOWER => {
                    set.txpower = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::SUPPORTED_CHANNELS => set.supported_channels = Some(payload.to_vec()),
                attr_id::MESH_PATH => set.mesh_path = Some(payload.to_vec()),
                attr_id::MESH_ID => set.mesh_id = Some(payload.to_vec()),
                attr_id::MESH_PLINK_STATE => {
                    set.mesh_plink_state = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                attr_id::MESH_GATE_ANNOUNCEMENT => {
                    set.mesh_gate_announcement = Some(payload.to_vec())
                }
                attr_id::HWSIM_ATTR_FRAME_DATA => {
                    set.hwsim_attr_frame_data = Some(payload.to_vec())
                }
                attr_id::HWSIM_ATTR_COOKIE => {
                    if payload.len() == 8 {
                        let mut bytes = [0u8; 8];
                        bytes.copy_from_slice(payload);
                        set.hwsim_attr_cookie = Some(u64::from_le_bytes(bytes));
                    } else {
                        return Err(NetlinkError::InvalidPayloadLength);
                    }
                }
                attr_id::HWSIM_ATTR_FLAGS => {
                    set.hwsim_attr_flags = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                _ => (),
            }
        }
        Ok(set)
    }

    /// Converts the `Nl80211AttrPacket` to an owned `Nl80211AttrSet` using
    /// `mac80211_hwsim` attribute IDs.
    pub fn to_owned_hwsim(&self) -> Result<Nl80211AttrSet, NetlinkError> {
        let mut set = Nl80211AttrSet { attributes: self.attributes.to_vec(), ..Default::default() };
        let attrs_to_parse = self.attributes;

        for (hdr, payload) in nl80211_util::iter_nl_attrs(attrs_to_parse) {
            let attr_type = hdr.attr_type();
            let attr_enum = num_traits::FromPrimitive::from_u16(attr_type);

            match attr_enum {
                Some(HwsimAttrEnum::AddrTransmitter) => {
                    set.iface_mac = Some(nl80211_util::parse_mac_addr_from_payload(payload)?)
                }
                Some(HwsimAttrEnum::Frame) => set.hwsim_attr_frame_data = Some(payload.to_vec()),
                Some(HwsimAttrEnum::Flags) => {
                    set.hwsim_attr_flags = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                Some(HwsimAttrEnum::RxRate) => {
                    set.rx_rate = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                Some(HwsimAttrEnum::Signal) => {
                    set.signal = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                Some(HwsimAttrEnum::Cookie) => {
                    if payload.len() == 8 {
                        let mut bytes = [0u8; 8];
                        bytes.copy_from_slice(payload);
                        set.hwsim_attr_cookie = Some(u64::from_le_bytes(bytes));
                    } else {
                        return Err(NetlinkError::InvalidPayloadLength);
                    }
                }
                Some(HwsimAttrEnum::Freq) => {
                    set.frequency = Some(nl80211_util::parse_u32_from_payload(payload)?)
                }
                _ => {}
            }
        }
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ethernet::MacAddr,
        netlink::{nl80211::attr_id, nl80211_util},
    };

    #[test]
    fn test_nl80211_attr_set_builder_and_parser() {
        let mut builder = Nl80211AttrSet::builder();
        builder
            .u32_attr(attr_id::HW_INDEX, 1)
            .string_attr(attr_id::IFACE_NAME, "wlan0")
            .mac_addr_attr(
                attr_id::IFACE_MAC,
                &MacAddr { bytes: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55] },
            )
            .bytes_attr(attr_id::HWSIM_ATTR_FRAME_DATA, &[0xDE, 0xAD, 0xBE, 0xEF]);

        let set = builder.build();

        let attributes = nl80211_util::build_netlink_message(
            0,
            0,
            &[
                (attr_id::HW_INDEX, nl80211_util::create_u32_attr_payload(1)),
                (attr_id::IFACE_NAME, nl80211_util::create_string_attr_payload("wlan0")),
                (
                    attr_id::IFACE_MAC,
                    nl80211_util::create_mac_addr_attr_payload(&MacAddr {
                        bytes: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
                    }),
                ),
                (
                    attr_id::HWSIM_ATTR_FRAME_DATA,
                    nl80211_util::create_bytes_attr_payload(&[0xDE, 0xAD, 0xBE, 0xEF]),
                ),
            ],
        )
        .unwrap();

        let parsed_set = Nl80211AttrSet::parse(&attributes).unwrap();

        assert_eq!(set.hw_index, parsed_set.hw_index);
        assert_eq!(set.iface_name, parsed_set.iface_name);
        assert_eq!(set.iface_mac, parsed_set.iface_mac);
        assert_eq!(set.hwsim_attr_frame_data, parsed_set.hwsim_attr_frame_data);
    }

    // Validate `Nl80211AttrSet` attribute parsing from byte vector.
    #[test]
    fn test_attr_set_parse() {
        let packet: Vec<u8> = include_bytes!("test_data/hwsim_cmd_frame.bin").to_vec();
        let payload = &packet[20..];
        let attrs = Nl80211AttrSet::parse_hwsim(payload).unwrap();

        // Validate each attribute parsed
        assert!(attrs.iface_mac.is_some());
        assert!(attrs.hwsim_attr_frame_data.is_some());
        assert_eq!(attrs.hwsim_attr_flags, Some(2));
        assert!(attrs.rx_rate.is_none());
        assert!(attrs.signal.is_none());
        assert_eq!(attrs.hwsim_attr_cookie, Some(201));
        assert_eq!(attrs.frequency, Some(2422));
    }

    #[test]
    fn test_nl80211_attr_set_display() {
        let packet: Vec<u8> = include_bytes!("test_data/hwsim_cmd_frame.bin").to_vec();
        let payload = &packet[20..];
        let attrs = Nl80211AttrSet::parse_hwsim(payload).unwrap();

        let fmt_attrs = format!("{}", attrs);
        assert!(fmt_attrs.contains("iface_mac: 02:15:B2:00:00:00"));
        assert!(fmt_attrs.contains("hwsim_attr_cookie: 201"));
    }

    #[test]
    fn test_nl80211_attr_packet_new() {
        let attributes: &[u8] = &[0x01, 0x02, 0x03, 0x04];
        let packet = Nl80211AttrPacket::new(attributes);
        assert_eq!(packet.attributes, attributes);
    }

    #[test]
    fn test_nl80211_attr_packet_to_owned() {
        let attributes = nl80211_util::build_netlink_message(
            0,
            0,
            &[
                (attr_id::HW_INDEX, nl80211_util::create_u32_attr_payload(1)),
                (attr_id::IFACE_NAME, nl80211_util::create_string_attr_payload("wlan0")),
            ],
        )
        .unwrap();

        let packet = Nl80211AttrPacket::new(&attributes);
        let set = packet.to_owned().unwrap();

        assert_eq!(set.hw_index, Some(1));
        assert_eq!(set.iface_name, Some("wlan0".to_string()));
    }
}
