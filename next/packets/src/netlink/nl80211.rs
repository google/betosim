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

//! Defines structures for parsing `nl80211` Netlink packets, specifically for
//! the `mac80211_hwsim` driver.
//!
//! The `nl80211` protocol is used for communication between user space daemons
//! (like `wpa_supplicant` or `hostapd`) and the kernel's wireless subsystem
//! (`mac80211`). The `mac80211_hwsim` is a software-simulated WiFi device
//! that uses this `nl80211` interface. This module provides the data structures
//! to parse, create, and interpret these Netlink messages, allowing a user
//! space daemon to control and interact with simulated WiFi hardware.

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, U16, Unaligned, byteorder::LittleEndian,
};

/// Attribute IDs used in Netlink messages for mac80211_hwsim.
/// These IDs correspond to specific data types or actions.
pub mod attr_id {
    /// Indicates the index of the simulated hardware device.
    pub const HW_INDEX: u16 = 1;
    /// Indicates the MAC address of a simulated interface.
    pub const IFACE_MAC: u16 = 2;
    /// Indicates the name of a simulated interface.
    pub const IFACE_NAME: u16 = 3;
    /// Indicates the type of a simulated interface (station, AP, etc.).
    pub const IFACE_TYPE: u16 = 4;
    /// Requests the creation of a simulated interface.
    pub const REQ_IFACE_NUM: u16 = 5;
    /// Notifies about the maximum number of interfaces supported by the
    /// hardware.
    pub const MAX_IFACES: u16 = 6;
    /// Sets or gets the regulatory domain index.
    pub const REG_DOM: u16 = 7;
    /// Sets or gets the regulatory alpha2 code.
    pub const REG_ALPHA2: u16 = 8;
    /// Triggers a channel switch operation.
    pub const CHANNEL: u16 = 9;
    /// Sets or gets the frequency of the current channel.
    pub const FREQUENCY: u16 = 10;
    /// Sets or gets the channel type (active, passive, etc.).
    pub const CHANNEL_TYPE: u16 = 11;
    /// Sets or gets the channel flags (disabled, no IR, radar detect, etc.).
    pub const CHANNEL_FLAGS: u16 = 12;
    /// Sets or gets the maximum transmission power in dBm.
    pub const MAX_TX_POWER: u16 = 13;
    /// Sets or gets the center frequency of the first segment for HT40
    /// channels.
    pub const CENTER_FREQ1: u16 = 14;
    /// Sets or gets the center frequency of the second segment for 80/160 MHz
    /// channels.
    pub const CENTER_FREQ2: u16 = 15;
    /// Sets or gets the signal strength of a received frame.
    pub const SIGNAL: u16 = 16;
    /// Sets or gets the noise level.
    pub const NOISE: u16 = 17;
    /// Indicates the timestamp of a received frame.
    pub const RX_RATE: u16 = 18;
    /// Adds or removes a key (e.g., for WEP, WPA).
    pub const KEY: u16 = 19;
    /// Sets or gets the key index.
    pub const KEY_IDX: u16 = 20;
    /// Sets or gets the key data.
    pub const KEY_DATA: u16 = 21;
    /// Sets or gets the key sequence number.
    pub const KEY_SEQ: u16 = 22;
    /// Sets or gets the key flags (e.g., whether it's a group key).
    pub const KEY_FLAGS: u16 = 23;
    /// Sets or gets the cipher suite used for encryption.
    pub const CIPHER: u16 = 24;
    /// Sets or gets the beacon interval in milliseconds.
    pub const BEACON_INTERVAL: u16 = 25;
    /// Sets or gets the DTIM period.
    pub const DTIM_PERIOD: u16 = 26;
    /// Sets or gets the hidden SSID mode.
    pub const HIDDEN_SSID: u16 = 27;
    /// Sets or gets the supported rates.
    pub const SUPPORTED_RATES: u16 = 28;
    /// Enables or disables short preamble.
    pub const SHORT_PREAMBLE: u16 = 29;
    /// Sets or gets the short slot time enabled status.
    pub const SHORT_SLOT_TIME: u16 = 30;
    /// Sets or gets the EDCA parameters.
    pub const EDCA_PARAMS: u16 = 31;
    /// Enables or disables WMM (Wireless Multimedia).
    pub const WMM_ENABLED: u16 = 32;
    /// Sets or gets the power constraint.
    pub const POWER_CONSTRAINT: u16 = 33;
    /// Sets or gets the local power constraint.
    pub const LOCAL_POWER_CONSTRAINT: u16 = 34;
    /// Sets or gets the transmit power.
    pub const TXPOWER: u16 = 35;
    /// Sets or gets the supported channels.
    pub const SUPPORTED_CHANNELS: u16 = 36;
    /// Adds or removes a mesh path.
    pub const MESH_PATH: u16 = 37;
    /// Sets or gets the mesh ID.
    pub const MESH_ID: u16 = 38;
    /// Sets or gets the mesh PLINK state.
    pub const MESH_PLINK_STATE: u16 = 39;
    /// Sets or gets the mesh gate announcement parameters.
    pub const MESH_GATE_ANNOUNCEMENT: u16 = 40;
    // HWSIM specific attributes (examples, actual values might differ)
    /// Carries the raw 802.11 frame data for TX/RX operations.
    pub const HWSIM_ATTR_FRAME_DATA: u16 = 100;
    /// Cookie for identifying frames.
    pub const HWSIM_ATTR_COOKIE: u16 = 101;
    /// Flags for frame transmission (e.g., NO_ACK).
    pub const HWSIM_ATTR_FLAGS: u16 = 102;
}

/// Generic Netlink message header (`struct genlmsghdr`).
/// This header precedes the Netlink attributes in a generic Netlink message.
#[repr(C)]
#[derive(Debug, Copy, Clone, FromBytes, IntoBytes, Unaligned, Immutable, KnownLayout)]
pub struct GenlMsgHdr {
    /// Command (`nlmsg_type` for the generic netlink family).
    pub cmd: u8,
    /// Version of the generic netlink family specific header.
    pub version: u8,
    /// Reserved for future use.
    pub reserved: U16<LittleEndian>,
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::*;
    use crate::netlink::nl80211::attr_id::HWSIM_ATTR_FRAME_DATA;

    #[test]
    fn test_genl_msg_hdr_size() {
        assert_eq!(size_of::<GenlMsgHdr>(), 4, "GenlMsgHdr size should be 4 bytes");
    }

    #[test]
    fn test_hwsim_attr_ids() {
        // Just a simple check to ensure the constant is accessible
        assert_eq!(HWSIM_ATTR_FRAME_DATA, 100);
    }
}
