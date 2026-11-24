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

use crate::apdu::Instruction;

#[derive(Debug, Deserialize, Clone)]
pub struct XmlCardProfile {
    #[serde(rename = "EID")]
    pub eid: Option<String>,
    #[serde(rename = "ATR")]
    pub atr: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename = "IccProfile")]
pub struct XmlIccProfile {
    #[serde(rename = "MF")]
    pub master_file: Option<XmlDedicatedFile>,
    #[serde(rename = "ADF")]
    #[serde(default)]
    pub application_dedicated_files: Vec<XmlApplicationDedicatedFile>,
    #[serde(rename = "PinProfile")]
    pub pin_profile: Option<XmlPinProfile>,
    #[serde(rename = "FacilityLock")]
    pub facility_lock: Option<XmlFacilityLock>,
    #[serde(rename = "SETUPMENU")]
    pub setup_menu: Option<XmlSetupMenu>,
    #[serde(rename = "CardProfile")]
    pub card_profile: Option<XmlCardProfile>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct XmlDedicatedFile {
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_option_hex_u16")]
    pub path: Option<u16>,
    #[serde(rename = "$value")]
    #[serde(default)]
    pub members: Vec<XmlDedicatedFileMember>,
}

#[derive(Debug, Deserialize, Clone)]
pub enum XmlDedicatedFileMember {
    #[serde(rename = "DF")]
    Dedicated(XmlDedicatedFile),
    #[serde(rename = "EF")]
    Elementary(XmlElementaryFile),
    #[serde(rename = "ADF")]
    ApplicationDedicated(XmlApplicationDedicatedFile),
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum XmlFileStructure {
    #[serde(rename = "transparent")]
    Transparent,
    #[serde(rename = "linear fixed")]
    LinearFixed,
}

#[derive(Debug, Deserialize, Clone)]
pub struct XmlElementaryFile {
    #[serde(deserialize_with = "deserialize_hex_u16")]
    pub id: u16,
    pub structure: Option<XmlFileStructure>,
    #[serde(rename = "$value")]
    #[serde(default)]
    pub members: Vec<XmlElementaryFileMember>,
}

#[derive(Debug, Deserialize, Clone)]
pub enum XmlElementaryFileMember {
    #[serde(rename = "SIMIO")]
    Simio(XmlSimIo),
    #[serde(rename = "CCID")]
    Ccid(String),
    #[serde(rename = "CIMI")]
    Cimi(String),
}

#[derive(Debug, Deserialize, Clone)]
pub struct XmlSimIo {
    #[serde(rename = "cmd")]
    #[serde(deserialize_with = "deserialize_hex_instruction")]
    pub command: Instruction,
    #[serde(deserialize_with = "deserialize_hex_u8")]
    pub p1: u8,
    #[serde(deserialize_with = "deserialize_hex_u8")]
    pub p2: u8,
    #[serde(deserialize_with = "deserialize_hex_u8")]
    pub p3: u8,
    #[serde(rename = "$value")]
    pub response: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct XmlApplicationDedicatedFile {
    pub aid: String,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_option_hex_u16")]
    pub path: Option<u16>,
    #[serde(rename = "$value")]
    #[serde(default)]
    pub members: Vec<XmlApplicationDedicatedFileMember>,
}

#[derive(Debug, Deserialize, Clone)]
pub enum XmlApplicationDedicatedFileMember {
    #[serde(rename = "DF")]
    DedicatedFile(XmlDedicatedFile),
    #[serde(rename = "EF")]
    ElementaryFile(XmlElementaryFile),
    #[serde(rename = "File")]
    FileOverride(XmlApplicationFileOverride),
    #[serde(rename = "CGLA")]
    Cgla(XmlApduMapping),
    #[serde(rename = "CSIM")]
    Csim(XmlApduMapping),
}

#[derive(Debug, Deserialize, Clone)]
pub struct XmlApplicationFileOverride {
    #[serde(deserialize_with = "deserialize_hex_u16")]
    pub id: u16,
    #[serde(rename = "$value")]
    #[serde(default)]
    pub members: Vec<XmlApplicationFileOverrideMember>,
}

#[derive(Debug, Deserialize, Clone)]
pub enum XmlApplicationFileOverrideMember {
    #[serde(rename = "CGLA")]
    Cgla(XmlApduMapping),
}

#[derive(Debug, Deserialize, Clone)]
pub struct XmlApduMapping {
    pub cmd: String,
    #[serde(rename = "$value")]
    pub response: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct XmlPinProfile {
    #[serde(rename = "PINSTATE")]
    pub pin_state: Option<crate::config::PinState>,
    #[serde(rename = "PINCODE")]
    pub pin_code: Option<String>,
    #[serde(rename = "PUKCODE")]
    pub puk_code: Option<String>,
    #[serde(rename = "PINREMAINTIMES")]
    pub pin_remain_times: Option<u32>,
    #[serde(rename = "PUKREMAINTIMES")]
    pub puk_remain_times: Option<u32>,
    #[serde(rename = "PIN2CODE")]
    pub pin2_code: Option<String>,
    #[serde(rename = "PUK2CODE")]
    pub puk2_code: Option<String>,
    #[serde(rename = "PIN2REMAINTIMES")]
    pub pin2_remain_times: Option<u32>,
    #[serde(rename = "PUK2REMAINTIMES")]
    pub puk2_remain_times: Option<u32>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct XmlFacilityLock {
    #[serde(rename = "SC")]
    pub sim_lock: Option<String>,
    #[serde(rename = "FD")]
    pub fixed_dialing: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct XmlSetupMenu {
    pub text: String,
    #[serde(rename = "SELECTITEM")]
    #[serde(default)]
    pub items: Vec<XmlSelectItem>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct XmlSelectItem {
    #[serde(default)]
    pub id: Option<u8>,
    #[serde(rename = "menuId")]
    #[serde(deserialize_with = "deserialize_hex_u8")]
    pub menu_id: u8,
    pub text: String,
    #[serde(rename = "$value")]
    #[serde(default)]
    pub sub_items: Vec<XmlSelectItemOrDisplayText>,
}

#[derive(Debug, Deserialize, Clone)]
pub enum XmlSelectItemOrDisplayText {
    #[serde(rename = "SELECTITEM")]
    SelectItem(XmlSelectItem),
    #[serde(rename = "DISPLAYTEXT")]
    DisplayText(XmlDisplayText),
}

#[derive(Debug, Deserialize, Clone)]
pub struct XmlDisplayText {
    #[serde(default)]
    pub id: Option<u8>,
    #[serde(rename = "menuId")]
    #[serde(deserialize_with = "deserialize_hex_u8")]
    pub menu_id: u8,
    pub text: String,
}

fn deserialize_hex_u16<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    u16::from_str_radix(s.trim(), 16).map_err(serde::de::Error::custom)
}

fn deserialize_option_hex_u16<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                u16::from_str_radix(trimmed, 16).map(Some).map_err(serde::de::Error::custom)
            }
        }
        None => Ok(None),
    }
}

fn deserialize_hex_u8<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        Ok(0)
    } else {
        u8::from_str_radix(trimmed, 16).map_err(serde::de::Error::custom)
    }
}

fn deserialize_hex_instruction<'de, D>(deserializer: D) -> Result<Instruction, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_hex_u8(deserializer).map(Instruction::from)
}
