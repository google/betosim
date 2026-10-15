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

use modem_rs::{
    parser::{QuotedString, parse_raw_data, parse_until_semicolon},
    types::Parsable,
};
use modem_rs_derive::CommandParser;

// This Command enum is a direct translation of the one in
// `modem-rs/src/parser.rs`, adapted to use the CommandParser derive macro.
#[derive(CommandParser, Debug, PartialEq)]
pub enum Command<'a> {
    #[command(tag = "AT+CPIN?")]
    GetSimStatus,
    #[command(tag = "AT+CPIN=")]
    SetPin(QuotedString<'a>),
    #[command(tag = "AT+CLCK=")]
    SetFacilityLock(QuotedString<'a>, u8, QuotedString<'a>),
    #[command(tag = "AT+CRSM=")]
    SimIo(#[parser(parse_raw_data)] &'a [u8]),
    #[command(tag = "AT+CIMI")]
    GetImsi,
    #[command(tag = "AT+CICCID")]
    GetIccid,
    #[command(tag = "AT+CCHO=")]
    OpenLogicalChannel(#[parser(parse_raw_data)] &'a [u8]),
    #[command(tag = "AT+CCHC=")]
    CloseLogicalChannel(u8),
    #[command(tag = "AT+CGLA=")]
    TransmitLogicalChannel(u8, u8, #[parser(parse_raw_data)] &'a [u8]),
    #[command(tag = "AT+CPWD=")]
    ChangePassword(QuotedString<'a>, QuotedString<'a>, QuotedString<'a>),
    #[command(tag = "AT+CPINR=")]
    QueryPinRetries(QuotedString<'a>),
    #[command(tag = "AT+CCSS=")]
    SetCdmaSubscriptionSource(u8),
    #[command(tag = "AT+WRMP=")]
    SetCdmaRoamingPreference(u8),
    #[command(tag = "AT+MBAU=")]
    SimAuthentication(#[parser(parse_raw_data)] &'a [u8]),
    #[command(tag = "AT+CCFC=")]
    CallForwarding { reason: u8, mode: u8, number: Option<QuotedString<'a>>, type_: Option<u8> },
    #[command(tag = "ATD")]
    Dial(#[parser(parse_until_semicolon)] &'a [u8]),
    #[command(tag = "ATA")]
    Answer,
    #[command(tag = "ATH")]
    Hangup,
    #[command(tag = "AT+CLCC")]
    QueryCurrentCalls,
    #[command(tag = "AT+CMUT=")]
    SetMute(u8),
    #[command(tag = "AT+CMUT?")]
    QueryMute,
    #[command(tag = "AT+VTS=")]
    SendDtmf(#[parser(parse_raw_data)] &'a [u8]),
    #[command(tag = "AT+CUSD=")]
    CancelUssd,
    #[command(tag = "AT+WSOS=")]
    SetEmergencyMode(u8),
    #[command(tag = "AT+WSOS?")]
    QueryEmergencyMode,
    #[command(tag = "AT+COPS?")]
    QueryOperator,
    #[command(tag = "AT+CSQ")]
    QuerySignalStrength,
    #[command(tag = "AT+CMGS=")]
    SendSms(u8),
    #[command(tag = "AT+CNMA")]
    SendSmsAck,
    #[command(tag = "AT+CUSATD?")]
    QueryStkReady,
    #[command(tag = "AT+CUSATE=")]
    SendStkEnvelopeCommand(QuotedString<'a>),
    #[command(tag = "AT+CLIR?")]
    QueryClir,
    #[command(tag = "ATE")]
    SetEcho(u8),
    #[command(tag = "ATL")]
    SetSpeakerVolume(u8),
    #[command(tag = "ATM")]
    SetSpeakerMute(u8),
    #[command(tag = "ATQ")]
    SetQuietMode(u8),
    #[command(tag = "ATV")]
    SetVerboseMode(u8),
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
}

fn main() {
    // Not used when running `cargo test`
}

#[test]
fn test_parse_cpin_query() {
    let (rem, cmd) = Command::parse(b"AT+CPIN?").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::GetSimStatus);
}

#[test]
fn test_parse_cpin_set() {
    let (rem, cmd) = Command::parse(b"AT+CPIN=\"1234\"").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetPin(QuotedString(b"1234")));
}

#[test]
fn test_parse_clck() {
    let (rem, cmd) = Command::parse(b"AT+CLCK=\"SC\",1,\"1234\"").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetFacilityLock(QuotedString(b"SC"), 1, QuotedString(b"1234")));
}

#[test]
fn test_parse_crsm() {
    let (rem, cmd) = Command::parse(b"AT+CRSM=176,28480,0,0,7").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SimIo(b"176,28480,0,0,7"));
}

#[test]
fn test_parse_cimi() {
    let (rem, cmd) = Command::parse(b"AT+CIMI").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::GetImsi);
}

#[test]
fn test_parse_ciccid() {
    let (rem, cmd) = Command::parse(b"AT+CICCID").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::GetIccid);
}

#[test]
fn test_parse_ccho() {
    let (rem, cmd) = Command::parse(b"AT+CCHO=\"1234\"").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::OpenLogicalChannel(b"\"1234\""));
}

#[test]
fn test_parse_cchc() {
    let (rem, cmd) = Command::parse(b"AT+CCHC=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::CloseLogicalChannel(1));
}

#[test]
fn test_parse_cgla() {
    let (rem, cmd) = Command::parse(b"AT+CGLA=1,2,3").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::TransmitLogicalChannel(1, 2, b"3"));
}

#[test]
fn test_parse_cpwd() {
    let (rem, cmd) = Command::parse(b"AT+CPWD=\"SC\",\"1234\",\"5678\"").unwrap();
    assert!(rem.is_empty());
    assert_eq!(
        cmd,
        Command::ChangePassword(QuotedString(b"SC"), QuotedString(b"1234"), QuotedString(b"5678"))
    );
}

#[test]
fn test_parse_cpinr() {
    let (rem, cmd) = Command::parse(b"AT+CPINR=\"SIM PIN\"").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::QueryPinRetries(QuotedString(b"SIM PIN")));
}

#[test]
fn test_parse_ccss() {
    let (rem, cmd) = Command::parse(b"AT+CCSS=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetCdmaSubscriptionSource(1));
}

#[test]
fn test_parse_wrmp() {
    let (rem, cmd) = Command::parse(b"AT+WRMP=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetCdmaRoamingPreference(1));
}

#[test]
fn test_parse_mbau() {
    let (rem, cmd) = Command::parse(b"AT+MBAU=1234").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SimAuthentication(b"1234"));
}

#[test]
fn test_parse_ccfc_set() {
    let (rem, cmd) = Command::parse(b"AT+CCFC=1,1,\"12345\",145").unwrap();
    assert!(rem.is_empty());
    assert_eq!(
        cmd,
        Command::CallForwarding {
            reason: 1,
            mode: 1,
            number: Some(QuotedString(b"12345")),
            type_: Some(145)
        }
    );
}

#[test]
fn test_parse_ccfc_query() {
    let (rem, cmd) = Command::parse(b"AT+CCFC=1,1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::CallForwarding { reason: 1, mode: 1, number: None, type_: None });
}

#[test]
fn test_parse_atd() {
    let (rem, cmd) = Command::parse(b"ATD12345;").unwrap();
    assert_eq!(rem, b";");
    assert_eq!(cmd, Command::Dial(b"12345"));
}

#[test]
fn test_parse_ata() {
    let (rem, cmd) = Command::parse(b"ATA").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::Answer);
}

#[test]
fn test_parse_ath() {
    let (rem, cmd) = Command::parse(b"ATH").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::Hangup);
}

#[test]
fn test_parse_clcc() {
    let (rem, cmd) = Command::parse(b"AT+CLCC").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::QueryCurrentCalls);
}

#[test]
fn test_parse_cmut_set() {
    let (rem, cmd) = Command::parse(b"AT+CMUT=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetMute(1));
}

#[test]
fn test_parse_cmut_query() {
    let (rem, cmd) = Command::parse(b"AT+CMUT?").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::QueryMute);
}

#[test]
fn test_parse_vts() {
    let (rem, cmd) = Command::parse(b"AT+VTS=123").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SendDtmf(b"123"));
}

#[test]
fn test_parse_cusd() {
    let (rem, cmd) = Command::parse(b"AT+CUSD=").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::CancelUssd);
}

#[test]
fn test_parse_wsos_set() {
    let (rem, cmd) = Command::parse(b"AT+WSOS=0").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetEmergencyMode(0));
}

#[test]
fn test_parse_wsos_query() {
    let (rem, cmd) = Command::parse(b"AT+WSOS?").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::QueryEmergencyMode);
}

#[test]
fn test_parse_cops_query() {
    let (rem, cmd) = Command::parse(b"AT+COPS?").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::QueryOperator);
}

#[test]
fn test_parse_csq() {
    let (rem, cmd) = Command::parse(b"AT+CSQ").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::QuerySignalStrength);
}

#[test]
fn test_parse_cmgs() {
    let (rem, cmd) = Command::parse(b"AT+CMGS=12").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SendSms(12));
}

#[test]
fn test_parse_cnma() {
    let (rem, cmd) = Command::parse(b"AT+CNMA").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SendSmsAck);
}

#[test]
fn test_parse_cusatd_query() {
    let (rem, cmd) = Command::parse(b"AT+CUSATD?").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::QueryStkReady);
}

#[test]
fn test_parse_cusate() {
    let (rem, cmd) = Command::parse(b"AT+CUSATE=\"123\"").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SendStkEnvelope(QuotedString(b"123")));
}

#[test]
fn test_parse_clir_query() {
    let (rem, cmd) = Command::parse(b"AT+CLIR?").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::QueryClir);
}

#[test]
fn test_parse_e() {
    let (rem, cmd) = Command::parse(b"ATE1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetEcho(1));
}

#[test]
fn test_parse_l() {
    let (rem, cmd) = Command::parse(b"ATL3").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetSpeakerVolume(3));
}

#[test]
fn test_parse_m() {
    let (rem, cmd) = Command::parse(b"ATM1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetSpeakerMute(1));
}

#[test]
fn test_parse_q() {
    let (rem, cmd) = Command::parse(b"ATQ0").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetQuietMode(0));
}

#[test]
fn test_parse_v() {
    let (rem, cmd) = Command::parse(b"ATV1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetVerboseMode(1));
}

#[test]
fn test_parse_and_f() {
    let (rem, cmd) = Command::parse(b"AT&F").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::ResetToFactoryDefaults);
}

#[test]
fn test_parse_and_v() {
    let (rem, cmd) = Command::parse(b"AT&V").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::ViewActiveConfiguration);
}

#[test]
fn test_parse_and_w() {
    let (rem, cmd) = Command::parse(b"AT&W").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::WriteActiveConfiguration);
}

#[test]
fn test_parse_z() {
    let (rem, cmd) = Command::parse(b"ATZ").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::Reset);
}

#[test]
fn test_parse_i() {
    let (rem, cmd) = Command::parse(b"ATI").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::GetIdentificationInformation);
}

#[test]
fn test_parse_s0() {
    let (rem, cmd) = Command::parse(b"ATS0=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetAutoAnswer(1));
}

#[test]
fn test_parse_s3() {
    let (rem, cmd) = Command::parse(b"ATS3=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetCommandTerminationCharacter(1));
}

#[test]
fn test_parse_s4() {
    let (rem, cmd) = Command::parse(b"ATS4=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetResponseFormattingCharacter(1));
}

#[test]
fn test_parse_s5() {
    let (rem, cmd) = Command::parse(b"ATS5=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetCommandLineEditingCharacter(1));
}

#[test]
fn test_parse_s6() {
    let (rem, cmd) = Command::parse(b"ATS6=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetPauseBeforeBlindDialing(1));
}

#[test]
fn test_parse_s7() {
    let (rem, cmd) = Command::parse(b"ATS7=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetConnectionCompletionTimeout(1));
}

#[test]
fn test_parse_s8() {
    let (rem, cmd) = Command::parse(b"ATS8=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetCommaDialModifierTime(1));
}

#[test]
fn test_parse_s10() {
    let (rem, cmd) = Command::parse(b"ATS10=1").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::SetAutomaticDisconnectDelay(1));
}

#[test]
fn test_parse_gcap() {
    let (rem, cmd) = Command::parse(b"AT+GCAP").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::GetCapabilities);
}

#[test]
fn test_parse_gmi() {
    let (rem, cmd) = Command::parse(b"AT+GMI").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::GetManufacturerIdentification);
}

#[test]
fn test_parse_gmm() {
    let (rem, cmd) = Command::parse(b"AT+GMM").unwrap();
    assert!(rem.is_empty());
    assert_eq!(cmd, Command::GetModelId);
}
