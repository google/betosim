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

// src/constants.rs

use std::{fmt, time::Duration};

use crate::types::CtecTechnology;

pub const CALL_RING_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_PLMN: &str = "310260";
pub const DEFAULT_OPERATOR_NAME_LONG: &str = "Android Virtual Operator";
pub const DEFAULT_OPERATOR_NAME_SHORT: &str = "Android";
pub const DEFAULT_MSISDN_PREFIX: &str = "15555211";
pub const DEFAULT_FALLBACK_MSISDN: &str = "15551234567";

/// Standard AT signal strength unknown value.
pub const CSQ_SIGNAL_UNKNOWN: u8 = 99;

/// List of CTEC technologies supported by this simulator.
pub const SUPPORTED_CTEC_TECHS: &[CtecTechnology] =
    &[CtecTechnology::Gsm, CtecTechnology::Wcdma, CtecTechnology::Lte];

/// Standard UICC File Identifiers (ISO/IEC 7816-4 §5.3 / 3GPP TS 51.011 / TS
/// 31.102).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum UiccFileId {
    // Master File & Application Dedicated Files
    MasterFile = 0x3F00,
    AdfDefault = 0x7FFF,

    // Dedicated Files
    Telecom = 0x7F10,

    // Elementary Files (Transparent)
    Imsi = 0x6F07,
    Iccid = 0x2FE2,
    ForbiddenPlmn = 0x6F7B,
    AdministrativeData = 0x6FAD,
    VoiceMailIndicatorCphs = 0x6F11,

    // Elementary Files (Linear Fixed)
    Msisdn = 0x6F40,
    MailboxDialingNumbers = 0x6FC7,
    FixedDialingNumbers = 0x6F3B,
    /// EF_ADN (Abbreviated Dialling Numbers) under DF_TELECOM in 2G SIM (3GPP
    /// TS 51.011 §10.5.1).
    AbbreviatedDialingNumbers = 0x6F3A,
    /// EF_ADN (Abbreviated Dialling Numbers) under DF_PHONEBOOK (0x5F3A) in
    /// 3G/USIM (3GPP TS 31.102 §4.4.2.3).
    AbbreviatedDialingNumbersUsim = 0x4F3A,
    MailboxIdentifier = 0x6FC9,
    MessageWaitingIndicationStatus = 0x6FCA,
    ShortMessages = 0x6F3C,
}

impl UiccFileId {
    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    pub const fn is_virtual_fallback(self) -> bool {
        matches!(
            self,
            Self::Iccid
                | Self::Imsi
                | Self::Msisdn
                | Self::ForbiddenPlmn
                | Self::MailboxDialingNumbers
                | Self::AdministrativeData
        )
    }

    pub fn is_virtual_fallback_id(file_id: u16) -> bool {
        Self::try_from(file_id).is_ok_and(Self::is_virtual_fallback)
    }

    /// Returns the canonical default record length for standard 3GPP
    /// linear-fixed files.
    pub const fn default_record_len(self) -> Option<usize> {
        match self {
            // EF_MSISDN: TS 31.102 §4.4.2.3, TS 51.011 §10.5.1
            Self::Msisdn => Some(28),
            // EF_FDN: TS 31.102 §4.4.2.4, TS 51.011 §10.5.2
            Self::FixedDialingNumbers => Some(28),
            // EF_MBDN: TS 31.102 §4.4.2.14
            Self::MailboxDialingNumbers => Some(38),
            // EF_ADN: TS 31.102 §4.4.2.3
            Self::AbbreviatedDialingNumbers | Self::AbbreviatedDialingNumbersUsim => Some(28),
            // EF_MBI: TS 31.102 §4.4.2.13
            Self::MailboxIdentifier => Some(5),
            // EF_MWIS: TS 31.102 §4.4.2.15
            Self::MessageWaitingIndicationStatus => Some(5),
            // EF_SMS: TS 31.102 §4.4.2.5
            Self::ShortMessages => Some(176),
            _ => None,
        }
    }

    /// Returns true if this file ID represents a known record-based EF.
    pub const fn is_record_based(self) -> bool {
        self.default_record_len().is_some()
    }

    /// Returns true if this Elementary File uses the 3GPP ADN record format.
    pub const fn is_adn(self) -> bool {
        matches!(
            self,
            Self::Msisdn
                | Self::MailboxDialingNumbers
                | Self::FixedDialingNumbers
                | Self::AbbreviatedDialingNumbers
                | Self::AbbreviatedDialingNumbersUsim
        )
    }

    /// Returns true if this file should be synchronized across DF_TELECOM and
    /// ADF_USIM.
    pub const fn is_dual_df_synced(self) -> bool {
        matches!(self, Self::Msisdn | Self::MailboxDialingNumbers | Self::FixedDialingNumbers)
    }
}

impl From<UiccFileId> for u16 {
    fn from(id: UiccFileId) -> Self {
        id as u16
    }
}

impl TryFrom<u16> for UiccFileId {
    type Error = ();

    fn try_from(val: u16) -> Result<Self, Self::Error> {
        match val {
            0x3F00 => Ok(Self::MasterFile),
            0x7FFF => Ok(Self::AdfDefault),
            0x7F10 => Ok(Self::Telecom),
            0x6F07 => Ok(Self::Imsi),
            0x2FE2 => Ok(Self::Iccid),
            0x6F7B => Ok(Self::ForbiddenPlmn),
            0x6F40 => Ok(Self::Msisdn),
            0x6FC7 => Ok(Self::MailboxDialingNumbers),
            0x6FAD => Ok(Self::AdministrativeData),
            0x6F11 => Ok(Self::VoiceMailIndicatorCphs),
            0x6F3B => Ok(Self::FixedDialingNumbers),
            0x6F3A => Ok(Self::AbbreviatedDialingNumbers),
            0x4F3A => Ok(Self::AbbreviatedDialingNumbersUsim),
            0x6FC9 => Ok(Self::MailboxIdentifier),
            0x6FCA => Ok(Self::MessageWaitingIndicationStatus),
            0x6F3C => Ok(Self::ShortMessages),
            _ => Err(()),
        }
    }
}

impl fmt::Display for UiccFileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_u16())
    }
}

impl fmt::UpperHex for UiccFileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::UpperHex::fmt(&self.as_u16(), f)
    }
}

impl fmt::LowerHex for UiccFileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.as_u16(), f)
    }
}

// Abbreviated Dialing Numbers (ADN) Record Layout (TS 31.102 §4.4.2.3 / TS
// 51.011 §10.5.1)
pub const ADN_ALPHA_IDENTIFIER_LEN: usize = 14;
pub const ADN_DIALING_NUMBER_LEN: usize = 10;
pub const ADN_FOOTER_LEN: usize = 14;
pub const ADN_CAPABILITY_EXT_BYTES: [u8; 2] = [0xFF, 0xFF];

/// Default number of linear-fixed records provisioned for EF_MBDN.
/// TS 31.102 §4.4.2.3 specifies 4 default mailbox types: Voice, Fax, Electronic
/// Mail, and Other.
pub const DEFAULT_MBDN_RECORD_COUNT: usize = 4;

/// Default number of linear-fixed records provisioned for EF_FDN (Fixed
/// Dialling Numbers).
pub const DEFAULT_FDN_RECORD_COUNT: usize = 10;
pub const TAG_DF_NAME: u8 = 0x84;

// ISO 7816-4 APDU Status Words (SW)
pub const SW_SUCCESS: u16 = 0x9000;
pub const SW_WRONG_LENGTH: u16 = 0x6700;
pub const SW_FILE_NOT_FOUND: u16 = 0x6A82;
pub const SW_INCORRECT_PARAMS: u16 = 0x6A86;
pub const SW_REFERENCED_DATA_NOT_FOUND: u16 = 0x6A88;
pub const SW_CLASS_NOT_SUPPORTED: u16 = 0x6E00;
pub const SW_INS_NOT_SUPPORTED: u16 = 0x6D00;
pub const SW_TECHNICAL_PROBLEM: u16 = 0x6F00;
pub const SW_NO_CHANNEL_AVAILABLE: u16 = 0x6A81;

// GET_RESPONSE (0xC0) status words
pub const SW_BYTES_REMAINING_PREFIX: u8 = 0x61;
