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

//! ieee80211 frames

// TODO: only allow the warnings for the included code
#![allow(clippy::all)]
#![allow(missing_docs)]
#![allow(unused)]
include!(concat!(env!("OUT_DIR"), "/ieee80211_packets.rs"));

use crate::llc::{EtherType, LlcCtrl, LlcSap, LlcSnapHeader};
use anyhow::anyhow;

// Constants for field lengths
const ETHERTYPE_LEN: usize = 2;
pub const CCMP_HDR_LEN: usize = 8;

// Constants for Ieee80211 definitions.
// Reference: external/wpa_supplicant_8/src/common/ieee802_11_defs.h
const WLAN_FC_RETRY: u16 = 0x0800;
const WLAN_FC_PWRMGT: u16 = 0x1000;
const WLAN_FC_MOREDATA: u16 = 0x2000;
const WLAN_FC_ISWEP: u16 = 0x4000;
const WLAN_ACTION_PUBLIC: u8 = 4;
const WLAN_ACTION_HT: u8 = 7;
const WLAN_ACTION_SELF_PROTECTED: u8 = 15;
const WLAN_ACTION_VENDOR_SPECIFIC: u8 = 127;

/// A Ieee80211 MAC address

impl MacAddress {
    pub const LEN: usize = 6;

    const MDNS_MULTICAST_V4: MacAddress =
        MacAddress(u64::from_le_bytes([0x01, 0x00, 0x5e, 0x00, 0x00, 0xfb, 0, 0]));
    const MDNS_MULTICAST_V6: MacAddress =
        MacAddress(u64::from_le_bytes([0x33, 0x33, 0x5e, 0x00, 0x00, 0xfb, 0, 0]));

    pub fn to_vec(&self) -> [u8; 6] {
        u64::to_le_bytes(self.0)[0..6].try_into().expect("slice with incorrect length")
    }

    pub fn is_multicast(&self) -> bool {
        let addr = u64::to_le_bytes(self.0);
        (addr[0] & 0x1) == 1
    }

    pub fn is_broadcast(&self) -> bool {
        self.0 == u64::MAX
    }

    pub fn is_mdns(&self) -> bool {
        self == &Self::MDNS_MULTICAST_V4 || self == &Self::MDNS_MULTICAST_V6
    }
}

// TODO: Add unit tests.
impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = u64::to_le_bytes(self.0);
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
        )
    }
}

impl fmt::Display for Ieee80211 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{{ds: {}, src: {}, dst: {}}}",
            self.get_ds(),
            self.get_source(),
            self.get_destination()
        )
    }
}

impl From<&[u8; 6]> for MacAddress {
    fn from(bytes: &[u8; 6]) -> Self {
        Self(u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], 0, 0]))
    }
}

impl From<MacAddress> for [u8; 6] {
    fn from(MacAddress(addr): MacAddress) -> Self {
        let bytes = u64::to_le_bytes(addr);
        bytes[0..6].try_into().unwrap()
    }
}

pub struct Ieee8023<'a> {
    destination: MacAddress,
    source: MacAddress,
    ethertype: EtherType,
    payload: &'a [u8],
}

impl<'a> Ieee8023<'a> {
    pub const HDR_LEN: usize = 14;

    /// Creates an `Ieee8023` instance from packet slice.
    fn from(packet: &'a [u8]) -> anyhow::Result<Self> {
        // Ensure the packet has enough bytes for the header
        anyhow::ensure!(
            packet.len() >= Self::HDR_LEN,
            "Packet (len: {}) too short for IEEE 802.3 header",
            packet.len()
        );
        let dest_slice: &[u8; 6] = packet[..MacAddress::LEN].try_into()?;
        let src_slice: &[u8; 6] = packet[MacAddress::LEN..2 * MacAddress::LEN].try_into()?;
        let ethertype_bytes = packet[2 * MacAddress::LEN..Self::HDR_LEN].try_into()?;
        let ethertype = EtherType::try_from(u16::from_be_bytes(ethertype_bytes))
            .map_err(|e| anyhow::anyhow!("invalid EtherType: {e}"))?;

        Ok(Ieee8023 {
            destination: MacAddress::from(dest_slice),
            source: MacAddress::from(src_slice),
            ethertype,
            payload: &packet[Self::HDR_LEN..],
        })
    }

    fn to_vec(self) -> anyhow::Result<Vec<u8>> {
        // Build 802.3 frame
        let mut ethernet_frame =
            Vec::with_capacity(MacAddress::LEN * 2 + ETHERTYPE_LEN + self.payload.len());

        ethernet_frame.extend_from_slice(&self.destination.to_vec());
        ethernet_frame.extend_from_slice(&self.source.to_vec());
        // Add extracted EtherType
        ethernet_frame.extend_from_slice(&u16::from(self.ethertype).to_be_bytes());
        // Actually data is after 802.2 LLC/SNAP header
        ethernet_frame.extend_from_slice(self.payload);
        Ok(ethernet_frame)
    }
}

impl Ieee80211 {
    /// Create Ieee80211 from Ieee8023 frame.
    pub fn from_ieee8023(packet: &[u8], bssid: MacAddress) -> anyhow::Result<Ieee80211> {
        let ieee8023 = Ieee8023::from(packet)?;

        let llc_snap_header = LlcSnapHeader {
            dsap: LlcSap::Snap,
            ssap: LlcSap::Snap,
            ctrl: LlcCtrl::UiCmd,
            oui: 0,
            ethertype: ieee8023.ethertype,
        };
        // IEEE80211 payload: LLC/SNAP Header + IEEE8023 payload
        let mut payload = Vec::with_capacity(LlcSnapHeader::LEN + ieee8023.payload.len());
        llc_snap_header.encode(&mut payload)?;
        payload.extend_from_slice(ieee8023.payload);

        Ok(Ieee80211FromAp {
            duration_id: 0,
            ftype: FrameType::Data,
            more_data: 0,
            more_frags: 0,
            order: 0,
            pm: 0,
            protected: 0,
            retry: 0,
            stype: 0,
            version: 0,
            bssid,
            source: ieee8023.source,
            destination: ieee8023.destination,
            seq_ctrl: 0,
            payload,
        }
        .try_into()?)
    }

    /// Frame has addr4 field
    pub fn has_a4(&self) -> bool {
        self.to_ds == 1 && self.from_ds == 1
    }

    /// Frame is sent to ap
    pub fn is_to_ap(&self) -> bool {
        self.to_ds == 1 && self.from_ds == 0
    }

    /// Frame type is management
    pub fn is_mgmt(&self) -> bool {
        self.ftype == FrameType::Mgmt
    }

    /// Generates the Additional Authentication Data (AAD) for CCMP encryption.
    ///
    /// Reference Linux kernel net/mac80211/wpa.c
    pub fn get_aad(&self) -> Vec<u8> {
        // Initialize AAD with header length - 2 bytes (no duration id)
        let hdr_len = self.hdr_length();
        let mut aad = vec![0u8; hdr_len - 2];

        // Construct the Frame Control bytes for the AAD:
        aad[0] = (self.version as u8) | (self.ftype as u8) << 2 | (self.stype as u8) << 4;

        if !self.is_mgmt() {
            // Clear the first three bits of stype (bits 4, 5, and 6)
            aad[0] &= !(0x07 << 4);
        }

        aad[1] = (self.to_ds as u8) << 0
        | (self.from_ds as u8) << 1
        | (self.more_frags as u8) << 2
        | (0 << 3) // Clear Retry bit
        | (0 << 4) // Clear Power Management bit
        | (0 << 5) // Clear More Data bit
        | (1 << 6) // Set Protected Frame bit
        | (self.order as u8) << 7;

        // Insert 3 MAC Addresses ( 3 * 6 = 18 bytes):
        aad[2..20].copy_from_slice(&self.payload[..18]);
        // Insert Masked Sequence Control.
        aad[20] = (self.payload[18] & 0x0f) as u8;
        // aad[21] is set to 0 by default

        // Handle Address 4 and QoS Control field (TID) as applicable
        if self.has_a4() {
            aad[22..28].copy_from_slice(&self.payload[20..26]);
            if self.is_qos_data() {
                aad[28] = self.get_qos_tid();
            }
        } else if self.is_qos_data() {
            aad[22] = self.get_qos_tid();
        }

        aad
    }

    /// Calculates the length of the IEEE 802.11 frame header.
    pub fn hdr_length(&self) -> usize {
        // Base header length is 24. +6 if Addr4 is used. +2 for QoS Data
        24 + (6 * self.has_a4() as usize) + (2 * self.is_qos_data() as usize)
    }

    /// Frame is a QoS Data frame
    pub fn is_qos_data(&self) -> bool {
        self.is_data() && self.stype == DataSubType::Qos as u8
    }

    /// Retrieves the QoS TID (Traffic Identifier) from the IEEE 802.11 frame
    pub fn get_qos_tid(&self) -> u8 {
        if !self.is_qos_data() {
            return 0; // No QoS Control field, return default TID 0
        }

        // QOS TID is last 2 bytes of header
        let qos_offset = self.hdr_length() - 2;
        // Extract the QoS TID
        let qos_control = u16::from_be_bytes(
            self.payload[qos_offset..qos_offset + 2]
                .try_into()
                .expect("Failed to convert QoS control bytes"),
        );

        (qos_control >> 8) as u8
    }

    /// Retrieves the QoS Control field from the IEEE 802.11 frame
    pub fn get_qos_control(&self) -> u16 {
        if !self.is_qos_data() {
            return 0;
        }
        u16::from_be_bytes(
            self.get_payload()[2..4].try_into().expect("Failed to convert QoS control bytes"),
        )
    }

    /// Extracts the Packet Number (PN) from the IEEE 802.11 frame
    pub fn get_packet_number(&self) -> [u8; 6] {
        let body_pos = self.hdr_length() - 4;
        let frame_body = &self.payload[body_pos..(body_pos + 8)]; // Get the packet num from frame

        // Extract the PN bytes in the specified order
        [frame_body[7], frame_body[6], frame_body[5], frame_body[4], frame_body[1], frame_body[0]]
    }

    /// Generates the Nonce for CCMP encryption
    ///
    /// Reference Linux kernel net/mac80211/wpa.c
    pub fn get_nonce(&self, pn: &[u8]) -> [u8; 13] {
        let qos_tid = self.get_qos_tid();
        let mgmt_flag = self.is_mgmt() as u8;
        let addr2 = self.get_addr2().to_vec();
        let mut nonce = [0u8; 13];
        // Construct the nonce using qos_tid, mgmt bit, addr2, and pn
        nonce[0] = qos_tid | (mgmt_flag << 4);
        nonce[1..7].copy_from_slice(&addr2);
        nonce[7..].copy_from_slice(pn);
        nonce
    }

    /// Check if the frame is multicast based on the destination address
    pub fn is_multicast(&self) -> bool {
        self.get_addr1().is_multicast()
    }

    /// Check if the frame is broadcast based on the destination address
    pub fn is_broadcast(&self) -> bool {
        self.get_addr1().is_broadcast()
    }

    /// Frame is Robust Management frame
    ///
    /// Reference Linux kernel include/linux/ieee80211.h
    pub fn is_robust_mgmt(&self) -> bool {
        if self.payload.len() < 21 || !self.is_mgmt() {
            // 25 - 4 (fc and duration id)
            return false;
        }

        match ManagementSubType::try_from(self.stype).unwrap() {
            // Disassoc and Deauth are robust mgmt
            ManagementSubType::Disassoc | ManagementSubType::Deauth => true,
            /*
             * Action frames, excluding Public Action frames, are Robust
             * Management Frames. However, if we are looking at a Protected
             * frame, skip the check since the data may be encrypted and
             * the frame has already been found to be a Robust Management
             * Frame (by the other end).
             */
            ManagementSubType::Action => {
                if self.is_protected() {
                    return true; // Assume protected Action frames are robust
                }
                // Access category at offset 20 (24 - 2 frame control - 2 dutation id)
                let category = u8::from_be_bytes([self.payload[20]]);

                !matches!(
                    category,
                    WLAN_ACTION_PUBLIC
                        | WLAN_ACTION_HT
                        | WLAN_ACTION_SELF_PROTECTED
                        | WLAN_ACTION_VENDOR_SPECIFIC
                )
            }
            _ => false, // Other management frames are not robust by default
        }
    }

    /// Frame is (management) beacon frame
    pub fn is_beacon(&self) -> bool {
        self.ftype == FrameType::Mgmt && self.stype == (ManagementSubType::Beacon as u8)
    }

    /// Frame type is data
    pub fn is_data(&self) -> bool {
        self.ftype == FrameType::Data
    }

    /// Frame is probe request
    pub fn is_probe_req(&self) -> bool {
        self.ftype == FrameType::Ctl && self.stype == (ManagementSubType::ProbeReq as u8)
    }

    /// Frame is protected
    pub fn is_protected(&self) -> bool {
        self.protected != 0u8
    }

    /// Frame type is EAPoL
    pub fn is_eapol(&self) -> anyhow::Result<bool> {
        Ok(self.get_ethertype()? == EtherType::Eapol)
    }

    /// Whether frame needs to be encrypted
    pub fn needs_encryption(&self) -> bool {
        !self.is_protected() && (self.is_data() || self.is_robust_mgmt())
    }

    /// Whether frame needs to be decrypted
    pub fn needs_decryption(&self) -> bool {
        self.is_protected() && (self.is_data() || self.is_robust_mgmt())
    }

    /// Set whether frame is protected
    pub fn set_protected(&mut self, protected: bool) {
        self.protected = protected.into();
    }

    pub fn get_ds(&self) -> String {
        match self.specialize().unwrap() {
            Ieee80211Child::Ieee80211ToAp(hdr) => "ToAp",
            Ieee80211Child::Ieee80211FromAp(hdr) => "FromAp",
            Ieee80211Child::Ieee80211Ibss(hdr) => "Ibss",
            Ieee80211Child::Ieee80211Wds(hdr) => "Wds",
            _ => panic!("unexpected specialized header"),
        }
        .to_string()
    }

    pub fn get_source(&self) -> MacAddress {
        match self.specialize().unwrap() {
            Ieee80211Child::Ieee80211ToAp(hdr) => hdr.source,
            Ieee80211Child::Ieee80211FromAp(hdr) => hdr.source,
            Ieee80211Child::Ieee80211Ibss(hdr) => hdr.source,
            Ieee80211Child::Ieee80211Wds(hdr) => hdr.source,
            _ => panic!("unexpected specialized header"),
        }
    }

    /// Ieee80211 packets have 3-4 addresses in different positions based
    /// on the FromDS and ToDS flags. This function gets the destination
    /// address depending on the FromDS+ToDS packet subtypes.
    pub fn get_destination(&self) -> MacAddress {
        match self.specialize().unwrap() {
            Ieee80211Child::Ieee80211ToAp(hdr) => hdr.destination,
            Ieee80211Child::Ieee80211FromAp(hdr) => hdr.destination,
            Ieee80211Child::Ieee80211Ibss(hdr) => hdr.destination,
            Ieee80211Child::Ieee80211Wds(hdr) => hdr.destination,
            _ => panic!("unexpected specialized header"),
        }
    }

    pub fn get_bssid(&self) -> Option<MacAddress> {
        match self.specialize().unwrap() {
            Ieee80211Child::Ieee80211ToAp(hdr) => Some(hdr.bssid),
            Ieee80211Child::Ieee80211FromAp(hdr) => Some(hdr.bssid),
            Ieee80211Child::Ieee80211Ibss(hdr) => Some(hdr.bssid),
            Ieee80211Child::Ieee80211Wds(hdr) => None,
            _ => panic!("unexpected specialized header"),
        }
    }

    pub fn get_addr1(&self) -> MacAddress {
        match self.specialize().unwrap() {
            Ieee80211Child::Ieee80211ToAp(hdr) => hdr.bssid,
            Ieee80211Child::Ieee80211FromAp(hdr) => hdr.destination,
            Ieee80211Child::Ieee80211Ibss(hdr) => hdr.destination,
            Ieee80211Child::Ieee80211Wds(hdr) => hdr.receiver,
            _ => panic!("unexpected specialized header"),
        }
    }

    pub fn get_addr2(&self) -> MacAddress {
        match self.specialize().unwrap() {
            Ieee80211Child::Ieee80211Ibss(hdr) => hdr.source,
            Ieee80211Child::Ieee80211FromAp(hdr) => hdr.bssid,
            Ieee80211Child::Ieee80211ToAp(hdr) => hdr.source,
            Ieee80211Child::Ieee80211Wds(hdr) => hdr.transmitter,
            _ => panic!("unexpected specialized header"),
        }
    }

    pub fn get_ssid_from_beacon_frame(&self) -> anyhow::Result<String> {
        // Verify packet is a beacon frame
        if !self.is_beacon() {
            return Err(anyhow!("Frame is not beacon frame."));
        };

        // SSID field starts after the first 36 bytes. Ieee80211 payload starts after 4 bytes.
        let pos = 36 - 4;

        // Check for SSID element ID (0) and extract the SSID
        let payload = &self.payload;
        if payload[pos] == 0 {
            let ssid_len = payload[pos + 1] as usize;
            let ssid_bytes = &payload[pos + 2..pos + 2 + ssid_len];
            return Ok(String::from_utf8(ssid_bytes.to_vec())?);
        }

        Err(anyhow!("SSID not found."))
    }

    fn get_payload(&self) -> Vec<u8> {
        match self.specialize().unwrap() {
            Ieee80211Child::Ieee80211ToAp(hdr) => hdr.payload,
            Ieee80211Child::Ieee80211FromAp(hdr) => hdr.payload,
            Ieee80211Child::Ieee80211Ibss(hdr) => hdr.payload,
            Ieee80211Child::Ieee80211Wds(hdr) => hdr.payload,
            _ => panic!("unexpected specialized header"),
        }
    }

    fn get_ethertype(&self) -> anyhow::Result<EtherType> {
        if !self.is_data() {
            return Err(anyhow!("Not an 802.2 LLC/SNAP frame"));
        }

        // Extract and validate LLC/SNAP header
        let payload = self.get_payload();
        if payload.len() < LlcSnapHeader::LEN {
            return Err(anyhow!("Payload too short for LLC/SNAP header"));
        }
        let llc_snap_header = LlcSnapHeader::decode_full(&payload[..LlcSnapHeader::LEN])?;
        Ok(llc_snap_header.ethertype())
    }

    pub fn with_address(
        &self,
        source: Option<MacAddress>,
        destination: Option<MacAddress>,
    ) -> Ieee80211 {
        match self.specialize().unwrap() {
            Ieee80211Child::Ieee80211ToAp(frame) => {
                frame.with_address(source, destination).try_into().unwrap()
            }
            Ieee80211Child::Ieee80211FromAp(frame) => {
                frame.with_address(source, destination).try_into().unwrap()
            }
            Ieee80211Child::Ieee80211Ibss(frame) => {
                frame.with_address(source, destination).try_into().unwrap()
            }
            Ieee80211Child::Ieee80211Wds(frame) => {
                frame.with_address(source, destination).try_into().unwrap()
            }
            _ => panic!("Unknown Ieee80211Child type"),
        }
    }

    /// Covert Ieee80211ToAp to Ieee80211FromAp packet.
    pub fn into_from_ap(&self) -> anyhow::Result<Ieee80211FromAp> {
        match self.specialize().unwrap() {
            Ieee80211Child::Ieee80211ToAp(frame_to_ap) => {
                // Flip from_ap and to_ap bits.
                // TODO: Investigate if there is a way to copy frame_control flags at once.
                // The header struct only has 7 fields, not 15. Most fields come from le16 frame_control.
                Ok(Ieee80211FromAp {
                    duration_id: frame_to_ap.duration_id,
                    ftype: frame_to_ap.ftype,
                    more_data: frame_to_ap.more_data,
                    more_frags: frame_to_ap.more_frags,
                    order: frame_to_ap.order,
                    pm: frame_to_ap.pm,
                    protected: frame_to_ap.protected,
                    retry: frame_to_ap.retry,
                    stype: frame_to_ap.stype,
                    version: frame_to_ap.version,
                    bssid: frame_to_ap.bssid,
                    source: frame_to_ap.source,
                    destination: frame_to_ap.destination,
                    seq_ctrl: frame_to_ap.seq_ctrl,
                    payload: frame_to_ap.payload.to_vec(),
                })
            }
            _ => Err(anyhow!(
                "Invalid Ieee80211Child packet. from_ds: {}, to_ds: {}",
                self.from_ds,
                self.to_ds
            )),
        }
    }

    // Convert to ieee802.3
    pub fn to_ieee8023(&self) -> anyhow::Result<Vec<u8>> {
        let ethertype = self.get_ethertype()?;
        let payload = self.get_payload();
        Ieee8023 {
            destination: self.get_destination(),
            source: self.get_source(),
            ethertype,
            payload: &payload[LlcSnapHeader::LEN..],
        }
        .to_vec()
    }
}

impl Ieee80211FromAp {
    pub fn with_address(
        &self,
        source: Option<MacAddress>,
        destination: Option<MacAddress>,
    ) -> Ieee80211FromAp {
        Ieee80211FromAp {
            source: source.unwrap_or(self.source),
            destination: destination.unwrap_or(self.destination),
            ..self.clone()
        }
    }
}

impl Ieee80211ToAp {
    pub fn with_address(
        &self,
        source: Option<MacAddress>,
        destination: Option<MacAddress>,
    ) -> Ieee80211ToAp {
        Ieee80211ToAp {
            source: source.unwrap_or(self.source),
            destination: destination.unwrap_or(self.destination),
            ..self.clone()
        }
    }
}

impl Ieee80211Ibss {
    pub fn with_address(
        &self,
        source: Option<MacAddress>,
        destination: Option<MacAddress>,
    ) -> Ieee80211Ibss {
        Ieee80211Ibss {
            source: source.unwrap_or(self.source),
            destination: destination.unwrap_or(self.destination),
            ..self.clone()
        }
    }
}

impl Ieee80211Wds {
    pub fn with_address(
        &self,
        source: Option<MacAddress>,
        destination: Option<MacAddress>,
    ) -> Ieee80211Wds {
        Ieee80211Wds {
            source: source.unwrap_or(self.source),
            destination: destination.unwrap_or(self.destination),
            ..self.clone()
        }
    }
}

pub fn parse_mac_address(s: &str) -> Option<MacAddress> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut bytes = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        match u8::from_str_radix(part, 16) {
            Ok(n) => bytes[i] = n,
            Err(e) => return None,
        }
    }
    Some(MacAddress::from(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_address_len() {
        let mac_address: MacAddress = parse_mac_address("00:0b:85:71:20:ce").unwrap();
        assert_eq!(mac_address.encoded_len(), MacAddress::LEN);
    }

    #[test]
    fn test_mac_address_to_vec() {
        let mac_address: MacAddress = parse_mac_address("00:0b:85:71:20:ce").unwrap();
        let mac_address_bytes = mac_address.to_vec();
        let reconstructed_mac_address = MacAddress::from(&mac_address_bytes);
        assert_eq!(mac_address, reconstructed_mac_address);
    }

    // These tests use the packets available here
    // https://community.cisco.com/t5/wireless-mobility-knowledge-base/802-11-frames-a-starter-guide-to-learn-wireless-sniffer-traces/ta-p/3110019

    #[test]
    fn test_frame_qos() {
        let frame: Vec<u8> = vec![
            0x88, 0x02, 0x2c, 0x00, 0x00, 0x13, 0xe8, 0xeb, 0xd6, 0x03, 0x00, 0x0b, 0x85, 0x71,
            0x20, 0xce, 0x00, 0x0b, 0x85, 0x71, 0x20, 0xce, 0x00, 0x26, 0x00, 0x00,
        ];
        let hdr = Ieee80211::decode_full(&frame).unwrap();
        assert!(hdr.is_data());
        assert_eq!(hdr.stype, DataSubType::Qos as u8);
        assert_eq!(hdr.from_ds, 1);
        assert_eq!(hdr.to_ds, 0);
        assert_eq!(hdr.duration_id, 44);
        // Source address: Cisco_71:20:ce (00:0b:85:71:20:ce)
        let a = format!("{}", hdr.get_source());
        let b = format!("{}", parse_mac_address("00:0b:85:71:20:ce").unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn test_beacon_frame() {
        // Example from actual beacon frame from Hostapd with "AndroidWifi" SSID
        let frame: Vec<u8> = vec![
            0x80, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x13, 0x10, 0x85,
            0xfe, 0x01, 0x00, 0x13, 0x10, 0x85, 0xfe, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0xe8, 0x03, 0x01, 0x04, 0x00, 0x0b, 0x41, 0x6e, 0x64, 0x72,
            0x6f, 0x69, 0x64, 0x57, 0x69, 0x66, 0x69, 0x01, 0x04, 0x82, 0x84, 0x8b, 0x96, 0x03,
            0x01, 0x08, 0x2a, 0x01, 0x07, 0x2d, 0x1a, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x3d, 0x16, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x7f, 0x04, 0x00, 0x00, 0x00, 0x02,
        ];
        let decoded_frame = Ieee80211::decode_full(&frame).unwrap();
        assert!(decoded_frame.is_mgmt());
        assert!(decoded_frame.is_beacon());
        let ssid = decoded_frame.get_ssid_from_beacon_frame();
        assert!(ssid.is_ok());
        assert_eq!(ssid.unwrap(), "AndroidWifi");
    }

    #[test]
    fn test_is_multicast() {
        // Multicast MAC address: 01:00:5E:00:00:FB
        let mdns_mac_address = parse_mac_address("01:00:5e:00:00:fb").unwrap();
        assert!(mdns_mac_address.is_multicast());
        // Broadcast MAC address: ff:ff:ff:ff:ff:ff
        let broadcast_mac_address = parse_mac_address("ff:ff:ff:ff:ff:ff").unwrap();
        assert!(broadcast_mac_address.is_multicast());
        // Source address: Cisco_71:20:ce (00:0b:85:71:20:ce)
        let non_mdns_mac_address = parse_mac_address("00:0b:85:71:20:ce").unwrap();
        assert!(!non_mdns_mac_address.is_multicast());
    }

    fn test_is_broadcast() {
        // Multicast MAC address: 01:00:5E:00:00:FB
        let mdns_mac_address = parse_mac_address("01:00:5e:00:00:fb").unwrap();
        assert!(!mdns_mac_address.is_broadcast());
        // Broadcast MAC address: ff:ff:ff:ff:ff:ff
        let broadcast_mac_address = parse_mac_address("ff:ff:ff:ff:ff:ff").unwrap();
        assert!(broadcast_mac_address.is_broadcast());
        // Source address: Cisco_71:20:ce (00:0b:85:71:20:ce)
        let non_mdns_mac_address = parse_mac_address("00:0b:85:71:20:ce").unwrap();
        assert!(!non_mdns_mac_address.is_broadcast());
    }

    #[test]
    fn test_ieee8023_from_valid_packet() {
        let packet: [u8; 20] = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, // Destination MAC
            0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, // Source MAC
            0x08, 0x00, // EtherType (IPv4)
            0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11, // Data
        ];

        let result = Ieee8023::from(&packet);
        assert!(result.is_ok());

        let ieee8023 = result.unwrap();
        assert_eq!(ieee8023.destination.to_vec(), [0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(ieee8023.source.to_vec(), [0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB]);
        assert_eq!(ieee8023.ethertype, EtherType::IPv4);
        assert_eq!(ieee8023.payload, &[0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11]);
    }

    #[test]
    fn test_ieee8023_from_short_packet() {
        let packet: [u8; 10] = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99];

        let result = Ieee8023::from(&packet);
        assert!(result.is_err());
    }

    fn create_test_from_ap_ieee80211(
        source: MacAddress,
        destination: MacAddress,
        bssid: MacAddress,
    ) -> Ieee80211 {
        Ieee80211FromAp {
            duration_id: 0,
            ftype: FrameType::Mgmt,
            more_data: 0,
            more_frags: 0,
            order: 0,
            pm: 0,
            protected: 0,
            retry: 0,
            stype: 0,
            version: 0,
            bssid,
            source,
            destination,
            seq_ctrl: 0,
            payload: Vec::new(),
        }
        .try_into()
        .unwrap()
    }

    fn create_test_ibss_ieee80211(
        source: MacAddress,
        destination: MacAddress,
        bssid: MacAddress,
    ) -> Ieee80211 {
        Ieee80211Ibss {
            duration_id: 0,
            ftype: FrameType::Mgmt,
            more_data: 0,
            more_frags: 0,
            order: 0,
            pm: 0,
            protected: 0,
            retry: 0,
            stype: 0,
            version: 0,
            bssid,
            source,
            destination,
            seq_ctrl: 0,
            payload: Vec::new(),
        }
        .try_into()
        .unwrap()
    }

    fn create_test_to_ap_ieee80211(
        source: MacAddress,
        destination: MacAddress,
        bssid: MacAddress,
    ) -> Ieee80211 {
        Ieee80211ToAp {
            duration_id: 0,
            ftype: FrameType::Mgmt,
            more_data: 0,
            more_frags: 0,
            order: 0,
            pm: 0,
            protected: 0,
            retry: 0,
            stype: 0,
            version: 0,
            bssid,
            source,
            destination,
            seq_ctrl: 0,
            payload: Vec::new(),
        }
        .try_into()
        .unwrap()
    }

    fn create_test_wds_ieee80211(
        receiver: MacAddress,
        transmitter: MacAddress,
        destination: MacAddress,
        source: MacAddress,
    ) -> Ieee80211 {
        Ieee80211Wds {
            duration_id: 0,
            ftype: FrameType::Mgmt,
            more_data: 0,
            more_frags: 0,
            order: 0,
            pm: 0,
            protected: 0,
            retry: 0,
            stype: 0,
            version: 0,
            receiver,
            transmitter,
            destination,
            seq_ctrl: 0,
            source,
            payload: Vec::new(),
        }
        .try_into()
        .unwrap()
    }

    fn test_with_address(
        create_test_ieee80211: fn(MacAddress, MacAddress, MacAddress) -> Ieee80211,
    ) {
        let source = parse_mac_address("01:02:03:00:00:01").unwrap();
        let destination = parse_mac_address("01:02:03:00:00:02").unwrap();
        let bssid = parse_mac_address("00:13:10:85:fe:01").unwrap();
        let ieee80211 = create_test_ieee80211(source, destination, bssid);

        let new_source = parse_mac_address("01:02:03:00:00:03").unwrap();
        let new_destination = parse_mac_address("01:02:03:00:00:04").unwrap();

        let new_ieee80211 = ieee80211.with_address(Some(new_source), Some(new_destination));
        assert!(new_ieee80211.get_source() == new_source);
        assert!(new_ieee80211.get_destination() == new_destination);

        let new_ieee80211 = ieee80211.with_address(Some(new_source), None);
        assert!(new_ieee80211.get_source() == new_source);
        assert!(new_ieee80211.get_destination() == destination);

        let new_ieee80211 = ieee80211.with_address(None, Some(new_destination));
        assert!(new_ieee80211.get_source() == source);
        assert!(new_ieee80211.get_destination() == new_destination);
    }

    #[test]
    fn test_with_address_from_ap() {
        test_with_address(create_test_from_ap_ieee80211);
    }

    #[test]
    fn test_with_address_to_ap() {
        test_with_address(create_test_to_ap_ieee80211);
    }
    #[test]
    fn test_with_address_ibss() {
        test_with_address(create_test_ibss_ieee80211);
    }

    #[test]
    fn test_to_ieee8023() {
        let source = parse_mac_address("01:02:03:00:00:01").unwrap();
        let destination = parse_mac_address("01:02:03:00:00:02").unwrap();
        let bssid = parse_mac_address("00:13:10:85:fe:01").unwrap();

        // Test Data (802.11 frame with LLC/SNAP)
        let ieee80211: Ieee80211 = Ieee80211ToAp {
            duration_id: 0,
            ftype: FrameType::Data,
            more_data: 0,
            more_frags: 0,
            order: 0,
            pm: 0,
            protected: 0,
            retry: 0,
            stype: 0,
            version: 0,
            bssid,
            source,
            destination,
            seq_ctrl: 0,
            payload: vec![
                // LLC/SNAP Header
                LlcSap::Snap as u8,
                LlcSap::Snap as u8,
                LlcCtrl::UiCmd as u8,
                // OUI
                0x00,
                0x00,
                0x00,
                // EtherType
                0x08,
                0x00,
            ],
        }
        .try_into()
        .unwrap();

        // Call the function under test
        let result = ieee80211.to_ieee8023();
        // Assert
        assert!(result.is_ok());
        let ethernet_frame = result.unwrap();

        // Verify ethernet frame
        assert_eq!(&ethernet_frame[0..6], destination.to_vec().as_slice()); // Destination MAC
        assert_eq!(&ethernet_frame[6..12], source.to_vec().as_slice()); // Source MAC
        assert_eq!(&ethernet_frame[12..14], [0x08, 0x00]); // EtherType
    }

    #[test]
    fn test_has_a4() {
        let addr1 = parse_mac_address("01:02:03:00:00:01").unwrap();
        let addr2 = parse_mac_address("01:02:03:00:00:02").unwrap();
        let addr3 = parse_mac_address("01:02:03:00:00:03").unwrap();
        let addr4 = parse_mac_address("01:02:03:00:00:04").unwrap();

        // Only WDS has addr4
        assert!(!create_test_from_ap_ieee80211(addr1, addr2, addr3).has_a4());
        assert!(!create_test_ibss_ieee80211(addr1, addr2, addr3).has_a4());
        assert!(!create_test_to_ap_ieee80211(addr1, addr2, addr3).has_a4());
        assert!(create_test_wds_ieee80211(addr1, addr2, addr3, addr4).has_a4());
    }
}
