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

use nom::IResult;

pub use crate::types::QuotedString;
use crate::{
    call_service::CallCommand, data_service::DataCommand, misc_service::MiscCommand,
    network_service::NetworkCommand, sim_service::SimCommand, sms_service::SmsCommand,
    stk_service::StkCommand, sup_service::SupCommand,
};

pub fn parse_until_semicolon(input: &[u8]) -> IResult<&[u8], &[u8]> {
    use nom::{
        bytes::complete::{tag, take_while},
        combinator::opt,
    };
    let (input, content) = take_while(|c: u8| c != b';' && c != b'\r' && c != b'\n')(input)?;
    let (input, _) = opt(tag(b";"))(input)?;
    Ok((input, content))
}

/// Top-level AT Command wrapper enum across all modem-rs services.
#[derive(Debug, PartialEq, Clone)]
pub enum Command<'a> {
    Sim(SimCommand<'a>),
    Call(CallCommand),
    Sms(SmsCommand<'a>),
    Network(NetworkCommand<'a>),
    Data(DataCommand<'a>),
    Misc(MiscCommand),
    Sup(SupCommand<'a>),
    Stk(StkCommand<'a>),
}

impl<'a> Command<'a> {
    pub fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        use nom::{branch::alt, combinator::map};
        alt((
            map(SimCommand::parse, Self::Sim),
            map(CallCommand::parse, Self::Call),
            map(SmsCommand::parse, Self::Sms),
            map(NetworkCommand::parse, Self::Network),
            map(DataCommand::parse, Self::Data),
            map(SupCommand::parse, Self::Sup),
            map(StkCommand::parse, Self::Stk),
            map(MiscCommand::parse, Self::Misc),
        ))(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        apdu::Instruction,
        data_service::Qos,
        sms_service::{MessageStatus, MessageStorage},
        types::{
            CallForwardNumberType, CallForwardTime, CallForwardTon, CallForwardingMode,
            CallForwardingReason, CallMode, CallWaitingMode, CharacterSet, ClirMode, CopsFormat,
            CopsMode, CtecPreferredMask, DialArgs, DtmfArgs, DtmfTone, PacketEventReportingMode,
            Parsable, PdpContextActivateArgs, PdpType, PhoneNumber, ProductSerialNumberType,
            RadioPowerLevel, SendSmsArgs, ServiceClass, TypeOfAddress,
        },
    };

    #[test]
    fn test_parse_cpin_query() {
        let (rem, cmd) = Command::parse(b"AT+CPIN?").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Sim(SimCommand::GetSimStatus));
    }

    #[test]
    fn test_parse_ipr() {
        let (rem, cmd) = Command::parse(b"AT+IPR=9600").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Misc(MiscCommand::SetTeTaFixedLocalRate(9600)));
    }

    #[test]
    fn test_parse_cmee() {
        let (rem, cmd) = Command::parse(b"AT+CMEE=1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Misc(MiscCommand::SetReportMobileEquipmentError(
                crate::types::CmeeMode::Numeric
            ))
        );

        let (rem, cmd) = Command::parse(b"AT+CMEE?").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Misc(MiscCommand::QueryReportMobileEquipmentError));

        let (rem, cmd) = Command::parse(b"AT+CMEE=?").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Misc(MiscCommand::QuerySupportedReportMobileEquipmentError));
    }

    #[test]
    fn test_parse_cgdcont() {
        let (rem, cmd) = Command::parse(b"AT+CGDCONT=1,\"IP\",\"apn\"").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Data(DataCommand::DefinePdpContext(
                1,
                PdpType::Ip,
                QuotedString("apn"),
                None,
                None,
                None
            ))
        );
    }

    #[test]
    fn test_parse_cgdcont_extra() {
        let (rem, cmd) =
            Command::parse(b"AT+CGDCONT=1,\"IPV6\",\"fast.t-mobile.com\",,0,0").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Data(DataCommand::DefinePdpContext(
                1,
                PdpType::Ipv6,
                QuotedString("fast.t-mobile.com"),
                None,
                Some(0),
                Some(0)
            ))
        );
    }

    #[test]
    fn test_parse_cgeqmin() {
        let (rem, cmd) = Command::parse(b"AT+CGEQMIN=1,2,3,4,5,6").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Data(DataCommand::SetQualityOfServiceMinimum(1, Qos::new(2, 3, 4, 5, 6)))
        );
    }

    #[test]
    fn test_parse_cgeqreq() {
        let (rem, cmd) = Command::parse(b"AT+CGEQREQ=1,2,3,4,5,6").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Data(DataCommand::SetQualityOfServiceRequested(1, Qos::new(2, 3, 4, 5, 6)))
        );
    }

    #[test]
    fn test_parse_cgqmin() {
        let (rem, cmd) = Command::parse(b"AT+CGQMIN=1,2,3,4,5,6").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Data(DataCommand::SetQualityOfServiceMinimumGprs(1, Qos::new(2, 3, 4, 5, 6)))
        );
    }

    #[test]
    fn test_parse_cgqreq() {
        let (rem, cmd) = Command::parse(b"AT+CGQREQ=1,2,3,4,5,6").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Data(DataCommand::SetQualityOfServiceRequestedGprs(
                1,
                Qos::new(2, 3, 4, 5, 6)
            ))
        );
    }

    #[test]
    fn test_parse_cgact() {
        let (rem, cmd) = Command::parse(b"AT+CGACT=1,1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Data(DataCommand::SetPdpContextActivate(PdpContextActivateArgs {
                cid: 1,
                state: true,
            }))
        );
    }

    #[test]
    fn test_parse_cgatt() {
        let (rem, cmd) = Command::parse(b"AT+CGATT=1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Data(DataCommand::SetPsAttach(true)));
    }

    #[test]
    fn test_parse_cgcmod() {
        let (rem, cmd) = Command::parse(b"AT+CGCMOD=1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Data(DataCommand::SetPdpContextModify(1)));
    }

    #[test]
    fn test_parse_cgdata() {
        let (rem, cmd) = Command::parse(b"AT+CGDATA=\"PPP\",1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Data(DataCommand::EnterDataState(Some(QuotedString("PPP")), Some(1)))
        );

        // Spec-shaped omission of <L2P>: explicit empty slot before comma
        let (rem, cmd) = Command::parse(b"AT+CGDATA=,1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Data(DataCommand::EnterDataState(None, Some(1))));

        let (rem, cmd) = Command::parse(b"AT+CGDATA=").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Data(DataCommand::EnterDataState(None, None)));

        // Bare AT+CGDATA=1 is rejected: <L2P> is a string parameter
        let (rem, cmd) = Command::parse(b"AT+CGDATA=1").unwrap();
        assert!(!rem.is_empty());
        assert!(!matches!(cmd, Command::Data(DataCommand::EnterDataState(..))));
    }

    #[test]
    fn test_parse_cgerep() {
        let (rem, cmd) = Command::parse(b"AT+CGEREP=1,1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Data(DataCommand::SetPacketEventReporting(
                PacketEventReportingMode::Discard,
                Some(true)
            ))
        );
    }

    #[test]
    fn test_parse_cgpaddr() {
        let (rem, cmd) = Command::parse(b"AT+CGPADDR=1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Data(DataCommand::ShowPdpAddress(1)));
    }

    #[test]
    fn test_parse_cmgf() {
        let (rem, cmd) = Command::parse(b"AT+CMGF=1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sms(SmsCommand::SetSmsMessageFormat(crate::sms_service::MessageFormat::Text))
        );
    }

    #[test]
    fn test_parse_cusatd_query() {
        let (rem, cmd) = Command::parse(b"AT+CUSATD?").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Stk(StkCommand::QueryStkReady));
    }

    #[test]
    fn test_parse_cusatd_set() {
        let (rem, cmd) = Command::parse(b"AT+CUSATD=1,\"010203\"").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Stk(StkCommand::SetStkReady(true, Some(QuotedString("010203")))));
    }

    #[test]
    fn test_parse_cusate() {
        let (rem, cmd) = Command::parse(b"AT+CUSATE=\"D30782028281100150\"").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Stk(StkCommand::SendStkEnvelope(QuotedString("D30782028281100150")))
        );
    }

    #[test]
    fn test_parse_cusatt() {
        let (rem, cmd) = Command::parse(b"AT+CUSATT=\"81030124008202828183020001\"").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Stk(StkCommand::SendStkTerminalResponse(QuotedString(
                "81030124008202828183020001"
            )))
        );
    }

    #[test]
    fn test_parse_crsm() {
        let (rem, cmd) = Command::parse(b"AT+CRSM=176,28480,0,0,7").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sim(SimCommand::SimIo {
                command: Instruction::ReadBinary,
                file_id: 28480,
                p1: 0,
                p2: 0,
                p3: 7,
                data: None,
                path: None,
            })
        );

        let (rem2, cmd2) = Command::parse(b"AT+CRSM=176,28480,0,0,7,,").unwrap();
        assert!(rem2.is_empty());
        assert_eq!(
            cmd2,
            Command::Sim(SimCommand::SimIo {
                command: Instruction::ReadBinary,
                file_id: 28480,
                p1: 0,
                p2: 0,
                p3: 7,
                data: None,
                path: None,
            })
        );
    }

    #[test]
    fn test_parse_gprs_dial() {
        let (rem, cmd) = Command::parse(b"ATD*99***1#\r\n").unwrap();
        assert_eq!(rem, b"\r\n");
        let expected_dial_args = DialArgs {
            number: PhoneNumber::new_for_test("*99***1#"),
            clir: ClirMode::SubscriptionDefault,
            is_emergency: false,
        };
        assert_eq!(cmd, Command::Call(CallCommand::Dial(expected_dial_args)));
    }

    #[test]
    fn test_parse_ccwa_set_single_arg() {
        let (rem, cmd) = Command::parse(b"AT+CCWA=1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Sup(SupCommand::SetCallWaiting(true, None, None)));
    }

    #[test]
    fn test_parse_ccwa_set_multiple_args() {
        let (rem, cmd) = Command::parse(b"AT+CCWA=1,2,7").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sup(SupCommand::SetCallWaiting(
                true,
                Some(CallWaitingMode::Query),
                Some(ServiceClass::VOICE_DATA_FAX)
            ))
        );
    }

    #[test]
    fn test_parse_ccfc() {
        let (rem, cmd) = Command::parse(br#"AT+CCFC=0,1,"+1234567890",145,7"#).unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sup(SupCommand::CallForwarding {
                reason: CallForwardingReason::Unconditional,
                mode: CallForwardingMode::Enable,
                number: Some(PhoneNumber::new_for_test("+1234567890")),
                toa: Some(TypeOfAddress::International),
                class: Some(ServiceClass::VOICE_DATA_FAX),
                subaddr: None,
                satype: None,
                time: None,
            })
        );

        let (rem, cmd) = Command::parse(b"AT+CCFC=0,0").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sup(SupCommand::CallForwarding {
                reason: CallForwardingReason::Unconditional,
                mode: CallForwardingMode::Disable,
                number: None,
                toa: None,
                class: None,
                subaddr: None,
                satype: None,
                time: None,
            })
        );
    }

    #[test]
    fn test_parse_ccfcu() {
        let (rem, cmd) =
            Command::parse(br#"AT+CCFCU=0,3,2,145,"+1234567890",1,"","",,20"#).unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sup(SupCommand::CallForwardUtility {
                reason: CallForwardingReason::Unconditional,
                mode: CallForwardingMode::Registration,
                number_type: Some(CallForwardNumberType::Number),
                ton: Some(CallForwardTon(145)),
                number: Some(QuotedString("+1234567890")),
                class: Some(ServiceClass::VOICE),
                ruleset: Some(QuotedString("")),
                subaddr: Some(QuotedString("")),
                satype: None,
                time: Some(CallForwardTime(20)),
            })
        );

        let (rem, cmd) = Command::parse(br#"AT+CCFCU=0,2,2,129,"",1"#).unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sup(SupCommand::CallForwardUtility {
                reason: CallForwardingReason::Unconditional,
                mode: CallForwardingMode::Query,
                number_type: Some(CallForwardNumberType::Number),
                ton: Some(CallForwardTon(129)),
                number: Some(QuotedString("")),
                class: Some(ServiceClass::VOICE),
                ruleset: None,
                subaddr: None,
                satype: None,
                time: None,
            })
        );
    }

    #[test]
    fn test_parse_cmod_set() {
        let (rem, cmd) = Command::parse(b"AT+CMOD=0").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Misc(MiscCommand::SetCallMode(CallMode::SingleMode)));
    }

    #[test]
    fn test_parse_colp_set() {
        let (rem, cmd) = Command::parse(b"AT+COLP=0").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Sup(SupCommand::SetColp(false)));
    }

    #[test]
    fn test_parse_cscs_set() {
        let (rem, cmd) = Command::parse(br#"AT+CSCS="HEX""#).unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Misc(MiscCommand::SetCharacterSet(CharacterSet::Hex)));
    }

    #[test]
    fn test_parse_clir_set_goldfish() {
        let (rem, cmd) = Command::parse(b"AT+CLIR: 0").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Sup(SupCommand::SetClirGoldfish(ClirMode::SubscriptionDefault)));
    }

    #[test]
    fn test_parse_clir_set_standard() {
        let (rem, cmd) = Command::parse(b"AT+CLIR=0").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Sup(SupCommand::SetClir(ClirMode::SubscriptionDefault)));
    }

    #[test]
    fn test_parse_cgsn_query() {
        let (rem, cmd) = Command::parse(b"AT+CGSN").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Misc(MiscCommand::GetProductSerialNumberGsm));
    }

    #[test]
    fn test_parse_cgsn_set() {
        let (rem, cmd) = Command::parse(b"AT+CGSN=2").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Misc(MiscCommand::GetProductSerialNumberGsmWithType(
                ProductSerialNumberType::ImeiWithSvn
            ))
        );
    }

    #[test]
    fn test_parse_cops_set() {
        let (rem, cmd) = Command::parse(b"AT+COPS=3,2").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Network(NetworkCommand::SetOperator {
                mode: CopsMode::SetFormatOnly,
                format: Some(CopsFormat::Numeric),
                oper: None,
                act: None,
            })
        );
    }

    #[test]
    fn test_parse_cops_set_with_act() {
        for raw in [0u8, 3, 6, 7, 11, 99] {
            let input = format!("AT+COPS=1,2,\"310260\",{raw}");
            let (rem, cmd) = Command::parse(input.as_bytes()).unwrap();
            assert!(rem.is_empty(), "unparsed remainder for <AcT>={raw}");
            assert_eq!(
                cmd,
                Command::Network(NetworkCommand::SetOperator {
                    mode: CopsMode::Manual,
                    format: Some(CopsFormat::Numeric),
                    oper: Some(QuotedString("310260")),
                    act: Some(raw),
                })
            );
        }
    }

    #[test]
    fn test_parse_at() {
        let (rem, cmd) = Command::parse(b"AT").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Misc(MiscCommand::Test));
    }

    #[test]
    fn test_parse_at_invalid() {
        let (rem, cmd) = Command::parse(b"AT+INVALID").unwrap();
        assert!(!rem.is_empty());
        assert_eq!(cmd, Command::Misc(MiscCommand::Test));
    }

    #[test]
    fn test_parse_at_and_v() {
        let (rem, cmd) = Command::parse(b"AT&V").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Misc(MiscCommand::ViewActiveConfiguration));
    }

    #[test]
    fn test_parse_cmgw() {
        let (rem, cmd) = Command::parse(b"AT+CMGW=16").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Sms(SmsCommand::StoreSms(16, None)));

        let (rem, cmd) = Command::parse(b"AT+CMGW=16,0").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sms(SmsCommand::StoreSms(16, Some(MessageStatus::ReceivedUnread)))
        );

        let (rem, cmd) = Command::parse(b"AT+CMGW=16,1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Sms(SmsCommand::StoreSms(16, Some(MessageStatus::ReceivedRead))));

        let (rem, cmd) = Command::parse(b"AT+CMGW=16,2").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Sms(SmsCommand::StoreSms(16, Some(MessageStatus::StoredUnsent))));

        let (rem, cmd) = Command::parse(b"AT+CMGW=16,3").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Sms(SmsCommand::StoreSms(16, Some(MessageStatus::StoredSent))));

        // Out-of-range stat values must not parse as StoreSms.
        let (rem, cmd) = Command::parse(b"AT+CMGW=16,4").unwrap();
        assert!(!rem.is_empty());
        assert!(!matches!(cmd, Command::Sms(SmsCommand::StoreSms(..))));
    }

    #[test]
    fn test_parse_wsos() {
        let (rem, cmd) = Command::parse(b"AT+WSOS=0").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Call(CallCommand::SetEmergencyMode(false)));

        let (rem, cmd) = Command::parse(b"AT+WSOS=1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Call(CallCommand::SetEmergencyMode(true)));

        // AT+WSOS=2 should fail parsing (bool only accepts 0 and 1)
        assert!(CallCommand::parse(b"AT+WSOS=2").is_err());
    }

    #[test]
    fn test_parse_cpms() {
        let (rem, cmd) = Command::parse(b"AT+CPMS=\"SM\"").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sms(SmsCommand::SetPreferredMessageStorage(MessageStorage::Sim, None, None))
        );

        let (rem, cmd) = Command::parse(b"AT+CPMS=\"SM\",\"ME\"").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sms(SmsCommand::SetPreferredMessageStorage(
                MessageStorage::Sim,
                Some(MessageStorage::Me),
                None
            ))
        );

        let (rem, cmd) = Command::parse(b"AT+CPMS=\"SM\",\"ME\",\"SM\"").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sms(SmsCommand::SetPreferredMessageStorage(
                MessageStorage::Sim,
                Some(MessageStorage::Me),
                Some(MessageStorage::Sim)
            ))
        );

        // Invalid storage string fails parsing
        assert!(SmsCommand::parse(b"AT+CPMS=\"INVALID\"").is_err());
    }

    #[test]
    fn test_parse_cmut_bool() {
        let (rem, cmd) = Command::parse(b"AT+CMUT=0").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Call(CallCommand::SetMute(false)));

        let (rem, cmd) = Command::parse(b"AT+CMUT=1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Call(CallCommand::SetMute(true)));

        assert!(CallCommand::parse(b"AT+CMUT=2").is_err());
    }

    #[test]
    fn test_parse_dtmf_args_valid_and_invalid() {
        // Valid single DTMF digit without duration
        let (rem, cmd) = Command::parse(b"AT+VTS=1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Call(CallCommand::SendDtmf(DtmfArgs { tone: DtmfTone(b'1'), duration: None }))
        );

        // Valid DTMF digit with duration
        let (rem, cmd) = Command::parse(b"AT+VTS=\"A\",250").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Call(CallCommand::SendDtmf(DtmfArgs {
                tone: DtmfTone(b'A'),
                duration: Some(250),
            }))
        );

        // Case preservation
        let (_, cmd_lower) = Command::parse(b"AT+VTS=d").unwrap();
        assert_eq!(
            cmd_lower,
            Command::Call(CallCommand::SendDtmf(DtmfArgs { tone: DtmfTone(b'd'), duration: None }))
        );

        // Multi-digit rejection upfront
        assert!(CallCommand::parse(b"AT+VTS=12").is_err());

        // Invalid tone character rejection
        assert!(CallCommand::parse(b"AT+VTS=X").is_err());

        // Non-ASCII character whose lower 8 bits match valid DTMF (e.g. \u{0130} is
        // 304, 304 % 256 = 48 = b'0')
        assert!(CallCommand::parse("AT+VTS=\"\u{0130}\"".as_bytes()).is_err());

        // Malformed duration rejection
        assert!(CallCommand::parse(b"AT+VTS=1,abc").is_err());
    }

    #[test]
    fn test_parse_ctec_preferred_mask() {
        // Valid CTEC with hex mask (with 0x prefix)
        let (rem, cmd) = Command::parse(b"AT+CTEC=32,0x63").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Network(NetworkCommand::SetNetworkTechnology(
                32,
                Some(CtecPreferredMask(0x63))
            ))
        );

        // Valid CTEC with hex mask (without prefix)
        let (rem, cmd) = Command::parse(b"AT+CTEC=32,63").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Network(NetworkCommand::SetNetworkTechnology(
                32,
                Some(CtecPreferredMask(0x63))
            ))
        );

        assert_eq!(CtecPreferredMask::parse(b"0x04").unwrap().1, CtecPreferredMask(0x04));
        assert!(CtecPreferredMask::parse(b"0x80").is_err());
    }

    #[test]
    fn test_parse_cscs_character_sets() {
        assert_eq!(
            Command::parse(br#"AT+CSCS="GSM""#).unwrap().1,
            Command::Misc(MiscCommand::SetCharacterSet(CharacterSet::Gsm))
        );
        assert_eq!(
            Command::parse(br#"AT+CSCS="HEX""#).unwrap().1,
            Command::Misc(MiscCommand::SetCharacterSet(CharacterSet::Hex))
        );
        assert_eq!(
            Command::parse(br#"AT+CSCS="UCS2""#).unwrap().1,
            Command::Misc(MiscCommand::SetCharacterSet(CharacterSet::Ucs2))
        );
        assert_eq!(
            Command::parse(br#"AT+CSCS="IRA""#).unwrap().1,
            Command::Misc(MiscCommand::SetCharacterSet(CharacterSet::Ira))
        );
        assert_eq!(
            Command::parse(br#"AT+CSCS="PCCP437""#).unwrap().1,
            Command::Misc(MiscCommand::SetCharacterSet(CharacterSet::Pccp437))
        );
        assert_eq!(
            Command::parse(br#"AT+CSCS="8859-1""#).unwrap().1,
            Command::Misc(MiscCommand::SetCharacterSet(CharacterSet::Iso8859_1))
        );
        assert_eq!(
            Command::parse(br#"AT+CSCS="UTF-8""#).unwrap().1,
            Command::Misc(MiscCommand::SetCharacterSet(CharacterSet::Utf8))
        );
        assert!(CharacterSet::parse(br#""INVALID""#).is_err());
    }

    #[test]
    fn test_parse_cmgs_args() {
        // PDU mode (length only)
        let (rem, cmd) = Command::parse(b"AT+CMGS=24").unwrap();
        assert!(rem.is_empty());
        assert_eq!(cmd, Command::Sms(SmsCommand::SendSms(SendSmsArgs::Pdu { length: 24 })));

        // Text mode (destination address in quotes)
        let (rem, cmd) = Command::parse(br#"AT+CMGS="+1234567890""#).unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sms(SmsCommand::SendSms(SendSmsArgs::Text {
                destination_address: QuotedString("+1234567890"),
                type_of_destination_address: None,
            }))
        );

        // Text mode with toda
        let (rem, cmd) = Command::parse(br#"AT+CMGS="+1234567890",145"#).unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sms(SmsCommand::SendSms(SendSmsArgs::Text {
                destination_address: QuotedString("+1234567890"),
                type_of_destination_address: Some(TypeOfAddress::International),
            }))
        );
    }

    #[test]
    fn test_parse_cfun() {
        let (rem, cmd) = Command::parse(b"AT+CFUN=1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Network(NetworkCommand::SetRadioPower(RadioPowerLevel::Full, None))
        );

        let (rem, cmd) = Command::parse(b"AT+CFUN=1,1").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Network(NetworkCommand::SetRadioPower(RadioPowerLevel::Full, Some(true)))
        );

        let (rem, cmd) = Command::parse(b"AT+CFUN=0,0").unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Network(NetworkCommand::SetRadioPower(RadioPowerLevel::Minimum, Some(false)))
        );
    }

    #[test]
    fn test_parse_csca() {
        let (rem, cmd) = Command::parse(br#"AT+CSCA="+1234567890",145"#).unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sms(SmsCommand::SetSmscAddress(
                QuotedString("+1234567890"),
                Some(TypeOfAddress::International)
            ))
        );

        let (rem, cmd) = Command::parse(br#"AT+CSCA="12345",129"#).unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sms(SmsCommand::SetSmscAddress(
                QuotedString("12345"),
                Some(TypeOfAddress::Unknown)
            ))
        );

        let (rem, cmd) = Command::parse(br#"AT+CSCA="+1234567890""#).unwrap();
        assert!(rem.is_empty());
        assert_eq!(
            cmd,
            Command::Sms(SmsCommand::SetSmscAddress(QuotedString("+1234567890"), None))
        );
    }

    #[test]
    fn test_parse_quoted_string_utf8_validation() {
        // Valid UTF-8 quoted string
        let (rem, s) = QuotedString::parse(b"\"Hello World\"").unwrap();
        assert!(rem.is_empty());
        assert_eq!(s.as_str(), "Hello World");

        // Invalid UTF-8 bytes rejected upfront at nom parse time
        let invalid_utf8 = [b'"', 0xFF, 0xFE, 0xFD, b'"'];
        assert!(QuotedString::parse(&invalid_utf8).is_err());
    }
}
