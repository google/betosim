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

use std::{
    collections::{BTreeMap, HashMap},
    iter::once,
    ops::RangeInclusive,
};

use modem_rs_derive::CommandParser;
use tracing::{debug, info};

use crate::{
    CardState, SmsStore, apdu,
    config::{FileSystem, OverwritePolicy, ProfileMetadata, SimFile, SimProfile},
    constants::*,
    types::{
        AdnRecord, ApduData, ApplicationId, CdmaRoamingPreference, CdmaSubscriptionSource,
        CmeError, CmsError, DEFAULT_BARRING_PASSWORD, DEFAULT_PIN, DEFAULT_PIN2, DEFAULT_PUK2,
        ExecutionResult, Facility, FacilityLockMode, Parsable, PhoneNumber, PinString, PinType,
        Plmn, QuotedString, ServiceClass, SimSmsMessage,
    },
};

#[derive(Debug, PartialEq, Clone, Copy)]
enum SimAccessType {
    Csim,
    Cgla,
}

/// SIM service AT commands.
#[derive(Debug, PartialEq, Clone, CommandParser)]
pub enum SimCommand<'a> {
    #[command(tag = "AT+CPIN?")]
    GetSimStatus,
    #[command(tag = "AT+CPINR=")]
    QueryPinRetries(PinType),
    #[command(tag = "AT+CPIN=")]
    EnterPin(PinString<'a>, Option<PinString<'a>>),
    #[command(tag = "AT+CRSM=")]
    SimIo {
        command: apdu::Instruction,
        file_id: u16,
        p1: u8,
        p2: u8,
        p3: u8,
        data: Option<ApduData<'a>>,
        path: Option<ApduData<'a>>,
    },
    #[command(tag = "AT+CIMI")]
    GetImsi,
    #[command(tag = "AT+CICCID")]
    GetIccid,
    #[command(tag = "AT+CCHO=")]
    OpenLogicalChannel(ApplicationId<'a>),
    #[command(tag = "AT+CCHC=")]
    CloseLogicalChannel(u8),
    #[command(tag = "AT+CGLA=")]
    TransmitLogicalChannel(u8, u8, ApduData<'a>),
    #[command(tag = "AT+CPWD=")]
    ChangePassword(Facility, QuotedString<'a>, QuotedString<'a>),
    /// 3GPP TS 27.007 §7.4: Selects facility lock state or queries status.
    #[command(tag = "AT+CLCK=")]
    SetFacilityLock(Facility, FacilityLockMode, Option<QuotedString<'a>>, Option<ServiceClass>),
    /// 3GPP2 C.S0023: Set CDMA subscription source
    #[command(tag = "AT+CCSS=")]
    SetCdmaSubscriptionSource(CdmaSubscriptionSource),
    /// 3GPP2 C.S0023: Query CDMA subscription source
    #[command(tag = "AT+CCSS?")]
    QueryCdmaSubscriptionSource,
    /// 3GPP2 C.S0023: Set CDMA roaming preference
    #[command(tag = "AT+WRMP=")]
    SetCdmaRoamingPreference(CdmaRoamingPreference),
    /// 3GPP2 C.S0023: Query CDMA roaming preference
    #[command(tag = "AT+WRMP?")]
    QueryCdmaRoamingPreference,
    /// Generic SIM access (+CSIM)
    #[command(tag = "AT+CSIM=")]
    GenericSimAccess(u32, ApduData<'a>),
    #[command(tag = "AT+MBAU=")]
    SimAuthentication(ApduData<'a>),
    /// SIM authentication (Vendor caret version)
    #[command(tag = "AT^MBAU=")]
    SimAuthenticationVendor(ApduData<'a>),
    #[command(tag = "AT+CEID")]
    GetEid,
    #[command(tag = "AT+CATR")]
    GetAtr,
}

const VALID_PIN_LEN: RangeInclusive<usize> = 4..=8;
const PUK_LEN: usize = 8;
const DEFAULT_PIN_RETRIES: u32 = 3;
const DEFAULT_PUK_RETRIES: u32 = 10;

const DEFAULT_PUK: &str = "12345678";

const DEFAULT_FALLBACK_IMSI: &str = "310260123456789";
const DEFAULT_FALLBACK_ICCID: &str = "89012608640220133897";
const EF_FPLMN_DATA_FALLBACK: &[u8] = &[0xFF; 12];
/// Standard FCP template payload for STATUS / SIM state responses in hex string
/// format.
const STATUS_FCP_HEX: &str = "62338202782183023F00A50C80016187010183040007DBF08A01058B062F0601020002C60C90016083010183010A83010D8102FFFF";
/// Pre-decoded byte representation of `STATUS_FCP_HEX` for compile-time safety
/// in `generate_df_fcp`.
const STATUS_FCP_BYTES: [u8; 53] = [
    0x62, 0x33, 0x82, 0x02, 0x78, 0x21, 0x83, 0x02, 0x3F, 0x00, 0xA5, 0x0C, 0x80, 0x01, 0x61, 0x87,
    0x01, 0x01, 0x83, 0x04, 0x00, 0x07, 0xDB, 0xF0, 0x8A, 0x01, 0x05, 0x8B, 0x06, 0x2F, 0x06, 0x01,
    0x02, 0x00, 0x02, 0xC6, 0x0C, 0x90, 0x01, 0x60, 0x83, 0x01, 0x01, 0x83, 0x01, 0x0A, 0x83, 0x01,
    0x0D, 0x81, 0x02, 0xFF, 0xFF,
];

/// 3GPP TS 51.011 §9.3 Access Condition Levels.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum AccessLevel {
    #[default]
    Always = 0x0,
    Pin1 = 0x1,
    Pin2 = 0x2,
    Never = 0xF,
}

/// 3GPP TS 51.011 §9.2.1 Access Conditions (Bytes 9–12).
///
/// Encodes 4-bit nibbles:
/// - Byte 9:  `[read: 4 bits | update: 4 bits]`
/// - Byte 10: `[increase: 4 bits | rfu: 4 bits]`
/// - Byte 11: `[rehabilitate: 4 bits | invalidate: 4 bits]`
/// - Byte 12: RFU / Administrative management (`0x00`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AccessConditions {
    pub read: AccessLevel,
    pub update: AccessLevel,
    pub increase: AccessLevel,
    pub rehabilitate: AccessLevel,
    pub invalidate: AccessLevel,
}

impl AccessConditions {
    pub fn to_bytes(self) -> [u8; 3] {
        [
            ((self.read as u8) << 4) | (self.update as u8),
            (self.increase as u8) << 4,
            ((self.rehabilitate as u8) << 4) | (self.invalidate as u8),
        ]
    }
}

/// 3GPP TS 51.011 §9.2.1 Byte 13: Elementary File Status.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum FileStatus {
    #[default]
    Valid = 0x00,
    Invalidated = 0x01,
}

/// 3GPP TS 51.011 §9.2.1 Byte 13: DF Characteristics (Clock Stop mode and
/// preference).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ClockStopPreference {
    #[default]
    NotAllowed = 0x00,
    NoPreference = 0x04,
    HighLevel = 0x05,
    LowLevel = 0x06,
}

/// 3GPP TS 51.011 §9.2.1 Bytes 16–22: CHV and Administrative Status for DF/MF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChvStatus {
    pub num_chvs: u8,
    pub chv1_status: u8,
    pub unblock_chv1_status: u8,
    pub chv2_status: u8,
    pub unblock_chv2_status: u8,
}

impl ChvStatus {
    pub fn to_bytes(self) -> [u8; 7] {
        [
            self.num_chvs,
            self.chv1_status,
            self.unblock_chv1_status,
            self.chv2_status,
            self.unblock_chv2_status,
            0x00, // Byte 21: RFU
            0x00, // Byte 22: RFU / Administrative data
        ]
    }
}

/// SIM File Type in 3GPP TS 51.011 §9.2.1 response header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SimFileType {
    Master = 0x01,
    Dedicated = 0x02,
    Elementary = 0x04,
}

/// Elementary File (EF) structure in 3GPP TS 51.011 §9.2.1 byte 14.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ElementaryFileStructure {
    Transparent = 0x00,
    LinearFixed = 0x01,
}

/// 3GPP TS 51.011 §9.2.1 response header for Elementary Files (EF).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementaryFileResponseHeader {
    pub file_size: u16,
    pub file_id: u16,
    pub file_type: SimFileType,
    pub access_conditions: AccessConditions,
    pub file_status: FileStatus,
    pub structure: ElementaryFileStructure,
    pub record_len: Option<u8>,
}

impl ElementaryFileResponseHeader {
    pub fn new(file_id: u16, file_size: usize, record_len: Option<usize>) -> Self {
        let (structure, r_len) = match record_len {
            Some(rlen) => (ElementaryFileStructure::LinearFixed, Some(rlen as u8)),
            None => (ElementaryFileStructure::Transparent, None),
        };
        Self {
            file_size: file_size as u16,
            file_id,
            file_type: SimFileType::Elementary,
            access_conditions: AccessConditions::default(),
            file_status: FileStatus::Valid,
            structure,
            record_len: r_len,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let following_data: &[u8] = match self.structure {
            ElementaryFileStructure::LinearFixed => {
                &[0x02, self.structure as u8, self.record_len.unwrap_or(0)]
            }
            ElementaryFileStructure::Transparent => &[0x01, self.structure as u8],
        };

        [0x00, 0x00] // Bytes 1-2: RFU
            .into_iter()
            .chain(self.file_size.to_be_bytes()) // Bytes 3-4: File size
            .chain(self.file_id.to_be_bytes()) // Bytes 5-6: File ID
            .chain(once(self.file_type as u8)) // Byte 7: File type
            .chain(once(0x00)) // Byte 8: Cyclic increase pointer / RFU
            .chain(self.access_conditions.to_bytes()) // Bytes 9-11: Access conditions
            .chain(once(self.file_status as u8)) // Byte 12: File status
            .chain(following_data.iter().copied()) // Bytes 13+: Following data & structure
            .collect()
    }
}

/// 3GPP TS 51.011 §9.2.1 response header for Dedicated Files (DF) / Master File
/// (MF).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedicatedFileResponseHeader {
    pub memory_allocated: u16,
    pub file_id: u16,
    pub file_type: SimFileType,
    pub clock_stop: ClockStopPreference,
    pub num_df_children: u8,
    pub num_ef_children: u8,
    pub chv_status: ChvStatus,
}

impl DedicatedFileResponseHeader {
    pub fn new(file_id: u16, is_mf: bool, num_df_children: usize, num_ef_children: usize) -> Self {
        Self {
            memory_allocated: 0,
            file_id,
            file_type: if is_mf { SimFileType::Master } else { SimFileType::Dedicated },
            clock_stop: ClockStopPreference::default(),
            num_df_children: num_df_children as u8,
            num_ef_children: num_ef_children as u8,
            chv_status: ChvStatus::default(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        [0x00, 0x00] // Bytes 1-2: RFU
            .into_iter()
            .chain(self.memory_allocated.to_be_bytes()) // Bytes 3-4: Memory allocated
            .chain(self.file_id.to_be_bytes()) // Bytes 5-6: File ID
            .chain(once(self.file_type as u8)) // Byte 7: File type
            .chain([0x00; 5]) // Bytes 8-12: RFU
            .chain(once(self.clock_stop as u8)) // Byte 13: DF characteristics
            .chain(once(self.num_df_children)) // Byte 14: Direct child DFs
            .chain(once(self.num_ef_children)) // Byte 15: Direct child EFs
            .chain(self.chv_status.to_bytes()) // Bytes 16-22: CHV & admin status
            .collect()
    }
}

/// Strongly-typed container for SIM file response headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimResponseHeader {
    Elementary(ElementaryFileResponseHeader),
    Dedicated(DedicatedFileResponseHeader),
}

impl SimResponseHeader {
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Elementary(ef) => ef.to_bytes(),
            Self::Dedicated(df) => df.to_bytes(),
        }
    }
}

// ISO 7816-4 SIM APDU Response Constants
const RESP_SUCCESS: SimResponse = SimResponse::RestrictedSimAccess { sw: SW_SUCCESS, data: None };
const RESP_WRONG_LENGTH: SimResponse =
    SimResponse::RestrictedSimAccess { sw: SW_WRONG_LENGTH, data: None };
const RESP_FILE_NOT_FOUND: SimResponse =
    SimResponse::RestrictedSimAccess { sw: SW_FILE_NOT_FOUND, data: None };
const RESP_INCORRECT_PARAMS: SimResponse =
    SimResponse::RestrictedSimAccess { sw: SW_INCORRECT_PARAMS, data: None };
const RESP_REFERENCED_DATA_NOT_FOUND: SimResponse =
    SimResponse::RestrictedSimAccess { sw: SW_REFERENCED_DATA_NOT_FOUND, data: None };

const P2_INVALID_SELECT: u8 = 0xF0;

// APDU Status Words (SW)

// Hex Data Templates

// SIM Authentication mock challenges and responses
const AUTH_CHALLENGE_1: &str = "2713AB0BA8E8E7D8F1D74545BA03F563";
const AUTH_RESPONSE_1: &str = "^MBAU: 0,8F2980FC3872FF89,E9620240\r\n";
const AUTH_CHALLENGE_2: &str = "C3718EC16B3C2A66F8A7200A64069F04";
const AUTH_RESPONSE_2: &str = "^MBAU: 0,CFDA6C980502DA48,F7E53577\r\n";
const AUTH_CHALLENGE_3: &str = "11111111111111111111111111111111";
const AUTH_RESPONSE_3: &str = "^MBAU: 0,0000000000000000,00000000\r\n";
const AUTH_CHALLENGE_4: &str = "11111111111111111111111111111111,12351417161900001130131215141716";
const AUTH_RESPONSE_4: &str = "^MBAU: 0,111013121514171619181B1A1D1C1F1E,1013121514171619181B1A1D1C1F1E11,13121514171619181B1A1D1C1F1E1110\r\n";

// Represents the state of the SIM card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimState {
    Absent,
    Ready,
    PinRequired,
    PukRequired,
    PermBlocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimResponse {
    PinStatus(RequiredPin),
    RestrictedSimAccess { sw: u16, data: Option<String> },
    Imsi(String),
    Iccid(String),
    OpenLogicalChannel(u8),
    CloseLogicalChannel,
    GenericLogicalChannelAccess(String),
    GenericSimAccess(String),
    CdmaSubscriptionSource(CdmaSubscriptionSource),
    CdmaRoamingPreference(CdmaRoamingPreference),
    SimAuthentication(String),
    FacilityLockStatus(u8),
    PinRemainingAttempts { pin_type: PinType, retries: u32, default_retries: u32 },
    Eid(String),
    Atr(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredPin {
    None,
    SimPin,
    SimPuk,
}

impl std::fmt::Display for SimResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimResponse::PinStatus(status) => match status {
                RequiredPin::None => write!(f, "+CPIN: READY\r\n"),
                RequiredPin::SimPin => write!(f, "+CPIN: SIM PIN\r\n"),
                RequiredPin::SimPuk => write!(f, "+CPIN: SIM PUK\r\n"),
            },
            SimResponse::RestrictedSimAccess { sw, data } => {
                let sw1 = (sw >> 8) as u8;
                let sw2 = (sw & 0xFF) as u8;
                if let Some(d) = data {
                    write!(f, "+CRSM: {sw1},{sw2},{d}\r\n")
                } else {
                    write!(f, "+CRSM: {sw1},{sw2}\r\n")
                }
            }
            SimResponse::Imsi(imsi) => write!(f, "{imsi}\r\n"),
            SimResponse::Iccid(iccid) => write!(f, "{iccid}\r\n"),
            SimResponse::OpenLogicalChannel(channel_id) => write!(f, "{channel_id}\r\n"),
            SimResponse::CloseLogicalChannel => write!(f, "+CCHC\r\n"),
            SimResponse::GenericLogicalChannelAccess(resp) => write!(f, "+CGLA: {resp}\r\n"),
            SimResponse::GenericSimAccess(resp) => write!(f, "+CSIM: {resp}\r\n"),
            SimResponse::CdmaSubscriptionSource(source) => write!(f, "+CCSS: {source}\r\n"),
            SimResponse::CdmaRoamingPreference(pref) => write!(f, "+WRMP: {pref}\r\n"),
            SimResponse::SimAuthentication(resp) => write!(f, "{resp}"),
            SimResponse::FacilityLockStatus(status) => write!(f, "+CLCK: {status}\r\n"),
            SimResponse::PinRemainingAttempts { pin_type, retries, default_retries } => {
                write!(f, "+CPINR: \"{}\",{retries},{default_retries}\r\n", pin_type.as_str())
            }
            SimResponse::Eid(eid) => write!(f, "+CEID: {eid}\r\n"),
            SimResponse::Atr(atr) => write!(f, "+CATR: {atr}\r\n"),
        }
    }
}

type SimResult = Result<Option<SimResponse>, ExecutionResult>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinVerifyError {
    IncorrectPassword,
    Blocked,
}

impl PinVerifyError {
    pub const fn into_cme_error(self, blocked_error: CmeError) -> CmeError {
        match self {
            Self::Blocked => blocked_error,
            Self::IncorrectPassword => CmeError::IncorrectPassword,
        }
    }
}

/// Encapsulates credentials (PIN and PUK), remaining attempts, and maximum
/// attempts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinEntry {
    pub pin: String,
    pub puk: String,
    pub pin_retries: u32,
    pub puk_retries: u32,
    pub max_pin_retries: u32,
    pub max_puk_retries: u32,
}

impl PinEntry {
    pub fn new(pin: &str, puk: &str, max_pin_retries: u32, max_puk_retries: u32) -> Self {
        Self {
            pin: pin.to_string(),
            puk: puk.to_string(),
            pin_retries: max_pin_retries,
            puk_retries: max_puk_retries,
            max_pin_retries,
            max_puk_retries,
        }
    }

    pub fn from_profile(
        pin: &str,
        puk: &str,
        pin_retries: Option<u32>,
        puk_retries: Option<u32>,
        default_pin: &str,
        default_puk: &str,
    ) -> Self {
        let pin = if !pin.is_empty() { pin } else { default_pin };
        let puk = if !puk.is_empty() { puk } else { default_puk };
        Self::new(
            pin,
            puk,
            pin_retries.unwrap_or(DEFAULT_PIN_RETRIES),
            puk_retries.unwrap_or(DEFAULT_PUK_RETRIES),
        )
    }

    pub fn is_blocked(&self) -> bool {
        self.pin_retries == 0
    }

    pub fn is_puk_blocked(&self) -> bool {
        self.puk_retries == 0
    }

    pub fn decrement_pin_retries(&mut self) {
        self.pin_retries = self.pin_retries.saturating_sub(1);
    }

    pub fn decrement_puk_retries(&mut self) {
        self.puk_retries = self.puk_retries.saturating_sub(1);
    }

    pub fn reset_pin_retries(&mut self) {
        self.pin_retries = self.max_pin_retries;
    }

    pub fn reset_puk_retries(&mut self) {
        self.puk_retries = self.max_puk_retries;
    }

    pub fn pin_attempts(&self) -> (u32, u32) {
        (self.pin_retries, self.max_pin_retries)
    }

    pub fn puk_attempts(&self) -> (u32, u32) {
        (self.puk_retries, self.max_puk_retries)
    }

    /// Verifies candidate PIN. On match resets retries; on mismatch decrements
    /// retries.
    pub fn verify_pin(&mut self, candidate: &str) -> Result<(), PinVerifyError> {
        if self.is_blocked() {
            return Err(PinVerifyError::Blocked);
        }
        if !candidate.is_ascii() || !VALID_PIN_LEN.contains(&candidate.len()) {
            return Err(PinVerifyError::IncorrectPassword);
        }
        if candidate == self.pin {
            self.reset_pin_retries();
            Ok(())
        } else {
            self.decrement_pin_retries();
            if self.is_blocked() {
                Err(PinVerifyError::Blocked)
            } else {
                Err(PinVerifyError::IncorrectPassword)
            }
        }
    }

    /// Changes the PIN after verifying the old PIN.
    pub fn change_pin(
        &mut self,
        old_password: &str,
        new_password: &str,
    ) -> Result<(), PinVerifyError> {
        if self.is_blocked() {
            return Err(PinVerifyError::Blocked);
        }
        if !new_password.is_ascii() || !VALID_PIN_LEN.contains(&new_password.len()) {
            return Err(PinVerifyError::IncorrectPassword);
        }
        self.verify_pin(old_password)?;
        self.pin = new_password.to_string();
        Ok(())
    }

    /// Unblocks the PIN using the PUK code and sets a new PIN.
    pub fn unblock_with_puk(&mut self, puk_candidate: &str, new_pin: &str) -> Result<(), CmeError> {
        if !puk_candidate.is_ascii()
            || puk_candidate.len() != PUK_LEN
            || !new_pin.is_ascii()
            || !VALID_PIN_LEN.contains(&new_pin.len())
        {
            return Err(CmeError::IncorrectPassword);
        }
        if puk_candidate == self.puk {
            self.pin = new_pin.to_string();
            self.reset_pin_retries();
            self.reset_puk_retries();
            Ok(())
        } else {
            self.decrement_puk_retries();
            Err(CmeError::IncorrectPassword)
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct LogicalChannel {
    open: bool,
    selected_aid: Option<String>,
    selected_file: Option<u16>,
    response_buffer: Vec<u8>,
}

impl LogicalChannel {
    fn reset(&mut self, clear_buffer: bool) {
        self.open = false;
        self.selected_aid = None;
        self.selected_file = None;
        if clear_buffer {
            self.response_buffer.clear();
        }
    }
}

fn default_channels() -> [LogicalChannel; 4] {
    [
        LogicalChannel { open: true, ..Default::default() },
        LogicalChannel::default(),
        LogicalChannel::default(),
        LogicalChannel::default(),
    ]
}

// Holds all state related to the SIM card.
pub struct SimService {
    provisioned: bool,
    state: SimState,
    pin_enabled: bool,
    pin1: PinEntry,
    pin2: PinEntry,
    call_barring_locks: HashMap<Facility, bool>,
    barring_password: PinEntry,
    fdn_enabled: bool,
    fs: FileSystem,
    sms_messages: BTreeMap<u8, SimSmsMessage>,
    channels: [LogicalChannel; 4],
    cdma_subscription_source: CdmaSubscriptionSource,
    cdma_roaming_preference: CdmaRoamingPreference,
    adfs: Vec<crate::config::ApplicationDedicatedFile>,
    eid: Option<String>,
    atr: Option<String>,
}

impl Default for SimService {
    fn default() -> Self {
        Self {
            provisioned: false,
            state: SimState::Absent,
            pin_enabled: false,
            pin1: PinEntry::new(DEFAULT_PIN, DEFAULT_PUK, DEFAULT_PIN_RETRIES, DEFAULT_PUK_RETRIES),
            pin2: PinEntry::new(
                DEFAULT_PIN2,
                DEFAULT_PUK2,
                DEFAULT_PIN_RETRIES,
                DEFAULT_PUK_RETRIES,
            ),
            call_barring_locks: HashMap::new(),
            barring_password: PinEntry::new(DEFAULT_BARRING_PASSWORD, "", DEFAULT_PIN_RETRIES, 0),
            fdn_enabled: false,
            fs: FileSystem::default(),
            sms_messages: BTreeMap::new(),
            // Channel 0 is the basic channel and is always open by default.
            channels: default_channels(),
            cdma_subscription_source: CdmaSubscriptionSource::default(),
            cdma_roaming_preference: CdmaRoamingPreference::default(),
            adfs: Vec::new(),
            eid: None,
            atr: None,
        }
    }
}

impl SimService {
    /// Creates a new unloaded SimService.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new SimService with the given profile loaded.
    pub fn from_profile(profile: &SimProfile) -> Self {
        let mut service = Self::new();
        service.load_profile(profile);
        service
    }

    /// Returns true if a profile is currently provisioned in this SIM slot.
    pub(crate) fn is_provisioned(&self) -> bool {
        self.provisioned
    }

    /// Returns the CME error corresponding to a non-ready SIM lifecycle state,
    /// or None if the SIM is ready for normal operation.
    pub(crate) fn gating_error(&self) -> Option<CmeError> {
        if !self.provisioned {
            return Some(CmeError::SimNotInserted);
        }
        match self.state {
            SimState::Absent => Some(CmeError::SimNotInserted),
            SimState::PinRequired => Some(CmeError::SimPinRequired),
            SimState::PukRequired => Some(CmeError::SimPukRequired),
            SimState::PermBlocked => Some(CmeError::SimFailure),
            SimState::Ready => None,
        }
    }

    /// Unprovisions and completely wipes the SIM card, restoring to default.
    fn clear_profile(&mut self) {
        *self = Self::default();
    }

    /// Loads a SIM profile configuration into the service, rebuilding the
    /// filesystem and resetting credentials and logical channels.
    pub fn load_profile(&mut self, profile: &SimProfile) {
        self.provisioned = true;
        self.state = match (profile.pin_profile.puk1_retries, profile.pin_profile.state) {
            (Some(0), _) | (_, crate::config::PinState::PermBlocked) => SimState::PermBlocked,
            (_, crate::config::PinState::EnabledNotVerified) => SimState::PinRequired,
            (_, crate::config::PinState::Blocked) => SimState::PukRequired,
            _ => SimState::Ready,
        };
        self.pin_enabled = matches!(
            profile.pin_profile.state,
            crate::config::PinState::EnabledNotVerified | crate::config::PinState::EnabledVerified
        );
        let pp = &profile.pin_profile;
        self.pin1 = PinEntry::from_profile(
            &pp.pin1,
            &pp.puk1,
            pp.pin1_retries,
            pp.puk1_retries,
            DEFAULT_PIN,
            DEFAULT_PUK,
        );
        self.pin2 = PinEntry::from_profile(
            &pp.pin2,
            &pp.puk2,
            pp.pin2_retries,
            pp.puk2_retries,
            DEFAULT_PIN2,
            DEFAULT_PUK2,
        );
        self.call_barring_locks.clear();
        self.barring_password = PinEntry::new(DEFAULT_BARRING_PASSWORD, "", DEFAULT_PIN_RETRIES, 0);
        self.fdn_enabled = false;
        self.sms_messages.clear();
        self.fs = profile.sim_io.file_system.clone();
        self.channels = default_channels();
        self.adfs = profile.adfs.clone();
        self.eid = profile.eid.clone();
        self.atr = profile.atr.clone();

        let iccid = if profile.iccid.is_empty() { DEFAULT_FALLBACK_ICCID } else { &profile.iccid };
        let iccid_swapped = crate::pdu::bcd::string_to_bcd(iccid);

        let imsi = if profile.imsi.is_empty() { DEFAULT_FALLBACK_IMSI } else { &profile.imsi };
        let imsi_encoded = crate::pdu::bcd::encode_imsi(imsi);

        let requested_msisdn = profile.msisdn.as_ref();
        self.fs.normalize_record_lengths();
        let existing_msisdn_ef = self.fs.find_ef_in_telecom_or_usim(UiccFileId::Msisdn);
        let msisdn_record_len =
            existing_msisdn_ef.and_then(|ef| ef.record_len()).unwrap_or_else(|| {
                UiccFileId::Msisdn.default_record_len().expect("file id is record based")
            });
        let msisdn_data = requested_msisdn
            .map(|p| AdnRecord::encode_from_number(p, msisdn_record_len))
            .or_else(|| existing_msisdn_ef.map(|ef| ef.data.clone()))
            .unwrap_or_else(|| vec![0xFF; msisdn_record_len]);
        let fplmn_data = EF_FPLMN_DATA_FALLBACK.to_vec();

        self.fs.ensure_ef_present(UiccFileId::Iccid, None, iccid_swapped, OverwritePolicy::Always);
        if let Some(encoded) = imsi_encoded {
            self.fs.ensure_ef_present(UiccFileId::Imsi, None, encoded, OverwritePolicy::Always);
        }
        self.fs.ensure_ef_present_in_telecom_and_usim(
            UiccFileId::Msisdn,
            Some(msisdn_record_len),
            msisdn_data,
            OverwritePolicy::IfUninitialized,
        );

        let mut ensure_dialing_numbers = |file_id: UiccFileId, default_count: usize| {
            let existing_ef = self.fs.find_ef_in_telecom_or_usim(file_id);
            let record_len = existing_ef
                .and_then(|ef| ef.record_len())
                .unwrap_or_else(|| file_id.default_record_len().expect("file id is record based"));
            let target_len = default_count * record_len;
            let mut data =
                existing_ef.map(|ef| ef.data.clone()).unwrap_or_else(|| vec![0xFF; target_len]);
            if data.len() < target_len {
                data.resize(target_len, 0xFF);
            }
            self.fs.ensure_ef_present_in_telecom_and_usim(
                file_id,
                Some(record_len),
                data,
                OverwritePolicy::IfUninitialized,
            );
        };

        ensure_dialing_numbers(UiccFileId::MailboxDialingNumbers, DEFAULT_MBDN_RECORD_COUNT);
        ensure_dialing_numbers(UiccFileId::FixedDialingNumbers, DEFAULT_FDN_RECORD_COUNT);

        self.fs.ensure_ef_present(
            UiccFileId::ForbiddenPlmn,
            None,
            fplmn_data,
            OverwritePolicy::Never,
        );
    }

    /// Inserts a SIM card by applying the provided profile. Fails if a SIM is
    /// already provisioned.
    pub(crate) fn insert_sim(&mut self, profile: &SimProfile) -> bool {
        if self.is_provisioned() {
            return false;
        }
        self.load_profile(profile);
        true
    }

    /// Removes and unprovisions the SIM card, clearing the filesystem and
    /// credentials.
    pub(crate) fn remove_sim(&mut self) -> bool {
        let was_provisioned = self.is_provisioned();
        self.clear_profile();
        was_provisioned
    }

    /// Returns the home PLMN (MCC + MNC) derived from the active SIM's EF_IMSI,
    /// or `None` if the SIM is absent or has no valid IMSI.
    pub(crate) fn home_plmn(&self) -> Option<Plmn> {
        self.get_imsi().and_then(|imsi| Plmn::from_imsi(&imsi, None))
    }

    /// Returns summary metadata for the active SIM profile, or `None` if the
    /// SIM is absent.
    pub(crate) fn get_profile_metadata(&self) -> Option<ProfileMetadata> {
        if !self.is_present() {
            return None;
        }
        Some(ProfileMetadata {
            iccid: self.get_iccid().unwrap_or_default(),
            imsi: self.get_imsi().unwrap_or_default(),
            msisdn: self.get_msisdn(),
            home_plmn: self.home_plmn(),
            eid: self.eid.clone(),
        })
    }

    fn lookup_cgla(
        &self,
        active_aid: &str,
        file_id: Option<u16>,
        apdu: &apdu::ParsedApdu<'_>,
    ) -> Option<String> {
        let is_select_aid = apdu.ins == apdu::Instruction::Select && apdu.p1 == 0x04;

        let aid =
            if is_select_aid { hex::encode_upper(apdu.data()) } else { active_aid.to_string() };

        let adf = self.adfs.iter().find(|am| am.aid == aid)?;

        if !is_select_aid
            && let Some(fid) = file_id
            && let Some(file_mock) = adf.files.iter().find(|f| f.id == fid)
            && let Some(m) = file_mock.cgla.iter().find(|m| m.cmd.matches(apdu))
        {
            return Some(m.response.to_string());
        }

        if apdu.ins == apdu::Instruction::Select
            && (apdu.p1 == 0x00 || apdu.p1 == 0x08)
            && let Some(&chunk) = apdu.data().first_chunk::<2>()
        {
            let target_fid = u16::from_be_bytes(chunk);
            if let Some(file_mock) = adf.files.iter().find(|f| f.id == target_fid)
                && let Some(m) = file_mock.cgla.iter().find(|m| m.cmd.matches(apdu))
            {
                return Some(m.response.to_string());
            }
        }

        adf.cgla.iter().find(|m| m.cmd.matches(apdu)).map(|m| m.response.to_string())
    }

    fn lookup_csim(&self, apdu: &apdu::ParsedApdu<'_>) -> Option<String> {
        let adf = self.adfs.iter().find(|am| am.aid == "CSIM")?;
        adf.csim.iter().find(|m| m.cmd.matches(apdu)).map(|m| m.response.to_string())
    }

    pub(crate) fn get_imsi(&self) -> Option<String> {
        if !self.is_present() {
            return None;
        }
        self.fs.find_ef(UiccFileId::Imsi).and_then(|ef| decode_imsi(&ef.data))
    }

    pub(crate) fn get_iccid(&self) -> Option<String> {
        if !self.is_present() {
            return None;
        }
        self.fs.find_ef(UiccFileId::Iccid).map(|ef| crate::pdu::bcd::bcd_to_string(&ef.data))
    }

    pub(crate) fn get_msisdn(&self) -> Option<PhoneNumber> {
        let ef = self.fs.find_ef_in_telecom_or_usim(UiccFileId::Msisdn)?;
        ef.records().find_map(|r| AdnRecord::decode(r).and_then(|adn| adn.number))
    }

    pub(crate) fn set_msisdn(&mut self, msisdn: Option<&PhoneNumber>) {
        let record_len = self
            .fs
            .find_ef_in_telecom_or_usim(UiccFileId::Msisdn)
            .and_then(|ef| ef.record_len())
            .unwrap_or_else(|| {
                UiccFileId::Msisdn.default_record_len().expect("file id is record based")
            });

        let encoded = msisdn
            .map(|num| AdnRecord::encode_from_number(num, record_len))
            .unwrap_or_else(|| vec![0xFF; record_len]);

        for df_id in [UiccFileId::Telecom, UiccFileId::AdfDefault] {
            if let Some(df) = self.fs.find_df_mut(df_id)
                && let Err(e) = df.update_record(UiccFileId::Msisdn, 1, &encoded)
            {
                debug!("Failed to update MSISDN record 1 in {df_id:?}: {e:?}");
            }
        }
    }

    fn select_sim_file(&mut self, idx: usize, apdu: &apdu::ParsedApdu<'_>) -> Result<(), u16> {
        let Ok(p1) = apdu::SelectP1::try_from(apdu.p1) else {
            return Err(SW_INCORRECT_PARAMS);
        };
        let channel = self.channels.get_mut(idx).ok_or(SW_INCORRECT_PARAMS)?;
        match p1 {
            apdu::SelectP1::ByDfName => {
                if apdu.data().is_empty() {
                    return Err(SW_FILE_NOT_FOUND);
                }
                let aid_upper = hex::encode_upper(apdu.data());
                if self.adfs.iter().any(|am| am.aid == aid_upper) {
                    channel.selected_aid = Some(aid_upper);
                    channel.selected_file = None;
                    Ok(())
                } else {
                    Err(SW_FILE_NOT_FOUND)
                }
            }
            apdu::SelectP1::ByFid | apdu::SelectP1::ByPathFromMf => {
                let Ok(&chunk) = <&[u8; 2]>::try_from(apdu.data()) else {
                    return Err(SW_WRONG_LENGTH);
                };
                let fid = u16::from_be_bytes(chunk);
                let is_file_in_active_adf = if let Some(active_aid) =
                    channel.selected_aid.as_deref()
                    && let Some(adf) = self.adfs.iter().find(|a| a.aid == active_aid)
                {
                    let in_overrides = adf.files.iter().any(|f| f.id == fid);
                    let in_fs_subtree =
                        if let Some(adf_df) = self.fs.find_df(UiccFileId::AdfDefault) {
                            adf_df.find_ef(fid).is_some() || adf_df.find_df(fid).is_some()
                        } else {
                            false
                        };
                    in_overrides || in_fs_subtree
                } else {
                    false
                };
                if self.fs.find_ef(fid).is_some()
                    || self.fs.find_df(fid).is_some()
                    || is_file_in_active_adf
                    || UiccFileId::is_virtual_fallback_id(fid)
                {
                    channel.selected_file = Some(fid);
                    if !is_file_in_active_adf {
                        channel.selected_aid = None;
                    }
                    Ok(())
                } else {
                    Err(SW_FILE_NOT_FOUND)
                }
            }
        }
    }

    pub(crate) fn get_cpin_urc(&self) -> Option<String> {
        let status = match self.state {
            SimState::Absent => return Some("+CPIN: ABSENT\r\n".to_string()),
            SimState::Ready => RequiredPin::None,
            SimState::PinRequired => RequiredPin::SimPin,
            SimState::PukRequired => RequiredPin::SimPuk,
            SimState::PermBlocked => return None,
        };
        Some(format!("{}", SimResponse::PinStatus(status)))
    }

    pub(crate) fn set_present(&mut self, present: bool) -> bool {
        if present && !self.provisioned {
            return false;
        }
        let old_state = self.state;
        if present {
            if self.state == SimState::Absent {
                self.state = if self.pin1.puk_retries == 0 {
                    SimState::PermBlocked
                } else if self.pin1.pin_retries == 0 {
                    SimState::PukRequired
                } else if self.pin_enabled {
                    SimState::PinRequired
                } else {
                    SimState::Ready
                };
            }
        } else {
            self.state = SimState::Absent;
            self.channels = default_channels();
        }
        self.state != old_state
    }

    // --- Public API for other services ---

    pub(crate) fn get_sms_count(&self) -> usize {
        if !self.is_present() {
            return 0;
        }
        self.sms_messages.len()
    }

    fn allocate_sms_slot(&self) -> Option<u8> {
        let mut slot = 1u8;
        for &occupied in self.sms_messages.keys() {
            if occupied == slot {
                slot = slot.checked_add(1)?;
            } else {
                break;
            }
        }
        Some(slot)
    }

    // --- Pure command handlers ---

    fn handle_get_sim_status(&self) -> SimResult {
        let status = match self.state {
            SimState::Absent => {
                return Err(ExecutionResult::cme_error(CmeError::SimNotInserted));
            }
            SimState::Ready => RequiredPin::None,
            SimState::PinRequired => RequiredPin::SimPin,
            SimState::PukRequired => RequiredPin::SimPuk,
            SimState::PermBlocked => return Err(ExecutionResult::cme_error(CmeError::SimFailure)),
        };
        Ok(Some(SimResponse::PinStatus(status)))
    }

    fn sync_pin1_state(&mut self) {
        if self.pin1.is_puk_blocked() {
            self.state = SimState::PermBlocked;
        } else if self.pin1.is_blocked() {
            self.state = SimState::PukRequired;
        }
    }

    fn handle_enter_pin(&mut self, pin_or_puk: PinString, new_pin: Option<PinString>) -> SimResult {
        let input = pin_or_puk.as_str();
        let pin_or_puk_len = input.len();
        let new_pin_str = new_pin.as_ref().map(|p| p.as_str());
        match (self.state, new_pin_str) {
            (SimState::Absent, _) => Err(ExecutionResult::cme_error(CmeError::SimNotInserted)),
            (SimState::PermBlocked, _) => {
                Err(ExecutionResult::cme_error(CmeError::OperationNotAllowed))
            }
            (SimState::Ready, Some(new_pin)) => {
                if self.pin2.is_blocked() && pin_or_puk_len == PUK_LEN {
                    self.pin2
                        .unblock_with_puk(input, new_pin)
                        .map_err(ExecutionResult::cme_error)?;
                    Ok(None)
                } else {
                    self.pin1.change_pin(input, new_pin).map_err(|err| {
                        self.sync_pin1_state();
                        ExecutionResult::cme_error(err.into_cme_error(CmeError::SimPukRequired))
                    })?;
                    Ok(None)
                }
            }
            (SimState::Ready, None) => {
                if pin_or_puk_len == 0 {
                    return Err(ExecutionResult::cme_error(CmeError::IncorrectPassword));
                }
                self.pin2.verify_pin(input).map_err(|err| {
                    ExecutionResult::cme_error(err.into_cme_error(CmeError::SimPuk2Required))
                })?;
                Ok(None)
            }
            (SimState::PinRequired, _) => {
                self.pin1.verify_pin(input).map_err(|_err| {
                    self.sync_pin1_state();
                    ExecutionResult::cme_error(CmeError::IncorrectPassword)
                })?;
                self.state = SimState::Ready;
                Ok(None)
            }
            (SimState::PukRequired, Some(new_pin)) => {
                self.pin1.unblock_with_puk(input, new_pin).map_err(|err| {
                    self.sync_pin1_state();
                    ExecutionResult::cme_error(err)
                })?;
                self.state = SimState::Ready;
                Ok(None)
            }
            (SimState::PukRequired, None) => {
                Err(ExecutionResult::cme_error(CmeError::IncorrectParameters))
            }
        }
    }

    fn get_optional_field(
        val: Option<String>,
        constructor: impl FnOnce(String) -> SimResponse,
    ) -> SimResult {
        val.map(|v| Some(constructor(v)))
            .ok_or_else(|| ExecutionResult::cme_error(CmeError::NotFound))
    }

    fn update_sim_file(
        &mut self,
        command: apdu::Instruction,
        file_id: impl Into<u16>,
        p1: u8,
        p2: u8,
        p3: u8,
        hex_str: &str,
    ) -> Result<(), SimResponse> {
        let file_id = file_id.into();
        let is_dual_synced = UiccFileId::try_from(file_id).is_ok_and(UiccFileId::is_dual_df_synced);

        if is_dual_synced {
            let mut updated = false;
            for df_id in [UiccFileId::Telecom, UiccFileId::AdfDefault] {
                if let Some(df) = self.fs.find_df_mut(df_id)
                    && let Some(ef) = df.find_ef_mut(file_id)
                {
                    ef.update(command, p1, p2, p3, hex_str).map_err(map_sw_to_response)?;
                    updated = true;
                }
            }
            if updated {
                return Ok(());
            }
        }

        // Update in the file system if it exists there
        if let Some(ef) = self.fs.find_ef_mut(file_id) {
            ef.update(command, p1, p2, p3, hex_str).map_err(map_sw_to_response)
        } else if UiccFileId::is_virtual_fallback_id(file_id) {
            Ok(())
        } else {
            Err(RESP_FILE_NOT_FOUND)
        }
    }

    fn read_binary_from_fs(
        &self,
        file_id: impl Into<u16>,
        p1: u8,
        p2: u8,
        p3: u8,
    ) -> Result<String, SimResponse> {
        let file_id = file_id.into();
        if let Some(ef) = self.fs.find_ef(file_id) {
            let offset = u16::from_be_bytes([p1, p2]) as usize;
            let ef_size = ef.size();
            if offset > ef_size {
                return Err(RESP_INCORRECT_PARAMS);
            }
            let length = if p3 == 0 { ef_size - offset } else { p3 as usize };
            if offset + length > ef_size {
                return Err(RESP_WRONG_LENGTH);
            }
            let end = std::cmp::min(offset + length, ef.data.len());
            let sliced = ef.data.get(offset..end).unwrap_or(&[]);
            Ok(hex::encode_upper(sliced))
        } else {
            Err(RESP_FILE_NOT_FOUND)
        }
    }

    fn read_record_from_fs(
        &self,
        file_id: impl Into<u16>,
        record_num: u8,
        p2: u8,
        p3: u8,
    ) -> Result<String, SimResponse> {
        if let Ok(mode) = apdu::RecordMode::try_from(p2) {
            if !mode.is_absolute() {
                return Err(RESP_INCORRECT_PARAMS);
            }
            let file_id = file_id.into();
            if let Some(ef) = self.fs.find_ef(file_id) {
                if let Some(record) = ef.record(record_num as usize) {
                    let p3_usize = p3 as usize;
                    let length = if p3_usize == 0 { record.len() } else { p3_usize };
                    let slice = record.get(..length).ok_or(RESP_WRONG_LENGTH)?;
                    return Ok(hex::encode_upper(slice));
                }
                Err(RESP_REFERENCED_DATA_NOT_FOUND)
            } else {
                Err(RESP_FILE_NOT_FOUND)
            }
        } else {
            Err(RESP_INCORRECT_PARAMS)
        }
    }

    fn handle_sim_io(
        &mut self,
        command: apdu::Instruction,
        file_id: u16,
        p1: u8,
        p2: u8,
        p3: u8,
        data: Option<&str>,
    ) -> SimResult {
        // 1. Handle UPDATE BINARY and UPDATE RECORD
        if command.is_update() {
            let resp = if let Some(hex_str) = data {
                match self.update_sim_file(command, file_id, p1, p2, p3, hex_str) {
                    Ok(()) => RESP_SUCCESS,
                    Err(err_resp) => err_resp,
                }
            } else {
                RESP_INCORRECT_PARAMS
            };
            return Ok(Some(resp));
        }

        // 2. Try to read from the loaded FileSystem first (for READ BINARY and SELECT)
        if command == apdu::Instruction::ReadBinary {
            match self.read_binary_from_fs(file_id, p1, p2, p3) {
                Ok(data_hex) => {
                    return Ok(Some(SimResponse::RestrictedSimAccess {
                        sw: SW_SUCCESS,
                        data: Some(data_hex),
                    }));
                }
                Err(err_resp) => return Ok(Some(err_resp)),
            }
        } else if command == apdu::Instruction::ReadRecord {
            match self.read_record_from_fs(file_id, p1, p2, p3) {
                Ok(record_hex) => {
                    return Ok(Some(SimResponse::RestrictedSimAccess {
                        sw: SW_SUCCESS,
                        data: Some(record_hex),
                    }));
                }
                Err(err_resp) => return Ok(Some(err_resp)),
            }
        } else if command == apdu::Instruction::Select && self.fs.find_df(file_id).is_some() {
            return Ok(Some(SimResponse::RestrictedSimAccess {
                sw: SW_SUCCESS,
                data: Some("6210".to_string()),
            }));
        } else if command == apdu::Instruction::Status {
            // Return FCP template for Master File (MF)
            return Ok(Some(SimResponse::RestrictedSimAccess {
                sw: SW_SUCCESS,
                data: Some(STATUS_FCP_HEX.to_string()),
            }));
        } else if command == apdu::Instruction::GetResponse {
            let header_opt = if let Some(ef) = self.fs.find_ef(file_id) {
                Some(SimResponseHeader::Elementary(ElementaryFileResponseHeader::new(
                    ef.file_id,
                    ef.size(),
                    ef.record_len(),
                )))
            } else if let Some(df) = self.fs.find_df(file_id) {
                let df_count =
                    df.files.iter().filter(|f| matches!(f, SimFile::DedicatedFile(_))).count();
                let ef_count =
                    df.files.iter().filter(|f| matches!(f, SimFile::ElementaryFile(_))).count();
                let is_mf = df.file_id == UiccFileId::MasterFile.as_u16();
                Some(SimResponseHeader::Dedicated(DedicatedFileResponseHeader::new(
                    df.file_id, is_mf, df_count, ef_count,
                )))
            } else {
                None
            };

            if let Some(header) = header_opt {
                let header_bytes = header.to_bytes();
                let end = if p3 == 0 { header_bytes.len() } else { p3 as usize };
                let resp_data = header_bytes.get(..end).unwrap_or(header_bytes.as_slice());
                return Ok(Some(SimResponse::RestrictedSimAccess {
                    sw: SW_SUCCESS,
                    data: Some(hex::encode_upper(resp_data)),
                }));
            }
        }

        Ok(Some(RESP_FILE_NOT_FOUND))
    }

    fn handle_open_logical_channel(&mut self, aid: ApplicationId) -> SimResult {
        let aid_clean = aid.as_str().to_ascii_uppercase();

        if !aid_clean.is_empty() {
            let aid_exists = self.adfs.iter().any(|am| am.aid == aid_clean);
            if !aid_exists {
                info!("[SimService] Rejecting logical channel for unknown AID: {}", aid_clean);
                return Err(ExecutionResult::cme_error(CmeError::NotFound));
            }
        }

        if let Some((channel_idx, channel)) =
            self.channels.iter_mut().enumerate().skip(1).find(|(_, ch)| !ch.open)
        {
            channel.open = true;
            channel.selected_aid = Some(aid_clean);
            Ok(Some(SimResponse::OpenLogicalChannel(channel_idx as u8)))
        } else {
            Err(ExecutionResult::cme_error(CmeError::NoResources))
        }
    }

    fn reset_channel(&mut self, idx: usize, clear_buffer: bool) {
        if let Some(channel) = self.channels.get_mut(idx) {
            channel.reset(clear_buffer);
        }
    }

    fn handle_close_logical_channel(&mut self, channel_id: u8) -> SimResult {
        let idx = channel_id as usize;
        if idx == 0 {
            return Err(ExecutionResult::cme_error(CmeError::InvalidIndex));
        }
        let channel = self
            .channels
            .get_mut(idx)
            .ok_or_else(|| ExecutionResult::cme_error(CmeError::InvalidIndex))?;
        if !channel.open {
            return Err(ExecutionResult::cme_error(CmeError::NotFound));
        }
        channel.reset(false);

        // Non-standard: AOSP Goldfish RIL requires "+CCHC" response on channel
        // close to prevent serialization locks.
        // TODO: Extract goldfish-specific quirks into flags.
        Ok(Some(SimResponse::CloseLogicalChannel))
    }

    fn process_logical_channel_apdu(
        &mut self,
        access_type: SimAccessType,
        idx: usize,
        apdu: &apdu::ParsedApdu<'_>,
    ) -> String {
        if !apdu.class.is_supported() {
            return format_sim_payload_status(SW_CLASS_NOT_SUPPORTED);
        }

        if apdu.ins != apdu::Instruction::GetResponse
            && let Some(channel) = self.channels.get_mut(idx)
        {
            channel.response_buffer.clear();
        }

        let (active_aid, selected_fid) = match self.channels.get(idx) {
            Some(ch) => (ch.selected_aid.as_deref().unwrap_or(""), ch.selected_file),
            None => ("", None),
        };

        // 2. Immediate validation check for invalid SELECT P2 parameter
        if apdu.ins == apdu::Instruction::Select && apdu.p2 == P2_INVALID_SELECT {
            return format_sim_payload_status(SW_INCORRECT_PARAMS);
        }

        // 3. Honor explicit APDU mappings defined in the XML profile
        let profile_response = match access_type {
            SimAccessType::Cgla => self.lookup_cgla(active_aid, selected_fid, apdu),
            SimAccessType::Csim => self.lookup_csim(apdu),
        };
        if let Some(resp) = profile_response {
            if apdu.ins == apdu::Instruction::Select {
                if let Some(sw) = get_status_word_from_payload(&resp)
                    && !is_error_sw(sw)
                {
                    // Ignore any error because the explicit XML profile
                    // mapping is authoritative
                    let _ = self.select_sim_file(idx, apdu);
                }
            } else if apdu.ins == apdu::Instruction::ManageChannel
                && let Some(sw) = get_status_word_from_payload(&resp)
                && sw == SW_SUCCESS
                && let Ok(action) = apdu::ManageChannelAction::try_from(apdu.p1)
            {
                match action {
                    apdu::ManageChannelAction::Open => {
                        if let Some(channel_idx) = get_channel_idx_from_open_response(&resp) {
                            if let Some(channel) = self.channels.get_mut(channel_idx) {
                                channel.open = true;
                                channel.selected_file = Some(UiccFileId::MasterFile.as_u16());
                                if access_type == SimAccessType::Csim {
                                    channel.selected_aid = Some("CSIM".to_string());
                                }
                            } else {
                                return format_sim_payload_status(SW_INCORRECT_PARAMS);
                            }
                        }
                    }
                    apdu::ManageChannelAction::Close => {
                        let target_idx = if apdu.p2 == 0 { idx } else { apdu.p2 as usize };
                        if target_idx != 0 {
                            self.reset_channel(target_idx, true);
                        }
                    }
                }
            }
            return resp;
        }

        // 4. Process standard filesystem APDUs
        let response_data = match apdu.ins {
            apdu::Instruction::ReadBinary => {
                if let Some(fid) = selected_fid {
                    match self.read_binary_from_fs(
                        fid,
                        apdu.p1,
                        apdu.p2,
                        apdu.expected_length().unwrap_or(0),
                    ) {
                        Ok(data_hex) => Some(format_sim_payload_data(&data_hex, SW_SUCCESS)),
                        Err(SimResponse::RestrictedSimAccess { sw, .. }) => {
                            Some(format_sim_payload_status(sw))
                        }
                        Err(_) => Some(format_sim_payload_status(SW_TECHNICAL_PROBLEM)),
                    }
                } else {
                    None
                }
            }
            apdu::Instruction::ReadRecord => {
                if let Some(fid) = selected_fid {
                    match self.read_record_from_fs(
                        fid,
                        apdu.p1,
                        apdu.p2,
                        apdu.expected_length().unwrap_or(0),
                    ) {
                        Ok(record_hex) => Some(format_sim_payload_data(&record_hex, SW_SUCCESS)),
                        Err(SimResponse::RestrictedSimAccess { sw, .. }) => {
                            Some(format_sim_payload_status(sw))
                        }
                        Err(_) => Some(format_sim_payload_status(SW_TECHNICAL_PROBLEM)),
                    }
                } else {
                    None
                }
            }
            apdu::Instruction::UpdateBinary => {
                if let Some(fid) = selected_fid {
                    let data_hex = hex::encode_upper(apdu.data());
                    match self.update_sim_file(
                        apdu::Instruction::UpdateBinary,
                        fid,
                        apdu.p1,
                        apdu.p2,
                        apdu.data().len() as u8,
                        &data_hex,
                    ) {
                        Ok(()) => Some(format_sim_payload_status(SW_SUCCESS)),
                        Err(SimResponse::RestrictedSimAccess { sw, .. }) => {
                            Some(format_sim_payload_status(sw))
                        }
                        Err(_) => Some(format_sim_payload_status(SW_TECHNICAL_PROBLEM)),
                    }
                } else {
                    None
                }
            }
            apdu::Instruction::UpdateRecord => {
                if let Some(fid) = selected_fid {
                    let data_hex = hex::encode_upper(apdu.data());
                    match self.update_sim_file(
                        apdu::Instruction::UpdateRecord,
                        fid,
                        apdu.p1,
                        apdu.p2,
                        apdu.data().len() as u8,
                        &data_hex,
                    ) {
                        Ok(()) => Some(format_sim_payload_status(SW_SUCCESS)),
                        Err(SimResponse::RestrictedSimAccess { sw, .. }) => {
                            Some(format_sim_payload_status(sw))
                        }
                        Err(_) => Some(format_sim_payload_status(SW_TECHNICAL_PROBLEM)),
                    }
                } else {
                    None
                }
            }
            _ => None,
        };

        // 5. Handle basic control APDUs or return error
        match response_data {
            Some(resp) => resp,
            None => match apdu.ins {
                apdu::Instruction::Select => {
                    if let Ok(p2) = apdu::SelectP2::try_from(apdu.p2) {
                        if p2 == apdu::SelectP2::ReturnProprietary {
                            return format_sim_payload_status(SW_INCORRECT_PARAMS);
                        }
                        match self.select_sim_file(idx, apdu) {
                            Ok(()) => {
                                if p2 == apdu::SelectP2::ReturnFcp {
                                    let (fid, aid) = match self.channels.get(idx) {
                                        Some(ch) => (
                                            ch.selected_file
                                                .unwrap_or(UiccFileId::AdfDefault.as_u16()),
                                            ch.selected_aid.as_deref(),
                                        ),
                                        None => (UiccFileId::AdfDefault.as_u16(), None),
                                    };
                                    let fcp_hex = generate_df_fcp(fid, aid);
                                    if let Ok(fcp_bytes) = hex::decode(&fcp_hex) {
                                        let len = fcp_bytes.len();
                                        if let Some(channel) = self.channels.get_mut(idx) {
                                            channel.response_buffer = fcp_bytes;
                                        }
                                        let sw_low = if len >= 256 { 0 } else { len as u8 };
                                        let sw = ((SW_BYTES_REMAINING_PREFIX as u16) << 8)
                                            | (sw_low as u16);
                                        return format_sim_payload_status(sw);
                                    }
                                }
                                format_sim_payload_status(SW_SUCCESS)
                            }
                            Err(sw) => format_sim_payload_status(sw),
                        }
                    } else {
                        format_sim_payload_status(SW_INCORRECT_PARAMS)
                    }
                }
                apdu::Instruction::Status => format_sim_payload_data(STATUS_FCP_HEX, SW_SUCCESS),
                apdu::Instruction::ManageChannel => {
                    let Ok(action) = apdu::ManageChannelAction::try_from(apdu.p1) else {
                        return format_sim_payload_status(SW_INCORRECT_PARAMS);
                    };
                    match action {
                        apdu::ManageChannelAction::Open => {
                            if let Some((channel_idx, channel)) = self
                                .channels
                                .iter_mut()
                                .enumerate()
                                .skip(1)
                                .find(|(_, ch)| !ch.open)
                            {
                                channel.open = true;
                                let channel_hex = format!("{channel_idx:02X}");
                                format_sim_payload_data(&channel_hex, SW_SUCCESS)
                            } else {
                                format_sim_payload_status(SW_NO_CHANNEL_AVAILABLE)
                            }
                        }
                        apdu::ManageChannelAction::Close => {
                            let target_idx = if apdu.p2 == 0 { idx } else { apdu.p2 as usize };
                            if target_idx == 0 {
                                format_sim_payload_status(SW_INCORRECT_PARAMS) // 6A86
                            } else {
                                match self.channels.get_mut(target_idx) {
                                    Some(ch) if ch.open => {
                                        ch.reset(true);
                                        format_sim_payload_status(SW_SUCCESS)
                                    }
                                    _ => format_sim_payload_status(SW_REFERENCED_DATA_NOT_FOUND), // 6A88
                                }
                            }
                        }
                    }
                }
                apdu::Instruction::GetResponse => self.handle_get_response(idx, apdu),
                _ => {
                    if access_type == SimAccessType::Cgla {
                        format_sim_payload_status(SW_INS_NOT_SUPPORTED)
                    } else {
                        format_sim_payload_status(SW_INCORRECT_PARAMS)
                    }
                }
            },
        }
    }

    fn handle_transmit_logical_channel(&mut self, channel_id: u8, data: ApduData) -> SimResult {
        let idx = channel_id as usize;
        match self.channels.get(idx) {
            Some(ch) if ch.open => {}
            Some(_) => return Err(ExecutionResult::cme_error(CmeError::NotFound)),
            None => return Err(ExecutionResult::cme_error(CmeError::InvalidIndex)),
        }

        let apdu_bytes = match data.decode_hex() {
            Ok(b) => b,
            Err(_) => {
                return Ok(Some(SimResponse::GenericLogicalChannelAccess(
                    format_sim_payload_status(SW_TECHNICAL_PROBLEM),
                )));
            }
        };

        let parsed = match apdu::ParsedApdu::parse(&apdu_bytes) {
            Ok(p) => p,
            Err(sw) => {
                return Ok(Some(SimResponse::GenericLogicalChannelAccess(
                    format_sim_payload_status(sw),
                )));
            }
        };

        if !parsed.class.is_supported()
            || (parsed.class.channel() != 0 && parsed.class.channel() != idx)
        {
            return Ok(Some(SimResponse::GenericLogicalChannelAccess(format_sim_payload_status(
                SW_CLASS_NOT_SUPPORTED,
            ))));
        }

        let response_data = self.process_logical_channel_apdu(SimAccessType::Cgla, idx, &parsed);

        Ok(Some(SimResponse::GenericLogicalChannelAccess(response_data)))
    }

    fn handle_csim_manage_channel(&mut self, channel_idx: usize, p1: u8, p2: u8) -> SimResult {
        let Ok(action) = apdu::ManageChannelAction::try_from(p1) else {
            let status = SW_INCORRECT_PARAMS;
            return Ok(Some(SimResponse::GenericSimAccess(format!("4,{status:04X}"))));
        };
        match action {
            apdu::ManageChannelAction::Open => {
                if let Some((channel_idx, channel)) =
                    self.channels.iter_mut().enumerate().skip(1).find(|(_, ch)| !ch.open)
                {
                    channel.open = true;
                    channel.selected_aid = Some("CSIM".to_string()); // CSIM channel
                    let resp_hex = format!("{channel_idx:02X}{SW_SUCCESS:04X}");
                    Ok(Some(SimResponse::GenericSimAccess(format!(
                        "{},{resp_hex}",
                        resp_hex.len()
                    ))))
                } else {
                    // No channel available
                    Ok(Some(SimResponse::GenericSimAccess(format!(
                        "4,{SW_NO_CHANNEL_AVAILABLE:04X}"
                    ))))
                }
            }
            apdu::ManageChannelAction::Close => {
                let target_idx = if p2 == 0 { channel_idx } else { p2 as usize };
                if target_idx == 0 {
                    let status = SW_INCORRECT_PARAMS;
                    Ok(Some(SimResponse::GenericSimAccess(format!("4,{status:04X}"))))
                } else {
                    match self.channels.get_mut(target_idx) {
                        Some(ch) if ch.open => {
                            ch.reset(false);
                            Ok(Some(SimResponse::GenericSimAccess(format!("4,{SW_SUCCESS:04X}"))))
                        }
                        _ => {
                            let status = SW_REFERENCED_DATA_NOT_FOUND;
                            Ok(Some(SimResponse::GenericSimAccess(format!("4,{status:04X}"))))
                        }
                    }
                }
            }
        }
    }

    fn handle_generic_sim_access(&mut self, _len: u32, apdu: ApduData) -> SimResult {
        let apdu_bytes = match apdu.decode_hex() {
            Ok(b) => b,
            Err(_) => return Err(ExecutionResult::cme_error(CmeError::Custom(100, "unknown"))),
        };

        let parsed = match apdu::ParsedApdu::parse(&apdu_bytes) {
            Ok(p) => p,
            Err(sw) => {
                let payload = format_sim_payload_status(sw);
                return Ok(Some(SimResponse::GenericSimAccess(payload)));
            }
        };

        let channel_idx = parsed.class.channel();
        let channel_open = self.channels.get(channel_idx).is_some_and(|ch| ch.open);
        if !parsed.class.is_supported() || !channel_open {
            return Ok(Some(SimResponse::GenericSimAccess(format_sim_payload_status(
                SW_CLASS_NOT_SUPPORTED,
            ))));
        }

        if parsed.ins == apdu::Instruction::ManageChannel {
            self.handle_csim_manage_channel(channel_idx, parsed.p1, parsed.p2)
        } else {
            let payload =
                self.process_logical_channel_apdu(SimAccessType::Csim, channel_idx, &parsed);
            Ok(Some(SimResponse::GenericSimAccess(payload)))
        }
    }

    fn handle_change_password(
        &mut self,
        facility: Facility,
        old_password: QuotedString,
        new_password: QuotedString,
    ) -> SimResult {
        if self.state == SimState::PermBlocked {
            return Err(ExecutionResult::cme_error(CmeError::OperationNotAllowed));
        }
        if self.state == SimState::PukRequired {
            return Err(ExecutionResult::cme_error(CmeError::SimPukRequired));
        }
        let old = old_password.as_str();
        let new = new_password.as_str();
        match facility {
            Facility::SimPin | Facility::SimPin2 | Facility::FixedDial => {
                let is_pin1 = facility == Facility::SimPin;
                let blocked_err =
                    if is_pin1 { CmeError::SimPukRequired } else { CmeError::SimPuk2Required };
                let target = if is_pin1 { &mut self.pin1 } else { &mut self.pin2 };
                target.change_pin(old, new).map_err(|err| {
                    if is_pin1 {
                        self.sync_pin1_state();
                    }
                    ExecutionResult::cme_error(err.into_cme_error(blocked_err))
                })?;
                Ok(None)
            }
            f if f.is_call_barring() => {
                if self.barring_password.is_blocked() {
                    return Err(ExecutionResult::cme_error(CmeError::OperationNotAllowed));
                }
                self.barring_password.change_pin(old, new).map_err(|err| {
                    ExecutionResult::cme_error(err.into_cme_error(CmeError::IncorrectPassword))
                })?;
                Ok(None)
            }
            _ => Err(ExecutionResult::cme_error(CmeError::OperationNotSupported)),
        }
    }

    pub(crate) fn handle_set_call_barring_lock(
        &mut self,
        facility: Facility,
        mode: FacilityLockMode,
        passwd: Option<QuotedString>,
    ) -> SimResult {
        if (mode == FacilityLockMode::Unlock || mode == FacilityLockMode::Lock)
            && self.barring_password.is_blocked()
        {
            return Err(ExecutionResult::cme_error(CmeError::OperationNotAllowed));
        }

        match mode {
            FacilityLockMode::Unlock | FacilityLockMode::Lock => {
                let passwd = passwd
                    .ok_or_else(|| ExecutionResult::cme_error(CmeError::IncorrectPassword))?;
                self.barring_password
                    .verify_pin(passwd.as_str())
                    .map(|()| {
                        if mode == FacilityLockMode::Lock {
                            self.call_barring_locks.insert(facility, true);
                        } else {
                            self.call_barring_locks.remove(&facility);
                        }
                        None
                    })
                    .map_err(|err| {
                        ExecutionResult::cme_error(err.into_cme_error(CmeError::IncorrectPassword))
                    })
            }
            FacilityLockMode::QueryStatus => {
                let is_locked = self.call_barring_locks.contains_key(&facility);
                Ok(Some(SimResponse::FacilityLockStatus(if is_locked { 1 } else { 0 })))
            }
        }
    }

    fn handle_sim_authentication(&self, data: ApduData) -> SimResult {
        let data_clean = data.as_str();
        let response = match data_clean {
            AUTH_CHALLENGE_1 => AUTH_RESPONSE_1,
            AUTH_CHALLENGE_2 => AUTH_RESPONSE_2,
            AUTH_CHALLENGE_3 => AUTH_RESPONSE_3,
            AUTH_CHALLENGE_4 => AUTH_RESPONSE_4,
            _ => {
                if data_clean.contains(',') {
                    AUTH_RESPONSE_4
                } else {
                    AUTH_RESPONSE_3
                }
            }
        };
        Ok(Some(SimResponse::SimAuthentication(response.to_string())))
    }

    pub(crate) fn handle_set_facility_lock(
        &mut self,
        facility: Facility,
        mode: FacilityLockMode,
        passwd: Option<QuotedString>,
    ) -> SimResult {
        match facility {
            Facility::SimPin => self.handle_set_pin1_lock(mode, passwd),
            Facility::FixedDial => self.handle_set_fdn_lock(mode, passwd),
            f if f.is_call_barring() => self.handle_set_call_barring_lock(f, mode, passwd),
            _ => Err(ExecutionResult::cme_error(CmeError::OperationNotSupported)),
        }
    }

    pub(crate) fn handle_set_pin1_lock(
        &mut self,
        mode: FacilityLockMode,
        passwd: Option<QuotedString>,
    ) -> SimResult {
        if mode == FacilityLockMode::Unlock || mode == FacilityLockMode::Lock {
            if self.state == SimState::PermBlocked {
                return Err(ExecutionResult::cme_error(CmeError::OperationNotAllowed));
            }
            if self.state == SimState::PukRequired {
                return Err(ExecutionResult::cme_error(CmeError::SimPukRequired));
            }
        }
        match mode {
            FacilityLockMode::Unlock | FacilityLockMode::Lock => {
                let passwd = passwd
                    .ok_or_else(|| ExecutionResult::cme_error(CmeError::IncorrectPassword))?;
                self.pin1
                    .verify_pin(passwd.as_str())
                    .map(|()| {
                        self.pin_enabled = mode == FacilityLockMode::Lock;
                        self.state = SimState::Ready;
                        None
                    })
                    .map_err(|err| {
                        self.sync_pin1_state();
                        ExecutionResult::cme_error(err.into_cme_error(CmeError::SimPukRequired))
                    })
            }
            FacilityLockMode::QueryStatus => {
                Ok(Some(SimResponse::FacilityLockStatus(if self.pin_enabled { 1 } else { 0 })))
            }
        }
    }

    pub(crate) fn handle_set_fdn_lock(
        &mut self,
        mode: FacilityLockMode,
        passwd: Option<QuotedString>,
    ) -> SimResult {
        if (mode == FacilityLockMode::Unlock || mode == FacilityLockMode::Lock)
            && self.pin2.is_blocked()
        {
            return Err(ExecutionResult::cme_error(CmeError::SimPuk2Required));
        }

        match mode {
            FacilityLockMode::Unlock | FacilityLockMode::Lock => {
                let passwd = passwd
                    .ok_or_else(|| ExecutionResult::cme_error(CmeError::IncorrectPassword))?;
                self.pin2
                    .verify_pin(passwd.as_str())
                    .map(|()| {
                        self.fdn_enabled = mode == FacilityLockMode::Lock;
                        None
                    })
                    .map_err(|err| {
                        ExecutionResult::cme_error(err.into_cme_error(CmeError::SimPuk2Required))
                    })
            }
            FacilityLockMode::QueryStatus => {
                Ok(Some(SimResponse::FacilityLockStatus(if self.fdn_enabled { 1 } else { 0 })))
            }
        }
    }

    pub(crate) fn is_fdn_allowed(&self, number: &PhoneNumber) -> bool {
        if !self.fdn_enabled {
            return true;
        }

        let Some(fdn_ef) = self.fs.find_ef(UiccFileId::FixedDialingNumbers) else {
            return false;
        };

        let normalized_target = number.normalized();
        if normalized_target.is_empty() {
            return false;
        }

        for record in fdn_ef.records() {
            if let Some(record) = AdnRecord::decode(record)
                && let Some(fdn_number) = record.number
            {
                let normalized_fdn = fdn_number.normalized();
                if !normalized_fdn.is_empty() && normalized_target.starts_with(normalized_fdn) {
                    return true;
                }
            }
        }

        false
    }

    fn handle_query_pin_retries_cpinr(&self, pin_type: PinType) -> SimResult {
        let (retries, default_retries) = match pin_type {
            PinType::SimPin => self.pin1.pin_attempts(),
            PinType::SimPuk => self.pin1.puk_attempts(),
            PinType::SimPin2 => self.pin2.pin_attempts(),
            PinType::SimPuk2 => self.pin2.puk_attempts(),
        };
        Ok(Some(SimResponse::PinRemainingAttempts { pin_type, retries, default_retries }))
    }

    pub(crate) fn execute<'a>(&mut self, command: &SimCommand<'a>) -> ExecutionResult {
        info!("[SimService] Executing SIM command: {command:?}");
        if self.state == SimState::Absent {
            return ExecutionResult::cme_error(CmeError::SimNotInserted); /* SIM not inserted */
        }
        let sim_result = match command {
            SimCommand::GenericSimAccess(len, apdu) => self.handle_generic_sim_access(*len, *apdu),
            SimCommand::GetSimStatus => self.handle_get_sim_status(),
            SimCommand::EnterPin(pin, new_pin) => self.handle_enter_pin(*pin, *new_pin),
            SimCommand::SimIo { command, file_id, p1, p2, p3, data, path: _ } => {
                self.handle_sim_io(*command, *file_id, *p1, *p2, *p3, data.map(|d| d.as_str()))
            }
            SimCommand::GetImsi => Self::get_optional_field(self.get_imsi(), SimResponse::Imsi),
            SimCommand::GetIccid => Self::get_optional_field(self.get_iccid(), SimResponse::Iccid),
            SimCommand::OpenLogicalChannel(aid) => self.handle_open_logical_channel(*aid),
            SimCommand::CloseLogicalChannel(channel_id) => {
                self.handle_close_logical_channel(*channel_id)
            }
            SimCommand::TransmitLogicalChannel(channel_id, _, data) => {
                self.handle_transmit_logical_channel(*channel_id, *data)
            }
            SimCommand::ChangePassword(facility, old_password, new_password) => {
                self.handle_change_password(*facility, *old_password, *new_password)
            }
            SimCommand::SetFacilityLock(facility, mode, passwd, _) => {
                self.handle_set_facility_lock(*facility, *mode, *passwd)
            }
            SimCommand::QueryPinRetries(pin_type) => self.handle_query_pin_retries_cpinr(*pin_type),
            SimCommand::SetCdmaSubscriptionSource(source) => {
                self.cdma_subscription_source = *source;
                Ok(None)
            }
            SimCommand::QueryCdmaSubscriptionSource => {
                Ok(Some(SimResponse::CdmaSubscriptionSource(self.cdma_subscription_source)))
            }
            SimCommand::SetCdmaRoamingPreference(preference) => {
                self.cdma_roaming_preference = *preference;
                Ok(None)
            }
            SimCommand::QueryCdmaRoamingPreference => {
                Ok(Some(SimResponse::CdmaRoamingPreference(self.cdma_roaming_preference)))
            }
            SimCommand::SimAuthentication(data) => self.handle_sim_authentication(*data),
            SimCommand::SimAuthenticationVendor(data) => self.handle_sim_authentication(*data),
            SimCommand::GetEid => Self::get_optional_field(self.eid.clone(), SimResponse::Eid),
            SimCommand::GetAtr => Self::get_optional_field(self.atr.clone(), SimResponse::Atr),
        };

        sim_result.into()
    }

    fn handle_get_response(&mut self, idx: usize, apdu: &apdu::ParsedApdu<'_>) -> String {
        if apdu.p1 != 0x00 || apdu.p2 != 0x00 {
            return format_sim_payload_status(SW_INCORRECT_PARAMS);
        }
        let Some(channel) = self.channels.get_mut(idx) else {
            return format_sim_payload_status(SW_REFERENCED_DATA_NOT_FOUND);
        };
        let buf = &mut channel.response_buffer;
        if buf.is_empty() {
            return format_sim_payload_status(SW_REFERENCED_DATA_NOT_FOUND);
        }

        let req_len = if let Some(le) = apdu.expected_length() {
            if le == 0 { 256 } else { le as usize }
        } else {
            buf.len()
        };

        let available = buf.len();
        let take_len = req_len.min(available);
        let chunk: Vec<u8> = buf.drain(..take_len).collect();
        let remaining = buf.len();
        let hex_data = hex::encode_upper(&chunk);

        if remaining > 0 {
            let sw_low = if remaining >= 256 { 0 } else { remaining as u8 };
            let sw = ((SW_BYTES_REMAINING_PREFIX as u16) << 8) | (sw_low as u16);
            format_sim_payload_data(&hex_data, sw)
        } else {
            format_sim_payload_data(&hex_data, SW_SUCCESS)
        }
    }

    fn check_sms_access(&self) -> Result<(), CmsError> {
        if !self.is_present() {
            Err(CmsError::SimNotInserted)
        } else if !self.is_ready() {
            Err(CmsError::SimPinRequired)
        } else {
            Ok(())
        }
    }
}

impl CardState for SimService {
    fn is_present(&self) -> bool {
        self.provisioned && self.state != SimState::Absent
    }

    /// True when PIN/PUK authentication has succeeded and the card is
    /// operational.
    fn is_ready(&self) -> bool {
        self.provisioned && self.state == SimState::Ready
    }

    fn validate_call(&self, number: &PhoneNumber, is_emergency: bool) -> Result<(), CmeError> {
        if is_emergency {
            return Ok(());
        }
        if let Some(err) = self.gating_error() {
            return Err(err);
        }
        if !self.is_fdn_allowed(number) {
            return Err(CmeError::FixedDialNumberOnlyAllowed);
        }
        Ok(())
    }
}

impl SmsStore for SimService {
    fn store_sms(&mut self, message: SimSmsMessage) -> Result<u8, CmsError> {
        self.check_sms_access()?;
        let index = self.allocate_sms_slot().ok_or(CmsError::MemoryFull)?;
        self.sms_messages.insert(index, message);
        Ok(index)
    }

    fn read_sms(&mut self, index: u8) -> Result<SimSmsMessage, CmsError> {
        self.check_sms_access()?;
        if let Some(msg) = self.sms_messages.get_mut(&index) {
            let res = msg.clone();
            msg.mark_read();
            Ok(res)
        } else {
            Err(CmsError::InvalidMemoryIndex)
        }
    }

    fn delete_sms(&mut self, index: u8) -> Result<(), CmsError> {
        self.check_sms_access()?;
        if self.sms_messages.remove(&index).is_some() {
            Ok(())
        } else {
            Err(CmsError::InvalidMemoryIndex)
        }
    }
}

fn generate_df_fcp(df_id: u16, active_aid: Option<&str>) -> String {
    let mut fcp_arr = STATUS_FCP_BYTES;
    let [_, _, _, _, _, _, _, _, fid0, fid1, ..] = &mut fcp_arr;
    [*fid0, *fid1] = df_id.to_be_bytes();
    let mut fcp_bytes = Vec::from(fcp_arr);

    if df_id == UiccFileId::AdfDefault.as_u16()
        && let Some(aid) = active_aid
        && let Ok(aid_bytes) = hex::decode(aid)
    {
        // Append Tag '84' (DF Name / AID)
        fcp_bytes.push(TAG_DF_NAME);
        fcp_bytes.push(aid_bytes.len() as u8);
        fcp_bytes.extend(aid_bytes);
        // Update Tag '62' length at index 1
        let fcp_len = (fcp_bytes.len() - 2) as u8;
        if let [_, len_byte, ..] = fcp_bytes.as_mut_slice() {
            *len_byte = fcp_len;
        }
    }
    hex::encode_upper(fcp_bytes)
}

fn decode_imsi(bytes: &[u8]) -> Option<String> {
    let (&len_byte, rest) = bytes.split_first()?;
    let len = len_byte as usize;
    let (&first_byte, content) = rest.get(..len)?.split_first()?;

    let mut imsi = String::new();
    let digit_1 = first_byte >> 4;
    if digit_1 <= 9 {
        imsi.push((b'0' + digit_1) as char);
    }

    for &b in content {
        let low = b & 0x0F;
        let high = b >> 4;
        if low <= 9 {
            imsi.push((b'0' + low) as char);
        }
        if high <= 9 {
            imsi.push((b'0' + high) as char);
        }
    }
    Some(imsi)
}

fn map_sw_to_response(sw: u16) -> SimResponse {
    match sw {
        SW_INCORRECT_PARAMS => RESP_INCORRECT_PARAMS,
        SW_WRONG_LENGTH => RESP_WRONG_LENGTH,
        SW_REFERENCED_DATA_NOT_FOUND => RESP_REFERENCED_DATA_NOT_FOUND,
        _ => SimResponse::RestrictedSimAccess { sw, data: None },
    }
}

fn format_sim_payload_data(data_hex: &str, status_word: u16) -> String {
    let combined_len = data_hex.len() + 4;
    format!("{combined_len},{data_hex}{status_word:04X}")
}

fn format_sim_payload_status(status_word: u16) -> String {
    format_sim_payload_data("", status_word)
}

// Helper functions for state synchronization and status word parsing
fn get_status_word_from_payload(payload: &str) -> Option<u16> {
    let part2 = payload.split(',').nth(1)?;
    let start = part2.len().checked_sub(4)?;
    let sw_str = part2.get(start..)?;
    u16::from_str_radix(sw_str, 16).ok()
}

fn is_error_sw(sw: u16) -> bool {
    let high = (sw >> 8) as u8;
    !matches!(high, 0x90 | 0x91 | 0x92 | 0x9F | 0x61 | 0x62 | 0x63)
}

fn get_channel_idx_from_open_response(payload: &str) -> Option<usize> {
    let part2 = payload.split(',').nth(1)?;
    if part2.len() < 6 {
        return None;
    }
    let channel_hex = part2.get(..2)?;
    usize::from_str_radix(channel_hex, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{
            DedicatedFile, ElementaryFile, FileSystem, PinProfile, SimFile, SimIo, SimProfile,
        },
        parser::QuotedString,
        types::{FacilityLockMode, SmsMessageStatus},
    };

    fn create_test_fdn_profile(fdn_data: Vec<u8>) -> SimProfile {
        SimProfile {
            iccid: "89014103211118500720".to_string(),
            imsi: "310260123456789".to_string(),
            pin_profile: PinProfile { pin2: "5678".to_string(), ..Default::default() },
            sim_io: SimIo {
                file_system: FileSystem {
                    master_file: DedicatedFile {
                        file_id: UiccFileId::MasterFile.as_u16(),
                        files: vec![
                            SimFile::ElementaryFile(ElementaryFile {
                                file_id: UiccFileId::Iccid.as_u16(),
                                record_len: None,
                                data: hex::decode("89014103211118500720").unwrap(),
                            }),
                            SimFile::DedicatedFile(DedicatedFile {
                                file_id: UiccFileId::Telecom.as_u16(),
                                files: vec![SimFile::ElementaryFile(ElementaryFile {
                                    file_id: UiccFileId::FixedDialingNumbers.as_u16(),
                                    record_len: Some(28),
                                    data: fdn_data,
                                })],
                            }),
                        ],
                    },
                },
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_is_fdn_allowed_disabled_by_default() {
        let mut fdn_record = vec![0xFF; 28];
        fdn_record[14] = 4; // len: 3 BCD + 1 TON
        fdn_record[15] = 0x81; // TON = Unknown, NPI = E.164
        fdn_record[16] = 0x21; // '1','2'
        fdn_record[17] = 0x43; // '3','4'
        fdn_record[18] = 0xF5; // '5', filler

        let profile = create_test_fdn_profile(fdn_record);
        let mut service = SimService::new();
        service.load_profile(&profile);

        assert!(service.is_fdn_allowed(&PhoneNumber::new_for_test("98765")));
        assert!(service.is_fdn_allowed(&PhoneNumber::new_for_test("12345")));
    }

    #[test]
    fn test_is_fdn_allowed_enabled_matching() {
        let mut fdn_record = vec![0xFF; 28];
        fdn_record[14] = 4; // len
        fdn_record[15] = 0x81;
        fdn_record[16] = 0x21;
        fdn_record[17] = 0x43;
        fdn_record[18] = 0xF5; // "12345"

        let mut profile = create_test_fdn_profile(fdn_record);
        profile.pin_profile.pin2 = "5678".to_string();
        let mut service = SimService::new();
        service.load_profile(&profile);

        service.handle_set_fdn_lock(FacilityLockMode::Lock, Some(QuotedString("5678"))).unwrap();
        assert!(service.fdn_enabled);

        assert!(service.is_fdn_allowed(&PhoneNumber::new_for_test("12345")));
        assert!(service.is_fdn_allowed(&PhoneNumber::new_for_test("1234567")));
        assert!(!service.is_fdn_allowed(&PhoneNumber::new_for_test("1234")));
        assert!(!service.is_fdn_allowed(&PhoneNumber::new_for_test("98765")));
        // Normalized international dialing matches domestic FDN entry
        assert!(service.is_fdn_allowed(&PhoneNumber::new_for_test("+12345")));
        assert!(!service.is_fdn_allowed(&PhoneNumber::new_for_test("")));

        // Security bypass fix verification (characters 'a' in BCD)
        let mut fdn_record_with_a = vec![0xFF; 28];
        fdn_record_with_a[14] = 3; // len: 2 bytes BCD + 1 TON
        fdn_record_with_a[15] = 0x81;
        fdn_record_with_a[16] = 0x21; // "12"
        fdn_record_with_a[17] = 0xC3; // "3a"

        let profile_a = create_test_fdn_profile(fdn_record_with_a);
        let mut service_a = SimService::new();
        service_a.load_profile(&profile_a);
        service_a.handle_set_fdn_lock(FacilityLockMode::Lock, Some(QuotedString("5678"))).unwrap();

        assert!(!service_a.is_fdn_allowed(&PhoneNumber::new_for_test("12345")));
    }

    #[test]
    fn test_handle_set_fdn_lock_lockout() {
        let fdn_record = vec![0xFF; 28];
        let mut profile = create_test_fdn_profile(fdn_record);
        profile.pin_profile.pin2 = "5678".to_string();
        profile.pin_profile.pin2_retries = Some(3);
        let mut service = SimService::new();
        service.load_profile(&profile);

        // Try invalid length PIN2 -> fails immediately, does NOT decrement
        // retries
        let res = service.handle_set_fdn_lock(FacilityLockMode::Lock, Some(QuotedString("123")));
        assert_eq!(res.err(), Some(ExecutionResult::cme_error(CmeError::IncorrectPassword)));
        assert_eq!(service.pin2.pin_retries, 3);

        let res =
            service.handle_set_fdn_lock(FacilityLockMode::Lock, Some(QuotedString("123456789")));
        assert_eq!(res.err(), Some(ExecutionResult::cme_error(CmeError::IncorrectPassword)));
        assert_eq!(service.pin2.pin_retries, 3);

        // Try wrong PIN2 -> retries decrement
        let res = service.handle_set_fdn_lock(FacilityLockMode::Lock, Some(QuotedString("0000")));
        assert!(res.is_err());
        assert_eq!(service.pin2.pin_retries, 2);

        // Try wrong PIN2 again
        let res = service.handle_set_fdn_lock(FacilityLockMode::Lock, Some(QuotedString("0000")));
        assert!(res.is_err());
        assert_eq!(service.pin2.pin_retries, 1);

        // Try wrong PIN2 third time -> retries reach 0
        let res = service.handle_set_fdn_lock(FacilityLockMode::Lock, Some(QuotedString("0000")));
        assert!(res.is_err());
        assert_eq!(service.pin2.pin_retries, 0);

        // Try CORRECT PIN2 now that it is blocked -> should STILL fail with
        // SimPuk2Required!
        let res = service.handle_set_fdn_lock(FacilityLockMode::Lock, Some(QuotedString("5678")));
        assert_eq!(res.err(), Some(ExecutionResult::cme_error(CmeError::SimPuk2Required)));

        assert!(!service.fdn_enabled);
    }

    #[test]
    fn test_elementary_file_response_header_serialization() {
        let ef_header = ElementaryFileResponseHeader::new(0x6F40, 28, Some(28));
        let bytes = ef_header.to_bytes();
        assert_eq!(bytes.len(), 15);
        assert_eq!(bytes[0..2], [0x00, 0x00]);
        assert_eq!(bytes[2..4], [0x00, 28]);
        assert_eq!(bytes[4], 0x6F);
        assert_eq!(bytes[5], 0x40);
        assert_eq!(bytes[6], SimFileType::Elementary as u8);
        assert_eq!(bytes[7], 0x00);
        assert_eq!(bytes[8..11], [0x00, 0x00, 0x00]);
        assert_eq!(bytes[11], FileStatus::Valid as u8);
        assert_eq!(bytes[12], 0x02);
        assert_eq!(bytes[13], ElementaryFileStructure::LinearFixed as u8);
        assert_eq!(bytes[14], 28);
    }

    #[test]
    fn test_transparent_elementary_file_response_header_serialization() {
        let ef_header = ElementaryFileResponseHeader::new(0x6F07, 9, None);
        let bytes = ef_header.to_bytes();
        assert_eq!(bytes.len(), 14);
        assert_eq!(bytes[0..2], [0x00, 0x00]);
        assert_eq!(bytes[2..4], [0x00, 9]);
        assert_eq!(bytes[4], 0x6F);
        assert_eq!(bytes[5], 0x07);
        assert_eq!(bytes[6], SimFileType::Elementary as u8);
        assert_eq!(bytes[7], 0x00);
        assert_eq!(bytes[8..11], [0x00, 0x00, 0x00]);
        assert_eq!(bytes[11], FileStatus::Valid as u8);
        assert_eq!(bytes[12], 0x01);
        assert_eq!(bytes[13], ElementaryFileStructure::Transparent as u8);
    }

    #[test]
    fn test_handle_sim_io_get_response_transparent_ef_p3_15() {
        let mut sim_service = SimService::new();
        sim_service.load_profile(&SimProfile::default());
        // EF_FPLMN is 0x6F7B (28539), a transparent EF of 12 bytes.
        let resp =
            sim_service.handle_sim_io(apdu::Instruction::GetResponse, 0x6F7B, 0, 0, 15, None);
        let SimResponse::RestrictedSimAccess { sw, data } = resp.unwrap().unwrap() else {
            panic!("Expected RestrictedSimAccess");
        };
        assert_eq!(sw, SW_SUCCESS);
        let data = data.expect("Expected data");
        // Transparent EF header is strictly 14 bytes (28 hex characters)
        assert_eq!(data.len(), 28);
        assert_eq!(&data[24..26], "01"); // Byte 13: length of following data = 1
        assert_eq!(&data[26..28], "00"); // Byte 14: structure = Transparent (0)
    }

    #[test]
    fn test_access_conditions_nibble_packing() {
        let custom_ac = AccessConditions {
            read: AccessLevel::Pin1,
            update: AccessLevel::Pin2,
            increase: AccessLevel::Never,
            rehabilitate: AccessLevel::Always,
            invalidate: AccessLevel::Pin1,
        };
        let bytes = custom_ac.to_bytes();
        assert_eq!(bytes[0], 0x12); // read: 1, update: 2
        assert_eq!(bytes[1], 0xF0); // increase: F, rfu: 0
        assert_eq!(bytes[2], 0x01); // rehabilitate: 0, invalidate: 1
    }

    #[test]
    fn test_chv_status_serialization() {
        let custom_chv = ChvStatus {
            num_chvs: 2,
            chv1_status: 0x83,
            unblock_chv1_status: 0x0A,
            chv2_status: 0x03,
            unblock_chv2_status: 0x0A,
        };
        let bytes = custom_chv.to_bytes();
        assert_eq!(bytes, [2, 0x83, 0x0A, 0x03, 0x0A, 0x00, 0x00]);
    }

    #[test]
    fn test_dedicated_file_response_header_serialization() {
        let df_header = DedicatedFileResponseHeader::new(0x7F10, false, 0, 2);
        let bytes = df_header.to_bytes();
        assert_eq!(bytes.len(), 22);
        assert_eq!(bytes[0..2], [0x00, 0x00]);
        assert_eq!(bytes[2..4], [0x00, 0x00]);
        assert_eq!(bytes[4], 0x7F);
        assert_eq!(bytes[5], 0x10);
        assert_eq!(bytes[6], SimFileType::Dedicated as u8);
        assert_eq!(bytes[7..12], [0x00; 5]);
        assert_eq!(bytes[12], ClockStopPreference::NotAllowed as u8);
        assert_eq!(bytes[13], 0);
        assert_eq!(bytes[14], 2);
        assert_eq!(bytes[15..22], [0x00; 7]);
    }

    #[test]
    fn test_find_df_mut() {
        let mut mf = DedicatedFile {
            file_id: UiccFileId::MasterFile.as_u16(),
            files: vec![SimFile::DedicatedFile(DedicatedFile {
                file_id: UiccFileId::Telecom.as_u16(),
                files: vec![],
            })],
        };
        let telecom = mf.find_df_mut(UiccFileId::Telecom);
        assert!(telecom.is_some());
        assert_eq!(telecom.unwrap().file_id, UiccFileId::Telecom.as_u16());
    }

    #[test]
    fn test_handle_sim_io_get_response_truncation() {
        let profile = SimProfile::default();
        let mut service = SimService::new();
        service.load_profile(&profile);
        let res = service
            .handle_sim_io(
                apdu::Instruction::GetResponse,
                UiccFileId::Msisdn.as_u16(),
                0,
                0,
                15,
                None,
            )
            .unwrap();
        if let Some(SimResponse::RestrictedSimAccess { sw, data }) = res {
            assert_eq!(sw, SW_SUCCESS);
            assert_eq!(data.unwrap().len(), 30); // 15 bytes = 30 hex characters
        } else {
            panic!("Expected RestrictedSimAccess");
        }
    }

    #[test]
    fn test_set_msisdn_preserves_subsequent_records() {
        let profile = SimProfile::default();
        let mut service = SimService::from_profile(&profile);
        let msisdn_len = UiccFileId::Msisdn.default_record_len().expect("file id is record based");

        // Populate EF_MSISDN in DF_TELECOM with 2 records
        if let Some(telecom) = service.fs.find_df_mut(UiccFileId::Telecom)
            && let Some(ef) = telecom.find_ef_mut(UiccFileId::Msisdn)
        {
            ef.data = vec![0xAA; 2 * msisdn_len];
        }

        let new_num = PhoneNumber::new_for_test("+15555215554");
        service.set_msisdn(Some(&new_num));

        let telecom = service.fs.find_df(UiccFileId::Telecom).unwrap();
        let ef = telecom.find_ef(UiccFileId::Msisdn).unwrap();
        assert_eq!(ef.record_count(), 2);
        // Record 1 was updated with encoded MSISDN
        assert_eq!(ef.record(1).unwrap(), &AdnRecord::encode_from_number(&new_num, msisdn_len));
        // Record 2 was preserved
        assert_eq!(ef.record(2).unwrap(), &[0xAA; 28]);
    }

    #[test]
    fn test_get_msisdn_falls_back_when_first_record_empty() {
        let record_len = 28;
        // Record 1: uninitialized (all 0xFF)
        let mut data = vec![0xFF; record_len];
        // Record 2: valid phone number "+15559876543"
        let second_number = PhoneNumber::new_for_test("+15559876543");
        data.extend(AdnRecord::encode_from_number(&second_number, record_len));

        let profile = SimProfile {
            sim_io: SimIo {
                file_system: FileSystem {
                    master_file: DedicatedFile {
                        file_id: UiccFileId::MasterFile.as_u16(),
                        files: vec![SimFile::DedicatedFile(DedicatedFile {
                            file_id: UiccFileId::Telecom.as_u16(),
                            files: vec![SimFile::ElementaryFile(ElementaryFile {
                                file_id: UiccFileId::Msisdn.as_u16(),
                                record_len: Some(record_len),
                                data,
                            })],
                        })],
                    },
                },
            },
            ..Default::default()
        };
        let service = SimService::from_profile(&profile);
        assert_eq!(service.get_msisdn(), Some(second_number));
    }

    #[test]
    fn test_sim_lifecycle_hot_swap_and_removal() {
        let default_prof =
            crate::profiles::get_builtin_profile(crate::profiles::SIM_TYPE_DEFAULT).unwrap();
        let mut service = SimService::new();
        service.load_profile(&default_prof);

        assert!(service.is_present());
        assert_eq!(service.get_imsi().as_deref(), Some("310260000000000"));
        assert_eq!(service.get_cpin_urc(), Some("+CPIN: READY\r\n".to_string()));

        let meta = service.get_profile_metadata().unwrap();
        assert_eq!(meta.imsi, "310260000000000");
        assert_eq!(meta.home_plmn.as_ref().map(Plmn::as_str), Some("310260"));

        // Store an SMS to verify it gets cleared on removal
        assert!(
            service
                .store_sms(SimSmsMessage {
                    status: SmsMessageStatus::ReceivedUnread,
                    pdu: vec![1, 2, 3],
                })
                .is_ok()
        );
        assert_eq!(service.get_sms_count(), 1);

        // Test ejection (set_present(false)) vs removal (remove_sim)
        assert!(service.set_present(false));
        assert!(!service.is_present());
        assert!(service.is_provisioned());
        assert_eq!(service.get_cpin_urc(), Some("+CPIN: ABSENT\r\n".to_string()));

        // Re-inserting the ejected card restores presence
        assert!(service.set_present(true));
        assert!(service.is_present());
        assert!(service.is_provisioned());
        assert_eq!(service.get_cpin_urc(), Some("+CPIN: READY\r\n".to_string()));

        // Remove SIM (unprovisions and completely clears the card)
        assert!(service.remove_sim());
        assert!(!service.is_present());
        assert!(!service.is_provisioned());
        assert_eq!(service.get_cpin_urc(), Some("+CPIN: ABSENT\r\n".to_string()));

        // Attempting to present an unprovisioned card fails
        assert!(!service.set_present(true));

        // When SIM is unprovisioned, queries return None and SMS is empty
        assert_eq!(service.get_profile_metadata(), None);
        assert_eq!(service.get_imsi(), None);
        assert_eq!(service.get_iccid(), None);
        assert_eq!(service.home_plmn(), None);
        assert_eq!(service.get_sms_count(), 0);

        // Insert Tel Alaska profile into empty slot
        let alaska_prof =
            crate::profiles::get_builtin_profile(crate::profiles::SIM_TYPE_TEL_ALASKA).unwrap();
        assert!(service.insert_sim(&alaska_prof));
        assert!(service.is_present());
        assert!(service.is_provisioned());
        assert!(!service.insert_sim(&alaska_prof));
        assert_eq!(service.get_imsi().as_deref(), Some("311740123456789"));
        assert_eq!(service.get_iccid().as_deref(), Some("89860318640220133897"));
        assert_eq!(service.get_cpin_urc(), Some("+CPIN: READY\r\n".to_string()));

        let alaska_meta = service.get_profile_metadata().unwrap();
        assert_eq!(alaska_meta.imsi, "311740123456789");
        assert_eq!(alaska_meta.home_plmn.as_ref().map(Plmn::as_str), Some("311740"));
    }

    #[test]
    fn test_load_profile_resets_logical_channels() {
        let default_prof =
            crate::profiles::get_builtin_profile(crate::profiles::SIM_TYPE_DEFAULT).unwrap();
        let mut service = SimService::new();
        service.load_profile(&default_prof);

        // Open logical channel 1
        let res = service.handle_open_logical_channel(ApplicationId("")).unwrap();
        assert_eq!(res, Some(SimResponse::OpenLogicalChannel(1)));
        assert!(service.channels[1].open);

        // Load new profile
        let cts_prof = crate::profiles::get_builtin_profile(crate::profiles::SIM_TYPE_CTS).unwrap();
        service.load_profile(&cts_prof);

        // Logical channel 1 should be closed, channel 0 open
        assert!(service.channels[0].open);
        assert!(!service.channels[1].open);
        assert_eq!(service.channels[1].selected_aid, None);
        assert_eq!(service.channels[1].selected_file, None);
    }

    #[test]
    fn test_load_profile_clears_sms_messages() {
        let default_prof =
            crate::profiles::get_builtin_profile(crate::profiles::SIM_TYPE_DEFAULT).unwrap();
        let mut service = SimService::new();
        service.load_profile(&default_prof);

        // Store an SMS message on the SIM card
        let dummy_pdu = vec![0x00, 0x01, 0x02, 0x03];
        let index = service
            .store_sms(SimSmsMessage { status: SmsMessageStatus::ReceivedUnread, pdu: dummy_pdu });
        assert_eq!(index, Ok(1));
        assert_eq!(service.get_sms_count(), 1);

        // Switching or reloading profile should clear stored SMS records
        let alaska_prof =
            crate::profiles::get_builtin_profile(crate::profiles::SIM_TYPE_TEL_ALASKA).unwrap();
        service.load_profile(&alaska_prof);
        assert_eq!(service.get_sms_count(), 0);
    }

    #[test]
    fn test_sim_sms_slot_recycling() {
        let default_prof =
            crate::profiles::get_builtin_profile(crate::profiles::SIM_TYPE_DEFAULT).unwrap();
        let mut service = SimService::new();
        service.load_profile(&default_prof);

        let make_msg =
            || SimSmsMessage { status: SmsMessageStatus::ReceivedUnread, pdu: vec![0x00, 0x01] };

        // Store 3 messages: slots 1, 2, 3
        assert_eq!(service.store_sms(make_msg()), Ok(1));
        assert_eq!(service.store_sms(make_msg()), Ok(2));
        assert_eq!(service.store_sms(make_msg()), Ok(3));
        assert_eq!(service.get_sms_count(), 3);

        // Delete slot 2
        assert!(service.delete_sms(2).is_ok());
        assert_eq!(service.get_sms_count(), 2);

        // Next store should reuse slot 2 (lowest available)
        assert_eq!(service.store_sms(make_msg()), Ok(2));

        // Subsequent store should take slot 4
        assert_eq!(service.store_sms(make_msg()), Ok(4));
        assert_eq!(service.get_sms_count(), 4);
    }

    #[test]
    fn test_set_present_false_resets_channels_and_buffers() {
        let default_prof =
            crate::profiles::get_builtin_profile(crate::profiles::SIM_TYPE_DEFAULT).unwrap();
        let mut service = SimService::new();
        service.load_profile(&default_prof);

        // Open logical channel 1 and buffer response data
        let res = service.handle_open_logical_channel(ApplicationId("")).unwrap();
        assert_eq!(res, Some(SimResponse::OpenLogicalChannel(1)));
        assert!(service.channels[1].open);
        service.channels[1].response_buffer = vec![0x12, 0x34];

        // Toggling SIM presence to false must tear down logical channels, reset
        // selected files, and clear buffers
        assert!(service.set_present(false));
        assert!(!service.is_present());
        assert_eq!(service.channels, default_channels());
        assert!(service.channels[1].response_buffer.is_empty());
    }

    #[test]
    fn test_pin_entry_methods_and_custom_retries() {
        let mut entry = PinEntry::new("4321", "87654321", 5, 8);
        assert_eq!(entry.pin_attempts(), (5, 5));
        assert_eq!(entry.puk_attempts(), (8, 8));

        // Invalid length PIN
        assert_eq!(entry.verify_pin("12"), Err(PinVerifyError::IncorrectPassword));
        assert_eq!(entry.pin_retries, 5); // Length error does not decrement

        // Non-ASCII PIN rejected without retry decrement
        assert_eq!(entry.verify_pin("123\u{1F980}"), Err(PinVerifyError::IncorrectPassword));
        assert_eq!(entry.pin_retries, 5);

        // Mismatched PIN decrements
        assert_eq!(entry.verify_pin("0000"), Err(PinVerifyError::IncorrectPassword));
        assert_eq!(entry.pin_retries, 4);

        // Matching PIN resets to max_pin_retries (5, not default 3)
        assert_eq!(entry.verify_pin("4321"), Ok(()));
        assert_eq!(entry.pin_retries, 5);

        // Invalid length candidate rejected without state corruption
        assert_eq!(entry.change_pin("4321", "12"), Err(PinVerifyError::IncorrectPassword));
        assert_eq!(entry.pin, "4321");

        // Non-ASCII candidate rejected without state corruption
        assert_eq!(
            entry.change_pin("4321", "123\u{1F980}"),
            Err(PinVerifyError::IncorrectPassword)
        );
        assert_eq!(entry.pin, "4321");

        // Change PIN
        assert_eq!(entry.change_pin("4321", "9999"), Ok(()));
        assert_eq!(entry.pin, "9999");
        assert_eq!(entry.pin_retries, 5);

        // Unblock with PUK
        entry.pin_retries = 0;
        assert!(entry.is_blocked());
        assert_eq!(entry.verify_pin("4321"), Err(PinVerifyError::Blocked));
        assert_eq!(entry.unblock_with_puk("87654321", "12"), Err(CmeError::IncorrectPassword));
        assert_eq!(
            entry.unblock_with_puk("8765432\u{1F980}", "1111"),
            Err(CmeError::IncorrectPassword)
        );
        assert_eq!(
            entry.unblock_with_puk("87654321", "123\u{1F980}"),
            Err(CmeError::IncorrectPassword)
        );
        assert_eq!(entry.unblock_with_puk("87654321", "1111"), Ok(()));
        assert_eq!(entry.pin, "1111");
        assert_eq!(entry.pin_attempts(), (5, 5));
        assert_eq!(entry.puk_attempts(), (8, 8));
    }
}
