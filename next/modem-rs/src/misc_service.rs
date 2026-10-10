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
    fmt::{Display, Formatter},
    str::FromStr,
    sync::Arc,
};

use jiff::{Zoned, civil::DateTime, tz::TimeZone};
use modem_rs_derive::CommandParser;
use nom::IResult;

use crate::{
    parser::QuotedString,
    time::{Clock, SystemClock},
    types::{
        CallMode, CharacterSet, CmeeMode, ExecutionResult, FlowControlMode, IcfFormat, IcfParity,
        Parsable, ProductSerialNumberType, SpeakerMuteMode, SpeakerVolume,
    },
};

const DEFAULT_IMEI: &str = "867400022047199";
const DEFAULT_SVN: &str = "01";
const DEFAULT_INFO: &str = "modem simulator";

/// Miscellaneous modem service AT commands.
#[derive(Debug, PartialEq, Clone, Copy, CommandParser)]
pub enum MiscCommand {
    #[command(tag = "AT+CMEE=?")]
    QuerySupportedReportMobileEquipmentError,
    #[command(tag = "AT+CMEE?")]
    QueryReportMobileEquipmentError,
    #[command(tag = "AT+CMEE=")]
    SetReportMobileEquipmentError(CmeeMode),
    /// Goldfish specific concatenated init command
    #[command(tag = "ATE0Q0V1")]
    GoldfishInitSequence,
    #[command(tag = "ATE")]
    SetEcho(bool),
    #[command(tag = "ATL")]
    SetSpeakerVolume(SpeakerVolume),
    #[command(tag = "ATM")]
    SetSpeakerMute(SpeakerMuteMode),
    #[command(tag = "ATQ")]
    SetQuietMode(bool),
    #[command(tag = "ATV")]
    SetVerboseMode(bool),
    #[command(tag = "AT&F")]
    ResetToFactoryDefaults,
    #[command(tag = "AT&V")]
    ViewActiveConfiguration,
    #[command(tag = "AT&W")]
    WriteActiveConfiguration,
    #[command(tag = "ATZ")]
    Reset,
    #[command(tag = "ATI")]
    GetIdentificationInformation,
    #[command(tag = "ATS0=")]
    SetAutoAnswer(u8),
    #[command(tag = "ATS3=")]
    SetCommandTerminationCharacter(u8),
    #[command(tag = "ATS4=")]
    SetResponseFormattingCharacter(u8),
    #[command(tag = "ATS5=")]
    SetCommandLineEditingCharacter(u8),
    #[command(tag = "ATS6=")]
    SetPauseBeforeBlindDialing(u8),
    #[command(tag = "ATS7=")]
    SetConnectionCompletionTimeout(u8),
    #[command(tag = "ATS8=")]
    SetCommaDialModifierTime(u8),
    #[command(tag = "ATS10=")]
    SetAutomaticDisconnectDelay(u8),
    #[command(tag = "AT+GCAP")]
    GetCapabilities,
    #[command(tag = "AT+GMI")]
    GetManufacturerIdentification,
    #[command(tag = "AT+GMM")]
    GetModelId,
    #[command(tag = "AT+GMR")]
    GetRevision,
    #[command(tag = "AT+GSN")]
    GetSerialNumber,
    #[command(tag = "AT+CGSN=")]
    GetProductSerialNumberGsmWithType(ProductSerialNumberType),
    #[command(tag = "AT+CGSN")]
    GetProductSerialNumberGsm,
    #[command(tag = "AT+ICF=")]
    SetTeTaControlCharacterFraming(IcfFormat, Option<IcfParity>),
    #[command(tag = "AT+IFC=")]
    SetTeTaLocalDataFlowControl(FlowControlMode, Option<FlowControlMode>),
    #[command(tag = "AT+IPR=")]
    SetTeTaFixedLocalRate(u32),
    #[command(tag = "AT+CCLK=")]
    SetTime(CclkTime),
    #[command(tag = "AT+CCLK?")]
    QueryTime,
    #[command(tag = "AT%CTZV=")]
    SetCtzv(bool),
    #[command(tag = "AT%CTZV?")]
    QueryCtzv,
    #[command(tag = "AT+CMOD=")]
    SetCallMode(CallMode),
    #[command(tag = "AT+CSCS=")]
    SetCharacterSet(CharacterSet),
    #[command(tag = "AT")]
    Test,
}

/// Network Identity and Time Zone (NITZ) unsolicited result (%CTZV, 3GPP TS
/// 22.042 / TS 24.008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NitzTime(pub Zoned);

impl Display for NitzTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let utc = self.0.with_time_zone(TimeZone::UTC);
        let offset = self.0.offset().seconds() / 900;
        let sign = if offset >= 0 { '+' } else { '-' };
        let is_dst = if self.0.time_zone().to_offset_info(self.0.timestamp()).dst().is_dst() {
            1
        } else {
            0
        };
        write!(
            f,
            "{}{sign}{:02}:{is_dst}",
            utc.strftime("%y/%m/%d:%H:%M:%S"),
            offset.unsigned_abs(),
        )?;
        if let Some(tz_name) = self.0.time_zone().iana_name() {
            write!(f, ":{}", tz_name.replace('/', "!"))?;
        }
        Ok(())
    }
}

impl From<Zoned> for NitzTime {
    fn from(now: Zoned) -> Self {
        Self(now)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CclkTime {
    pub datetime: DateTime,
    pub offset_quarter_hours: Option<i8>,
}

impl Display for CclkTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.datetime.strftime("%y/%m/%d,%H:%M:%S"))?;
        if let Some(offset) = self.offset_quarter_hours {
            let sign = if offset >= 0 { '+' } else { '-' };
            write!(f, "{sign}{:02}", offset.unsigned_abs())?;
        }
        Ok(())
    }
}

impl From<Zoned> for CclkTime {
    fn from(now: Zoned) -> Self {
        let offset_quarter_hours = (now.offset().seconds() / 900) as i8;
        let utc = now.with_time_zone(TimeZone::UTC).datetime();
        Self { datetime: utc, offset_quarter_hours: Some(offset_quarter_hours) }
    }
}

impl From<NitzTime> for CclkTime {
    fn from(nitz: NitzTime) -> Self {
        Self::from(nitz.0)
    }
}

impl FromStr for CclkTime {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 3GPP TS 27.007 §8.15: "yy/MM/dd,hh:mm:ss[+/-zz]"
        let dt_str = s.get(..17).ok_or("timestamp too short")?;
        let datetime = DateTime::strptime("%y/%m/%d,%H:%M:%S", dt_str)
            .map_err(|_e| "invalid datetime format")?;
        let offset_quarter_hours = match s.get(17..) {
            None | Some("") => None,
            Some(tz) => {
                let offset = tz.parse::<i8>().map_err(|_e| "invalid timezone offset")?;
                // 3GPP TS 27.007 §8.15: Time zone offset in 15-minute intervals (-96..=96, max
                // ±24h)
                if !(-96..=96).contains(&offset) {
                    return Err("timezone offset out of range");
                }
                Some(offset)
            }
        };
        Ok(Self { datetime, offset_quarter_hours })
    }
}

impl<'a> Parsable<'a> for CclkTime {
    fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (rem, quoted) = QuotedString::parse(input)?;
        let cclk = quoted.as_str().parse::<Self>().map_err(|_e| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Verify))
        })?;
        Ok((rem, cclk))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MiscResponse {
    Clock(CclkTime),
    CtzvMode(bool),
    TimeUpdate(NitzTime),
    ModelId(String),
    Revision(String),
    SerialNumber(String),
    ProductSerialNumberGsm(ProductSerialNumberType),
    ErrorReportingMode(CmeeMode),
    ErrorReportingSupported,
    ActiveConfiguration {
        speaker_volume: SpeakerVolume,
        speaker_mute: SpeakerMuteMode,
        quiet_mode: bool,
        verbose_mode: bool,
        icf_format: IcfFormat,
        icf_parity: IcfParity,
        ifc_dce: FlowControlMode,
        ifc_dte: FlowControlMode,
    },
    ManufacturerIdentification(String),
    Capabilities,
}

impl Display for MiscResponse {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            MiscResponse::Clock(clock) => write!(f, "+CCLK: \"{clock}\"\r\n"),
            MiscResponse::CtzvMode(mode) => write!(f, "%CTZV: {}\r\n", if *mode { 1 } else { 0 }),
            MiscResponse::TimeUpdate(nitz) => write!(f, "%CTZV: {nitz}\r\n"),
            MiscResponse::ModelId(model_id) => write!(f, "{model_id}\r\n"),
            MiscResponse::Revision(revision) => write!(f, "{revision}\r\n"),
            MiscResponse::SerialNumber(serial_number) => write!(f, "{serial_number}\r\n"),
            MiscResponse::ProductSerialNumberGsm(snt) => match snt {
                ProductSerialNumberType::ImeiWithInfo => {
                    write!(f, "{DEFAULT_IMEI}{DEFAULT_INFO}\r\n")
                }
                ProductSerialNumberType::Imei => write!(f, "{DEFAULT_IMEI}\r\n"),
                ProductSerialNumberType::ImeiWithSvn => {
                    write!(f, "{DEFAULT_IMEI}{DEFAULT_SVN}\r\n")
                }
                ProductSerialNumberType::Svn => write!(f, "{DEFAULT_SVN}\r\n"),
            },
            MiscResponse::ErrorReportingMode(mode) => write!(f, "+CMEE: {mode}\r\n"),
            MiscResponse::ErrorReportingSupported => write!(f, "+CMEE: (0-2)\r\n"),
            MiscResponse::ActiveConfiguration {
                speaker_volume,
                speaker_mute,
                quiet_mode,
                verbose_mode,
                icf_format,
                icf_parity,
                ifc_dce,
                ifc_dte,
            } => {
                let quiet = if *quiet_mode { 1 } else { 0 };
                let verbose = if *verbose_mode { 1 } else { 0 };
                write!(
                    f,
                    "ACTIVE PROFILE:\r\nL:{speaker_volume} M:{speaker_mute} Q:{quiet} V:{verbose} ICF:{icf_format},{icf_parity} IFC:{ifc_dce},{ifc_dte}\r\n"
                )
            }
            MiscResponse::ManufacturerIdentification(manufacturer) => {
                write!(f, "{manufacturer}\r\n")
            }
            MiscResponse::Capabilities => write!(f, "+GCAP: +FCLASS,+DS\r\n"),
        }
    }
}

type MiscResult = Result<Option<MiscResponse>, ExecutionResult>;

pub struct MiscService {
    clock: Arc<dyn Clock>,
    ctzv_mode: bool,
    cclk: Option<CclkTime>,
    cmee_mode: CmeeMode,
    speaker_volume: SpeakerVolume,
    speaker_mute: SpeakerMuteMode,
    quiet_mode: bool,
    verbose_mode: bool,
    icf_format: IcfFormat,
    icf_parity: IcfParity,
    ifc_dce: FlowControlMode,
    ifc_dte: FlowControlMode,
}

impl Default for MiscService {
    fn default() -> Self {
        Self::new(Arc::new(SystemClock))
    }
}

impl MiscService {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            ctzv_mode: false,
            cclk: None,
            cmee_mode: CmeeMode::default(),
            speaker_volume: SpeakerVolume::default(),
            speaker_mute: SpeakerMuteMode::default(),
            quiet_mode: false,
            verbose_mode: true,
            icf_format: IcfFormat::default(),
            icf_parity: IcfParity::default(),
            ifc_dce: FlowControlMode::default(),
            ifc_dte: FlowControlMode::default(),
        }
    }

    pub fn cmee_mode(&self) -> CmeeMode {
        self.cmee_mode
    }

    pub fn set_ctzv_mode(&mut self, enabled: bool) {
        self.ctzv_mode = enabled;
    }

    pub fn ctzv_enabled(&self) -> bool {
        self.ctzv_mode
    }

    pub fn current_time_update(&self) -> MiscResponse {
        MiscResponse::TimeUpdate(NitzTime::from(self.clock.now_zoned()))
    }

    fn handle_set_ctzv(&mut self, mode: bool) -> MiscResult {
        self.ctzv_mode = mode;
        Ok(None)
    }

    fn handle_query_ctzv(&self) -> MiscResult {
        Ok(Some(MiscResponse::CtzvMode(self.ctzv_mode)))
    }

    // --- Pure command handlers ---

    fn handle_set_time(&mut self, time: CclkTime) -> MiscResult {
        self.cclk = Some(time);
        Ok(None)
    }

    fn handle_query_time(&self) -> MiscResult {
        let cclk = match self.cclk {
            Some(time) => time,
            None => CclkTime::from(self.clock.now_zoned()),
        };
        Ok(Some(MiscResponse::Clock(cclk)))
    }

    fn handle_get_model_id(&self) -> MiscResult {
        Ok(Some(MiscResponse::ModelId("gLinux".to_string())))
    }

    fn handle_get_revision(&self) -> MiscResult {
        Ok(Some(MiscResponse::Revision("1.0".to_string())))
    }

    fn handle_get_serial_number(&self) -> MiscResult {
        Ok(Some(MiscResponse::SerialNumber("0123456789".to_string())))
    }

    fn handle_get_product_serial_number_gsm(&self) -> MiscResult {
        Ok(Some(MiscResponse::ProductSerialNumberGsm(ProductSerialNumberType::Imei)))
    }

    fn handle_get_product_serial_number_gsm_with_type(
        &self,
        snt: ProductSerialNumberType,
    ) -> MiscResult {
        Ok(Some(MiscResponse::ProductSerialNumberGsm(snt)))
    }

    fn handle_set_icf(&mut self, format: IcfFormat, parity: Option<IcfParity>) -> MiscResult {
        self.icf_format = format;
        if let Some(p) = parity {
            self.icf_parity = p;
        }
        Ok(None)
    }

    fn handle_set_ifc(&mut self, dce: FlowControlMode, dte: Option<FlowControlMode>) -> MiscResult {
        self.ifc_dce = dce;
        if let Some(d) = dte {
            self.ifc_dte = d;
        }
        Ok(None)
    }

    fn handle_set_report_mobile_equipment_error(&mut self, mode: CmeeMode) -> MiscResult {
        self.cmee_mode = mode;
        Ok(None)
    }

    fn handle_query_report_mobile_equipment_error(&self) -> MiscResult {
        Ok(Some(MiscResponse::ErrorReportingMode(self.cmee_mode)))
    }

    fn handle_query_supported_report_mobile_equipment_error(&self) -> MiscResult {
        Ok(Some(MiscResponse::ErrorReportingSupported))
    }

    fn handle_goldfish_init_sequence(&mut self) -> MiscResult {
        self.quiet_mode = false;
        self.verbose_mode = true;
        Ok(None)
    }

    fn handle_reset_to_factory_defaults(&mut self) -> MiscResult {
        self.cmee_mode = CmeeMode::default();
        self.speaker_volume = SpeakerVolume::default();
        self.speaker_mute = SpeakerMuteMode::default();
        self.quiet_mode = false;
        self.verbose_mode = true;
        self.icf_format = IcfFormat::default();
        self.icf_parity = IcfParity::default();
        self.ifc_dce = FlowControlMode::default();
        self.ifc_dte = FlowControlMode::default();
        Ok(None)
    }

    fn handle_view_active_configuration(&self) -> MiscResult {
        Ok(Some(MiscResponse::ActiveConfiguration {
            speaker_volume: self.speaker_volume,
            speaker_mute: self.speaker_mute,
            quiet_mode: self.quiet_mode,
            verbose_mode: self.verbose_mode,
            icf_format: self.icf_format,
            icf_parity: self.icf_parity,
            ifc_dce: self.ifc_dce,
            ifc_dte: self.ifc_dte,
        }))
    }

    fn handle_get_manufacturer_identification(&self) -> MiscResult {
        Ok(Some(MiscResponse::ManufacturerIdentification("Android".to_string())))
    }

    fn handle_get_capabilities(&self) -> MiscResult {
        Ok(Some(MiscResponse::Capabilities))
    }

    pub fn execute(&mut self, command: &MiscCommand) -> ExecutionResult {
        let misc_result = match command {
            MiscCommand::GetManufacturerIdentification => {
                self.handle_get_manufacturer_identification()
            }
            MiscCommand::GetCapabilities => self.handle_get_capabilities(),
            MiscCommand::GetModelId => self.handle_get_model_id(),
            MiscCommand::GetRevision => self.handle_get_revision(),
            MiscCommand::GetSerialNumber => self.handle_get_serial_number(),
            MiscCommand::GetProductSerialNumberGsm => self.handle_get_product_serial_number_gsm(),
            MiscCommand::GetProductSerialNumberGsmWithType(snt) => {
                self.handle_get_product_serial_number_gsm_with_type(*snt)
            }
            MiscCommand::SetTeTaControlCharacterFraming(f, p) => self.handle_set_icf(*f, *p),
            MiscCommand::SetTeTaLocalDataFlowControl(d1, d2) => self.handle_set_ifc(*d1, *d2),
            MiscCommand::SetTeTaFixedLocalRate(_) => Ok(None),
            MiscCommand::SetTime(time) => self.handle_set_time(*time),
            MiscCommand::QueryTime => self.handle_query_time(),
            MiscCommand::SetCtzv(mode) => self.handle_set_ctzv(*mode),
            MiscCommand::QueryCtzv => self.handle_query_ctzv(),
            MiscCommand::SetReportMobileEquipmentError(mode) => {
                self.handle_set_report_mobile_equipment_error(*mode)
            }
            MiscCommand::QueryReportMobileEquipmentError => {
                self.handle_query_report_mobile_equipment_error()
            }
            MiscCommand::QuerySupportedReportMobileEquipmentError => {
                self.handle_query_supported_report_mobile_equipment_error()
            }
            MiscCommand::GoldfishInitSequence => self.handle_goldfish_init_sequence(),
            MiscCommand::SetSpeakerVolume(vol) => {
                self.speaker_volume = *vol;
                Ok(None)
            }
            MiscCommand::SetSpeakerMute(mute) => {
                self.speaker_mute = *mute;
                Ok(None)
            }
            MiscCommand::SetQuietMode(quiet) => {
                self.quiet_mode = *quiet;
                Ok(None)
            }
            MiscCommand::SetVerboseMode(verbose) => {
                self.verbose_mode = *verbose;
                Ok(None)
            }
            MiscCommand::ResetToFactoryDefaults => self.handle_reset_to_factory_defaults(),
            MiscCommand::ViewActiveConfiguration => self.handle_view_active_configuration(),
            MiscCommand::WriteActiveConfiguration
            | MiscCommand::Reset
            | MiscCommand::GetIdentificationInformation
            | MiscCommand::SetAutoAnswer(_)
            | MiscCommand::SetCommandTerminationCharacter(_)
            | MiscCommand::SetResponseFormattingCharacter(_)
            | MiscCommand::SetCommandLineEditingCharacter(_)
            | MiscCommand::SetPauseBeforeBlindDialing(_)
            | MiscCommand::SetConnectionCompletionTimeout(_)
            | MiscCommand::SetCommaDialModifierTime(_)
            | MiscCommand::SetAutomaticDisconnectDelay(_)
            | MiscCommand::SetCallMode(_)
            | MiscCommand::SetCharacterSet(_)
            | MiscCommand::SetEcho(_)
            | MiscCommand::Test => Ok(None),
        };

        misc_result.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::MockClock;

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_nitz_time_formatting() {
        let zoned: Zoned = "2026-06-15T12:30:45-07:00[America/Los_Angeles]".parse().unwrap();
        let nitz = NitzTime::from(zoned);
        assert_eq!(nitz.to_string(), "26/06/15:19:30:45-28:1:America!Los_Angeles");
        assert_eq!(
            MiscResponse::TimeUpdate(nitz).to_string(),
            "%CTZV: 26/06/15:19:30:45-28:1:America!Los_Angeles\r\n"
        );

        let offset = jiff::tz::Offset::from_hours(-7).unwrap();
        let zoned_no_iana = DateTime::new(2026, 6, 15, 12, 30, 45, 0)
            .unwrap()
            .to_zoned(offset.to_time_zone())
            .unwrap();
        let nitz_no_iana = NitzTime::from(zoned_no_iana);
        assert_eq!(nitz_no_iana.to_string(), "26/06/15:19:30:45-28:0");
        assert_eq!(
            MiscResponse::TimeUpdate(nitz_no_iana).to_string(),
            "%CTZV: 26/06/15:19:30:45-28:0\r\n"
        );
    }

    #[test]
    fn test_ctzv_at_commands() {
        let clock = Arc::new(MockClock::default());
        let mut service = MiscService::new(clock.clone());

        assert!(!service.ctzv_enabled());

        // AT%CTZV?
        let res = service.execute(&MiscCommand::QueryCtzv);
        if let ExecutionResult::Success(handled) = res {
            assert_eq!(
                handled.responses,
                vec![
                    crate::types::Response::Misc(MiscResponse::CtzvMode(false)),
                    crate::types::Response::Ok,
                ]
            );
        } else {
            panic!("Expected Success for QueryCtzv");
        }

        // AT%CTZV=1
        let res = service.execute(&MiscCommand::SetCtzv(true));
        assert!(matches!(res, ExecutionResult::Success(_)));
        assert!(service.ctzv_enabled());

        // AT%CTZV?
        let res = service.execute(&MiscCommand::QueryCtzv);
        if let ExecutionResult::Success(handled) = res {
            assert_eq!(
                handled.responses,
                vec![
                    crate::types::Response::Misc(MiscResponse::CtzvMode(true)),
                    crate::types::Response::Ok,
                ]
            );
        } else {
            panic!("Expected Success for QueryCtzv");
        }

        // AT%CTZV=0
        let res = service.execute(&MiscCommand::SetCtzv(false));
        assert!(matches!(res, ExecutionResult::Success(_)));
        assert!(!service.ctzv_enabled());
    }

    #[test]
    fn test_cclk_time_parsing_and_display() {
        let (rem, cclk) = CclkTime::parse(b"\"25/08/02,12:30:00+00\"").unwrap();
        assert_eq!(rem, b"");
        assert_eq!(cclk.to_string(), "25/08/02,12:30:00+00");

        let (_, cclk_no_tz) = CclkTime::parse(b"\"25/08/02,12:30:00\"").unwrap();
        assert_eq!(cclk_no_tz.to_string(), "25/08/02,12:30:00");

        // Verify out-of-range offsets are rejected (e.g. -128 i8::MIN, +100, -97)
        assert!(CclkTime::parse(b"\"25/08/02,12:30:00-128\"").is_err());
        assert!(CclkTime::parse(b"\"25/08/02,12:30:00+100\"").is_err());
        assert!(CclkTime::parse(b"\"25/08/02,12:30:00-97\"").is_err());

        // Verify that formatting i8::MIN does not panic and uses unsigned_abs()
        let min_cclk = CclkTime {
            datetime: DateTime::new(2025, 8, 2, 12, 30, 0, 0).unwrap(),
            offset_quarter_hours: Some(i8::MIN),
        };
        assert_eq!(min_cclk.to_string(), "25/08/02,12:30:00-128");
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_cclk_at_commands() {
        let clock = Arc::new(MockClock::default());
        let zoned: Zoned = "2026-06-15T12:30:45-07:00[America/Los_Angeles]".parse().unwrap();
        clock.set_zoned(zoned);
        let mut service = MiscService::new(clock.clone());

        // Default AT+CCLK? queries clock
        let res = service.execute(&MiscCommand::QueryTime);
        if let ExecutionResult::Success(handled) = res {
            assert_eq!(
                handled.responses,
                vec![
                    crate::types::Response::Misc(MiscResponse::Clock(CclkTime {
                        datetime: DateTime::new(2026, 6, 15, 19, 30, 45, 0).unwrap(),
                        offset_quarter_hours: Some(-28),
                    })),
                    crate::types::Response::Ok,
                ]
            );
        } else {
            panic!("Expected Success for QueryTime");
        }

        // AT+CCLK="25/08/02,12:30:00+00"
        let manual_time = CclkTime {
            datetime: DateTime::new(2025, 8, 2, 12, 30, 0, 0).unwrap(),
            offset_quarter_hours: Some(0),
        };
        let res = service.execute(&MiscCommand::SetTime(manual_time));
        assert!(matches!(res, ExecutionResult::Success(_)));

        // AT+CCLK? returns manually set time
        let res = service.execute(&MiscCommand::QueryTime);
        if let ExecutionResult::Success(handled) = res {
            assert_eq!(
                handled.responses,
                vec![
                    crate::types::Response::Misc(MiscResponse::Clock(manual_time)),
                    crate::types::Response::Ok,
                ]
            );
        } else {
            panic!("Expected Success for QueryTime after SetTime");
        }
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_from_nitz_to_cclk() {
        let zoned: Zoned = "2026-06-15T12:30:45-07:00[America/Los_Angeles]".parse().unwrap();
        let nitz = NitzTime::from(zoned);
        let cclk = CclkTime::from(nitz);
        assert_eq!(cclk.to_string(), "26/06/15,19:30:45-28");
    }

    #[test]
    fn test_reset_to_factory_defaults() {
        let mut service = MiscService::default();
        service.cmee_mode = CmeeMode::Verbose;
        service.speaker_volume = SpeakerVolume::High;
        service.speaker_mute = SpeakerMuteMode::AlwaysOn;
        service.icf_format = IcfFormat::Data7Stop1;
        service.icf_parity = IcfParity::Odd;
        service.ifc_dce = FlowControlMode::None;
        service.ifc_dte = FlowControlMode::None;

        service.execute(&MiscCommand::ResetToFactoryDefaults);

        assert_eq!(service.cmee_mode, CmeeMode::default());
        assert_eq!(service.speaker_volume, SpeakerVolume::default());
        assert_eq!(service.speaker_mute, SpeakerMuteMode::default());
        assert_eq!(service.icf_format, IcfFormat::default());
        assert_eq!(service.icf_parity, IcfParity::default());
        assert_eq!(service.ifc_dce, FlowControlMode::default());
        assert_eq!(service.ifc_dte, FlowControlMode::default());
    }
}
