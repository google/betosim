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

//! Provides utility functions for working with IEEE 802.11 frames and headers.

#[cfg(test)]
use std::fmt::Write;

#[cfg(test)]
use crate::ieee80211::FrameControl;
#[cfg(test)]
use crate::ieee80211::management_subtype;
use crate::{ethernet::MacAddr, ieee80211::MacHeader3Addr};

/// Converts a FrameControl field to a human-readable string.
#[cfg(test)]
pub fn frame_control_to_string(fc: FrameControl) -> String {
    use crate::ieee80211::frame_type;
    let mut s = String::new();
    write!(s, "FC:0x{:04X} ", fc.get()).unwrap();
    s.push_str(&frame_type_to_string(fc.frame_type()));
    s.push(',');

    let subtype_str = match fc.frame_type() {
        frame_type::MANAGEMENT => management_subtype_to_string(fc.frame_subtype()),
        frame_type::DATA => data_subtype_to_string(fc.frame_subtype()),
        frame_type::CONTROL => format!("CtrlSubtype:{}", fc.frame_subtype()), // Placeholder
        _ => format!("Subtype:{}", fc.frame_subtype()),
    };
    s.push_str(&subtype_str);
    s.push(',');

    if fc.to_ds() {
        write!(s, "ToDS,").unwrap();
    }
    if fc.from_ds() {
        write!(s, "FromDS,").unwrap();
    }
    if fc.more_fragments() {
        write!(s, "MoreFrag,").unwrap();
    }
    if fc.retry() {
        write!(s, "Retry,").unwrap();
    }
    if fc.power_management() {
        write!(s, "PwrMgmt,").unwrap();
    }
    if fc.more_data() {
        write!(s, "MoreData,").unwrap();
    }
    if fc.protected_frame() {
        write!(s, "Protected,").unwrap();
    }
    if fc.order() {
        write!(s, "Order,").unwrap();
    }

    // Remove trailing comma if present
    if s.ends_with(',') {
        s.pop();
    }
    s
}

/// Converts an IEEE 802.11 frame type value to a string.
#[cfg(test)]
pub fn frame_type_to_string(frame_type_val: u8) -> String {
    use crate::ieee80211::frame_type;
    match frame_type_val {
        frame_type::MANAGEMENT => "Mgmt".to_string(),
        frame_type::CONTROL => "Ctrl".to_string(),
        frame_type::DATA => "Data".to_string(),
        _ => format!("Type:{}", frame_type_val),
    }
}

/// Converts an IEEE 802.11 management frame subtype value to a string.
#[cfg(test)]
pub fn management_subtype_to_string(subtype_val: u8) -> String {
    match subtype_val {
        management_subtype::ASSOCIATION_REQUEST => "AssocReq".to_string(),
        management_subtype::ASSOCIATION_RESPONSE => "AssocResp".to_string(),
        management_subtype::REASSOCIATION_REQUEST => "ReassocReq".to_string(),
        management_subtype::REASSOCIATION_RESPONSE => "ReassocResp".to_string(),
        management_subtype::PROBE_REQUEST => "ProbeReq".to_string(),
        management_subtype::PROBE_RESPONSE => "ProbeResp".to_string(),
        management_subtype::BEACON => "Beacon".to_string(),
        management_subtype::ATIM => "ATIM".to_string(),
        management_subtype::DISASSOCIATION => "Disassoc".to_string(),
        management_subtype::AUTHENTICATION => "Auth".to_string(),
        management_subtype::DEAUTHENTICATION => "Deauth".to_string(),
        management_subtype::ACTION => "Action".to_string(),
        _ => format!("MgmtSubtype:{}", subtype_val),
    }
}

/// Converts an IEEE 802.11 data frame subtype value to a string.
#[cfg(test)]
pub fn data_subtype_to_string(subtype_val: u8) -> String {
    use crate::ieee80211::data_subtype;
    match subtype_val {
        data_subtype::DATA => "Data".to_string(),
        data_subtype::DATA_CF_ACK => "DataCfAck".to_string(),
        data_subtype::DATA_CF_POLL => "DataCfPoll".to_string(),
        data_subtype::DATA_CF_ACK_POLL => "DataCfAckPoll".to_string(),
        data_subtype::NULL => "Null".to_string(),
        data_subtype::CF_ACK => "CfAck".to_string(),
        data_subtype::CF_POLL => "CfPoll".to_string(),
        data_subtype::CF_ACK_POLL => "CfAckPoll".to_string(),
        data_subtype::QOS_DATA => "QoSData".to_string(),
        _ => format!("DataSubtype:{}", subtype_val),
    }
}

#[cfg(test)]
pub fn is_beacon_frame(fc: FrameControl) -> bool {
    use crate::ieee80211::frame_type;
    fc.frame_type() == frame_type::MANAGEMENT && fc.frame_subtype() == management_subtype::BEACON
}

/// Gets the Receiver Address (RA) from a 3-address MAC header.
/// RA is always Addr1.
pub fn get_receiver_address(header: &MacHeader3Addr) -> MacAddr {
    header.addr1
}

/// Gets the Transmitter Address (TA) from a 3-address MAC header.
/// TA is always Addr2.
pub fn get_transmitter_address(header: &MacHeader3Addr) -> MacAddr {
    header.addr2
}

/// Gets the Destination Address (DA) from a 3-address MAC header.
/// - If ToDS is set, DA is Addr3.
/// - Otherwise (ToDS is not set), DA is Addr1.
pub fn get_destination_address(header: &MacHeader3Addr) -> MacAddr {
    if header.frame_control.to_ds() { header.addr3 } else { header.addr1 }
}

/// Gets the Source Address (SA) from a 3-address MAC header.
/// - If FromDS is set, SA is Addr3.
/// - Otherwise (FromDS is not set), SA is Addr2.
pub fn get_source_address(header: &MacHeader3Addr) -> MacAddr {
    if header.frame_control.from_ds() { header.addr3 } else { header.addr2 }
}

/// Gets the BSSID from a 3-address MAC header.
/// - ToDS=0, FromDS=0: BSSID is Addr3 (IBSS/Mgmt)
/// - ToDS=1, FromDS=0: BSSID is Addr1 (STA to AP)
/// - ToDS=0, FromDS=1: BSSID is Addr2 (AP to STA)
/// - ToDS=1, FromDS=1: Not applicable for 3-address header (WDS uses 4
///   addresses) Returns None if the ToDS/FromDS combination is for WDS (which
///   needs 4 addresses).
pub fn get_bssid(header: &MacHeader3Addr) -> Option<MacAddr> {
    let fc = header.frame_control;
    match (fc.to_ds(), fc.from_ds()) {
        (false, false) => Some(header.addr3), // IBSS or Management frame
        (true, false) => Some(header.addr1),  // STA to AP
        (false, true) => Some(header.addr2),  // AP to STA
        (true, true) => None,                 // WDS frame, BSSID context is different
    }
}

#[cfg(test)]
mod tests {
    use zerocopy::U16;

    use super::*;
    use crate::ieee80211::{
        FrameControl, MacHeader3Addr, SequenceControl, data_subtype, frame_type,
    };

    fn create_header(
        fc_val: u16,
        addr1: [u8; 6],
        addr2: [u8; 6],
        addr3: [u8; 6],
    ) -> MacHeader3Addr {
        MacHeader3Addr {
            frame_control: FrameControl::new(fc_val),
            duration_id: U16::new(0),
            addr1: MacAddr::new(addr1),
            addr2: MacAddr::new(addr2),
            addr3: MacAddr::new(addr3),
            sequence_control: SequenceControl::new(0),
        }
    }

    #[test]
    fn test_address_interpretation() {
        let mac1 = [1; 6];
        let mac2 = [2; 6];
        let mac3 = [3; 6];

        // Case 1: IBSS / Management (ToDS=0, FromDS=0)
        // FC: Type=Data, Subtype=Data. ToDS=0, FromDS=0. (0x0008)
        let header_ibss = create_header(0x0008, mac1, mac2, mac3);
        assert_eq!(get_receiver_address(&header_ibss).bytes, mac1); // RA = Addr1
        assert_eq!(get_transmitter_address(&header_ibss).bytes, mac2); // TA = Addr2
        assert_eq!(get_destination_address(&header_ibss).bytes, mac1); // DA = Addr1
        assert_eq!(get_source_address(&header_ibss).bytes, mac2); // SA = Addr2
        assert_eq!(get_bssid(&header_ibss).unwrap().bytes, mac3); // BSSID = Addr3

        // Case 2: STA to AP (ToDS=1, FromDS=0)
        // FC: Type=Data, Subtype=Data. ToDS=1, FromDS=0. (0x0108)
        let header_to_ap = create_header(0x0108, mac1, mac2, mac3);
        assert_eq!(get_receiver_address(&header_to_ap).bytes, mac1); // RA = Addr1 (BSSID)
        assert_eq!(get_transmitter_address(&header_to_ap).bytes, mac2); // TA = Addr2 (SA)
        assert_eq!(get_destination_address(&header_to_ap).bytes, mac3); // DA = Addr3
        assert_eq!(get_source_address(&header_to_ap).bytes, mac2); // SA = Addr2
        assert_eq!(get_bssid(&header_to_ap).unwrap().bytes, mac1); // BSSID = Addr1

        // Case 3: AP to STA (ToDS=0, FromDS=1)
        // FC: Type=Data, Subtype=Data. ToDS=0, FromDS=1. (0x0208)
        let header_from_ap = create_header(0x0208, mac1, mac2, mac3);
        assert_eq!(get_receiver_address(&header_from_ap).bytes, mac1); // RA = Addr1 (DA)
        assert_eq!(get_transmitter_address(&header_from_ap).bytes, mac2); // TA = Addr2 (BSSID)
        assert_eq!(get_destination_address(&header_from_ap).bytes, mac1); // DA = Addr1
        assert_eq!(get_source_address(&header_from_ap).bytes, mac3); // SA = Addr3
        assert_eq!(get_bssid(&header_from_ap).unwrap().bytes, mac2); // BSSID = Addr2

        // Case 4: WDS (ToDS=1, FromDS=1) - BSSID interpretation is different for 3-addr
        // header FC: Type=Data, Subtype=Data. ToDS=1, FromDS=1. (0x0308)
        let header_wds = create_header(0x0308, mac1, mac2, mac3);
        assert!(get_bssid(&header_wds).is_none());
    }

    #[test]
    fn test_frame_control_to_string_basic() {
        let fc = FrameControl::new(0x0080); // Beacon: Mgmt, Beacon
        assert_eq!(frame_control_to_string(fc), "FC:0x0080 Mgmt,Beacon");

        let fc_data_qos_protected = FrameControl::new(0x4188); // Data, QoSData, ToDS, Protected
        assert_eq!(
            frame_control_to_string(fc_data_qos_protected),
            "FC:0x4188 Data,QoSData,ToDS,Protected"
        );
    }

    #[test]
    fn test_type_subtype_to_string() {
        assert_eq!(frame_type_to_string(frame_type::MANAGEMENT), "Mgmt");
        assert_eq!(frame_type_to_string(frame_type::DATA), "Data");
        assert_eq!(frame_type_to_string(3), "Type:3"); // Reserved type

        assert_eq!(management_subtype_to_string(management_subtype::BEACON), "Beacon");
        assert_eq!(management_subtype_to_string(management_subtype::PROBE_REQUEST), "ProbeReq");
        assert_eq!(management_subtype_to_string(15), "MgmtSubtype:15"); // Reserved subtype

        assert_eq!(data_subtype_to_string(data_subtype::DATA), "Data");
        assert_eq!(data_subtype_to_string(data_subtype::QOS_DATA), "QoSData");
        assert_eq!(data_subtype_to_string(15), "DataSubtype:15"); // Reserved subtype
    }

    #[test]
    fn test_is_beacon_frame() {
        let beacon_fc = FrameControl::new(0x0080); // Mgmt, Beacon
        assert!(is_beacon_frame(beacon_fc));
        let data_fc = FrameControl::new(0x0108); // Data, ToDS
        assert!(!is_beacon_frame(data_fc));
    }
}
