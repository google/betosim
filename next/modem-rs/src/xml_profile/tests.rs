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

use serde::Deserialize;
use serde_xml_rs::{Deserializer, EventReader, ParserConfig};

use super::*;
use crate::{
    apdu::Instruction,
    constants::{SW_FILE_NOT_FOUND, SW_SUCCESS, UiccFileId},
    profiles::{PROFILE_CTS_XML, PROFILE_DEFAULT_XML, PROFILE_TEL_ALASKA_XML},
    sim_service::{SimCommand, SimResponse, SimService},
    types::{ExecutionResult, HandledCommand, Response},
};

#[test]
fn test_parse_default_profile() {
    let profile = parse_xml_profile(PROFILE_DEFAULT_XML);
    assert!(profile.is_ok(), "Failed to parse default profile: {:?}", profile.err());
    let profile = profile.unwrap();
    assert_eq!(profile.imsi, "310260000000000");
    assert_eq!(profile.iccid, "89860318640220133897");
}

#[test]
fn test_parse_cts_profile() {
    let profile = parse_xml_profile(PROFILE_CTS_XML);
    assert!(profile.is_ok(), "Failed to parse CTS profile: {:?}", profile.err());
    let profile = profile.unwrap();
    assert_eq!(profile.imsi, "310260000000000");
    assert_eq!(profile.iccid, "89860318640220133897");
}

#[test]
fn test_parse_tel_alaska_profile() {
    let profile = parse_xml_profile(PROFILE_TEL_ALASKA_XML);
    assert!(profile.is_ok(), "Failed to parse TelAlaska profile: {:?}", profile.err());
    let profile = profile.unwrap();
    assert_eq!(profile.imsi, "311740123456789");
    assert_eq!(profile.iccid, "89860318640220133897");
}

#[test]
fn test_invalid_record_number_fails() {
    let bad_xml = r#"<IccProfile>
        <MF>
            <EF id="6FC7" structure="linear fixed">
                <SIMIO cmd="B2" p1="00" p2="04" p3="26">90,00,01020304</SIMIO>
            </EF>
        </MF>
    </IccProfile>"#;
    let result = parse_xml_profile(bad_xml);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        XmlProfileError::InvalidValue {
            file_id: UiccFileId::MailboxDialingNumbers.as_u16(),
            field: "SIMIO p1 (record number)".to_string(),
            value: "0".to_string(),
            expected: "1-indexed record number between 1 and 255".to_string(),
        }
    );

    let overflow_xml = r#"<IccProfile>
        <MF>
            <EF id="6FC7" structure="linear fixed">
                <SIMIO cmd="B2" p1="100" p2="04" p3="26">90,00,01020304</SIMIO>
            </EF>
        </MF>
    </IccProfile>"#;
    let result = parse_xml_profile(overflow_xml);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), XmlProfileError::Deserialization(_)));
}

#[test]
fn test_parameter_normalization() {
    let xml = r#"<IccProfile>
        <MF>
            <EF id="6FC7" structure="linear fixed">
                <SIMIO cmd="B2" p1="0A" p2="04" p3="01">90,00,01</SIMIO>
            </EF>
        </MF>
    </IccProfile>"#;
    let profile = parse_xml_profile(xml).unwrap();
    let ef = profile.sim_io.file_system.find_ef(UiccFileId::MailboxDialingNumbers).unwrap();
    assert_eq!(ef.record_len, Some(1));
    assert_eq!(ef.data, hex::decode("FFFFFFFFFFFFFFFFFF01").unwrap());
}

#[test]
fn test_deduce_record_len_from_dc_mappings() {
    let xml = r#"<IccProfile>
        <MF>
            <EF id="6FC7" structure="linear fixed">
                <SIMIO cmd="DC" p1="1" p2="04" p3="26">144,0</SIMIO>
                <SIMIO cmd="DC" p1="2" p2="04" p3="26">144,0</SIMIO>
            </EF>
        </MF>
    </IccProfile>"#;
    let profile = parse_xml_profile(xml).unwrap();
    let ef = profile.sim_io.file_system.find_ef(UiccFileId::MailboxDialingNumbers).unwrap();
    let mbdn_len =
        UiccFileId::MailboxDialingNumbers.default_record_len().expect("file id is record based");
    assert_eq!(ef.record_len, Some(mbdn_len));
    assert_eq!(ef.size(), 2 * mbdn_len);
    assert_eq!(ef.data, vec![0xFF; 2 * mbdn_len]);
}

#[test]
fn test_invalid_file_id_fails() {
    let xml = r#"<IccProfile><MF><EF id="XYZ"></EF></MF></IccProfile>"#;
    let result = parse_xml_profile(xml);
    assert!(matches!(result, Err(XmlProfileError::Deserialization(_))));
}

#[test]
fn test_invalid_hex_param_fails() {
    let xml = r#"<IccProfile><MF><EF id="6FC7"><SIMIO cmd="B2" p1="XX" p2="04" p3="26">144,0,01</SIMIO></EF></MF></IccProfile>"#;
    let result = parse_xml_profile(xml);
    assert!(matches!(result, Err(XmlProfileError::Deserialization(_))));
}

#[test]
fn test_invalid_ccid_fails() {
    let xml = r#"<IccProfile><MF><EF id="2FE2"><CCID>898603abc</CCID></EF></MF></IccProfile>"#;
    let result = parse_xml_profile(xml);
    assert!(matches!(result, Err(XmlProfileError::InvalidValue { field, .. }) if field == "CCID"));
}

#[test]
fn test_invalid_cimi_fails() {
    let xml = r#"<IccProfile><MF><ADF aid="A0000000871002FF86FF0389FFFFFFFF"><EF id="6F07"><CIMI>31174abc</CIMI></EF></ADF></MF></IccProfile>"#;
    let result = parse_xml_profile(xml);
    assert!(matches!(result, Err(XmlProfileError::InvalidValue { field, .. }) if field == "CIMI"));
}

#[test]
fn test_invalid_simio_response_fails() {
    let xml = r#"<IccProfile><MF><EF id="2FE2"><SIMIO cmd="B0" p1="0" p2="0" p3="A">144,0</SIMIO></EF></MF></IccProfile>"#;
    let result = parse_xml_profile(xml);
    assert!(matches!(result, Err(XmlProfileError::InvalidSimIoResponse { .. })));
}

#[test]
fn test_missing_mf_fails() {
    let xml = r#"<IccProfile></IccProfile>"#;
    let result = parse_xml_profile(xml);
    assert_eq!(result, Err(XmlProfileError::MissingMasterFile));
}

#[test]
fn test_parse_pin_retries() {
    let xml = r#"<IccProfile>
        <MF></MF>
        <PinProfile>
            <PINREMAINTIMES>5</PINREMAINTIMES>
            <PUKREMAINTIMES>15</PUKREMAINTIMES>
            <PIN2REMAINTIMES>6</PIN2REMAINTIMES>
            <PUK2REMAINTIMES>16</PUK2REMAINTIMES>
        </PinProfile>
    </IccProfile>"#;
    let profile = parse_xml_profile(xml).unwrap();
    assert_eq!(profile.pin_profile.pin1_retries, Some(5));
    assert_eq!(profile.pin_profile.puk1_retries, Some(15));
    assert_eq!(profile.pin_profile.pin2_retries, Some(6));
    assert_eq!(profile.pin_profile.puk2_retries, Some(16));
}

#[test]
fn test_parse_eid_and_atr() {
    let xml = r#"<IccProfile>
        <MF></MF>
        <CardProfile>
            <EID>89049032000001000000000254806852</EID>
            <ATR>3F979580BFFE8210428031A073BE211797</ATR>
        </CardProfile>
    </IccProfile>"#;
    let profile = parse_xml_profile(xml).unwrap();
    assert_eq!(profile.eid, Some("89049032000001000000000254806852".to_string()));
    assert_eq!(profile.atr, Some("3F979580BFFE8210428031A073BE211797".to_string()));
}

#[test]
fn test_parse_fcp_linear_fixed_c0_simio() {
    // Tag 0x82 len 5: 02 (linear fixed) 00 00 1C (rec_len = 28) 03 (num_records =
    // 3) Tag 0x80 len 2: 00 54 (file_size = 84 = 3 * 28)
    let xml = r#"<IccProfile>
        <MF>
            <EF id="4F33">
                <SIMIO cmd="C0" p1="0" p2="0" p3="1C">98,130,621982050200001C0383024F338A01058B036F060180020054</SIMIO>
            </EF>
        </MF>
    </IccProfile>"#;
    let profile = parse_xml_profile(xml).unwrap();
    let ef = profile.sim_io.file_system.find_ef(0x4F33u16).unwrap();
    assert_eq!(ef.record_len, Some(28));
    assert_eq!(ef.size(), 84);
    assert_eq!(ef.data, vec![0xFF; 84]);
}

#[test]
fn test_parse_pin_state() {
    let xml = r#"<IccProfile>
        <MF></MF>
        <PinProfile>
            <PINSTATE>PINSTATE_ENABLED_NOT_VERIFIED</PINSTATE>
        </PinProfile>
    </IccProfile>"#;
    let profile = parse_xml_profile(xml).unwrap();
    assert_eq!(profile.pin_profile.state, crate::config::PinState::EnabledNotVerified);

    let xml2 = r#"<IccProfile>
        <MF></MF>
        <PinProfile>
            <PINSTATE>EnabledVerified</PINSTATE>
        </PinProfile>
    </IccProfile>"#;
    let profile2 = parse_xml_profile(xml2).unwrap();
    assert_eq!(profile2.pin_profile.state, crate::config::PinState::EnabledVerified);
}

#[test]
fn test_xml_simio_status_word() {
    let simio = schema::XmlSimIo {
        command: Instruction::GetResponse,
        p1: 0,
        p2: 0,
        p3: 0x0F,
        response: "106,130".to_string(),
    };
    assert_eq!(simio.command, Instruction::GetResponse);
    assert_eq!(simio.status_word(), Some(SW_FILE_NOT_FOUND));

    let success_simio = schema::XmlSimIo {
        command: Instruction::ReadBinary,
        p1: 0,
        p2: 0,
        p3: 4,
        response: "144,0,00000003".to_string(),
    };
    assert_eq!(success_simio.command, Instruction::ReadBinary);
    assert_eq!(success_simio.status_word(), Some(SW_SUCCESS));
}

#[test]
fn test_xml_elementary_file_is_file_not_found() {
    let xml_ef = schema::XmlElementaryFile {
        id: UiccFileId::VoiceMailIndicatorCphs.as_u16(),
        structure: Some(schema::XmlFileStructure::Transparent),
        members: vec![schema::XmlElementaryFileMember::Simio(schema::XmlSimIo {
            command: Instruction::GetResponse,
            p1: 0,
            p2: 0,
            p3: 0x0F,
            response: "106,130".to_string(),
        })],
    };
    assert!(xml_ef.is_file_not_found());
}

#[test]
fn test_ef_with_file_not_found_is_omitted() {
    let target_fid = UiccFileId::VoiceMailIndicatorCphs;
    let get_response_cmd = Instruction::GetResponse;
    let xml = format!(
        r#"<IccProfile>
        <MF>
            <EF id="{target_fid:04X}" structure="transparent">
                <SIMIO cmd="{get_response_cmd:02X}" p1="0" p2="0" p3="F">106,130</SIMIO>
            </EF>
        </MF>
    </IccProfile>"#
    );
    let profile = parse_xml_profile(&xml).unwrap();
    assert!(profile.sim_io.file_system.find_ef(target_fid).is_none());
}

fn find_xml_ef_in_df(
    df: &schema::XmlDedicatedFile,
    target_id: u16,
) -> Option<&schema::XmlElementaryFile> {
    for member in &df.members {
        match member {
            schema::XmlDedicatedFileMember::Elementary(ef) if ef.id == target_id => {
                return Some(ef);
            }
            schema::XmlDedicatedFileMember::Dedicated(sub_df) => {
                if let Some(ef) = find_xml_ef_in_df(sub_df, target_id) {
                    return Some(ef);
                }
            }
            schema::XmlDedicatedFileMember::ApplicationDedicated(adf) => {
                if let Some(ef) = find_xml_ef_in_adf(adf, target_id) {
                    return Some(ef);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_xml_ef_in_adf(
    adf: &schema::XmlApplicationDedicatedFile,
    target_id: u16,
) -> Option<&schema::XmlElementaryFile> {
    for member in &adf.members {
        match member {
            schema::XmlApplicationDedicatedFileMember::ElementaryFile(ef) if ef.id == target_id => {
                return Some(ef);
            }
            schema::XmlApplicationDedicatedFileMember::DedicatedFile(df) => {
                if let Some(ef) = find_xml_ef_in_df(df, target_id) {
                    return Some(ef);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn test_builtin_profiles_omit_cphs_mwi() {
    for (name, xml) in [
        ("default", PROFILE_DEFAULT_XML),
        ("cts", PROFILE_CTS_XML),
        ("tel_alaska", PROFILE_TEL_ALASKA_XML),
    ] {
        // 1. Verify that the raw XML profile actually defines EF 0x6F11 with status
        //    word 106,130
        let config = ParserConfig::new()
            .max_entity_expansion_depth(0)
            .max_entity_expansion_length(0)
            .trim_whitespace(true)
            .whitespace_to_characters(true)
            .cdata_to_characters(true)
            .ignore_comments(true)
            .coalesce_characters(true);
        let reader = EventReader::new_with_config(xml.as_bytes(), config);
        let mut deserializer = Deserializer::new(reader);
        let raw_profile = schema::XmlIccProfile::deserialize(&mut deserializer)
            .unwrap_or_else(|e| panic!("failed to deserialize raw {name} profile: {e:?}"));

        let target_fid = UiccFileId::VoiceMailIndicatorCphs.as_u16();
        let raw_ef = raw_profile
            .application_dedicated_files
            .iter()
            .find_map(|adf| find_xml_ef_in_adf(adf, target_fid))
            .or_else(|| {
                raw_profile.master_file.as_ref().and_then(|mf| find_xml_ef_in_df(mf, target_fid))
            })
            .unwrap_or_else(|| panic!("{name} profile must define raw EF 0x{target_fid:04X}"));

        assert!(
            raw_ef.is_file_not_found(),
            "{name} profile EF 0x{target_fid:04X} must be marked as not found"
        );
        let simio = raw_ef
            .members
            .iter()
            .find_map(|m| match m {
                schema::XmlElementaryFileMember::Simio(s) => Some(s),
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!("{name} profile EF 0x{target_fid:04X} must have a SIMIO mapping")
            });
        assert_eq!(simio.command, Instruction::GetResponse);
        assert_eq!(simio.status_word(), Some(SW_FILE_NOT_FOUND));

        // 2. Verify that parsing the profile filters out EF from the filesystem
        let profile = parse_xml_profile(xml)
            .unwrap_or_else(|e| panic!("failed to parse {name} profile: {e:?}"));
        assert!(
            profile.sim_io.file_system.find_ef(target_fid).is_none(),
            "{name} profile unexpectedly contains EF 0x{target_fid:04X}"
        );
    }
}

#[test]
fn test_sim_service_with_parsed_profile_rejects_missing_ef() {
    let profile = parse_xml_profile(PROFILE_DEFAULT_XML).unwrap();
    let mut service = SimService::from_profile(&profile);
    let target_fid = UiccFileId::VoiceMailIndicatorCphs.as_u16();

    let get_resp_cmd = SimCommand::SimIo {
        command: Instruction::GetResponse,
        file_id: target_fid,
        p1: 0,
        p2: 0,
        p3: 15,
        data: None,
        path: None,
    };
    let result = service.execute(&get_resp_cmd);
    assert_eq!(
        result,
        ExecutionResult::Success(HandledCommand {
            responses: vec![
                Response::Sim(SimResponse::RestrictedSimAccess {
                    sw: SW_FILE_NOT_FOUND,
                    data: None,
                }),
                Response::Ok,
            ],
            actions: vec![],
        })
    );

    let read_binary_cmd = SimCommand::SimIo {
        command: Instruction::ReadBinary,
        file_id: target_fid,
        p1: 0,
        p2: 0,
        p3: 0,
        data: None,
        path: None,
    };
    let result = service.execute(&read_binary_cmd);
    assert_eq!(
        result,
        ExecutionResult::Success(HandledCommand {
            responses: vec![
                Response::Sim(SimResponse::RestrictedSimAccess {
                    sw: SW_FILE_NOT_FOUND,
                    data: None,
                }),
                Response::Ok,
            ],
            actions: vec![],
        })
    );
}
