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

//! Provides utility functions for working with LLC and SNAP headers.

use crate::llc::{LlcHeader, control_field, sap};
use std::fmt::Write;

/// Converts an LLC Service Access Point (SAP) value to a human-readable string.
///
/// If the SAP value is unknown, it returns the hex representation of the value.
pub fn sap_to_string(sap_val: u8) -> String {
    match sap_val {
        sap::NULL => "NullLSAP".to_string(),
        sap::ILMI => "ILMI".to_string(),
        sap::GLOBAL => "GlobalDSAP".to_string(),
        sap::SNAP => "SNAP".to_string(),
        sap::STP => "STP/BPDU".to_string(),
        _ => format!("SAP:0x{:02X}", sap_val),
    }
}

/// Converts an LLC Control Field value (for 1-byte control fields) to a human-readable string.
///
/// If the control field value is unknown, it returns the hex representation of the value.
pub fn control_field_to_string(control_val: u8) -> String {
    match control_val {
        control_field::UI => "UI".to_string(),
        control_field::XID => "XID".to_string(),
        control_field::TEST => "TEST".to_string(),
        // Add other control field types if needed
        _ => format!("Ctrl:0x{:02X}", control_val),
    }
}

/// Checks if the LLC header indicates that a SNAP header follows.
///
/// This is true if DSAP and SSAP are both `0xAA` (SNAP SAP).
pub fn is_snap_llc(llc_header: &LlcHeader) -> bool {
    llc_header.dsap == sap::SNAP && llc_header.ssap == sap::SNAP
}

/// Formats an `LlcHeader` into a human-readable string.
pub fn format_llc_header(header: &LlcHeader) -> String {
    let mut s = String::new();
    write!(
        s,
        "LLC DSAP:{} SSAP:{} Ctrl:{}",
        sap_to_string(header.dsap),
        sap_to_string(header.ssap),
        control_field_to_string(header.control)
    )
    .unwrap();
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llc::LlcHeader;

    #[test]
    fn test_sap_to_string_known() {
        assert_eq!(sap_to_string(sap::SNAP), "SNAP");
        assert_eq!(sap_to_string(sap::STP), "STP/BPDU");
        assert_eq!(sap_to_string(sap::NULL), "NullLSAP");
    }

    #[test]
    fn test_sap_to_string_unknown() {
        assert_eq!(sap_to_string(0xFE), "SAP:0xFE");
    }

    #[test]
    fn test_control_field_to_string_known() {
        assert_eq!(control_field_to_string(control_field::UI), "UI");
        assert_eq!(control_field_to_string(control_field::XID), "XID");
    }

    #[test]
    fn test_control_field_to_string_unknown() {
        assert_eq!(control_field_to_string(0xFF), "Ctrl:0xFF");
    }

    #[test]
    fn test_is_snap_llc() {
        let snap_llc = LlcHeader::new(sap::SNAP, sap::SNAP, control_field::UI);
        assert!(is_snap_llc(&snap_llc));

        let non_snap_llc_dsap = LlcHeader::new(sap::STP, sap::SNAP, control_field::UI);
        assert!(!is_snap_llc(&non_snap_llc_dsap));

        let non_snap_llc_ssap = LlcHeader::new(sap::SNAP, sap::STP, control_field::UI);
        assert!(!is_snap_llc(&non_snap_llc_ssap));
    }

    #[test]
    fn test_format_llc_header() {
        let header = LlcHeader::new(sap::SNAP, sap::SNAP, control_field::UI);
        assert_eq!(format_llc_header(&header), "LLC DSAP:SNAP SSAP:SNAP Ctrl:UI");
        let header_stp = LlcHeader::new(sap::STP, sap::STP, control_field::UI);
        assert_eq!(format_llc_header(&header_stp), "LLC DSAP:STP/BPDU SSAP:STP/BPDU Ctrl:UI");
    }
}
