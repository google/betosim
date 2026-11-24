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

use super::{error::XmlProfileError, schema::*};
use crate::{
    apdu::{self, ParsedApdu},
    config::{
        ApduMapping, ApplicationDedicatedFile, ApplicationFileOverride, DedicatedFile,
        ElementaryFile, SimFile, StkMenuItem,
    },
    constants::UiccFileId,
};

struct SimIoMapping {
    cmd: apdu::Instruction,
    p1: u8,
    p2: u8,
    p3: u8,
    response: String,
}

impl SimIoMapping {
    fn validate_record_addressing(&self, file_id: u16) -> Result<usize, XmlProfileError> {
        if self.p2 != 0x04 {
            return Err(XmlProfileError::InvalidValue {
                file_id,
                field: "SIMIO p2 (addressing mode)".to_string(),
                value: format!("{:02X}", self.p2),
                expected: "absolute addressing mode (04)".to_string(),
            });
        }
        if self.p1 == 0 {
            return Err(XmlProfileError::InvalidValue {
                file_id,
                field: "SIMIO p1 (record number)".to_string(),
                value: "0".to_string(),
                expected: "1-indexed record number between 1 and 255".to_string(),
            });
        }
        Ok(self.p1 as usize)
    }
}

/// Helper struct holding the result of parsing an Application Dedicated File
/// (ADF) from the XML, separating the ADF representation itself from its nested
/// filesystem members and sub-ADFs.
pub struct ParsedAdf {
    pub adf: ApplicationDedicatedFile,
    pub fs_members: Vec<SimFile>,
    pub nested_adfs: Vec<ApplicationDedicatedFile>,
}

impl TryFrom<XmlApplicationDedicatedFile> for ParsedAdf {
    type Error = XmlProfileError;
    fn try_from(xml_adf: XmlApplicationDedicatedFile) -> Result<Self, Self::Error> {
        let mut cgla = Vec::new();
        let mut csim = Vec::new();
        let mut files = Vec::new();
        let mut fs_members = Vec::new();
        let mut nested_adfs = Vec::new();

        let aid = xml_adf.aid.to_ascii_uppercase();

        for member in xml_adf.members {
            match member {
                XmlApplicationDedicatedFileMember::Cgla(m) => {
                    let norm_cmd = ParsedApdu::parse_mapped(&m.cmd).map_err(|e| {
                        XmlProfileError::InvalidApduCommand { command: m.cmd.clone(), error: e }
                    })?;
                    cgla.push(ApduMapping { cmd: norm_cmd, response: m.response });
                }
                XmlApplicationDedicatedFileMember::Csim(m) => {
                    let norm_cmd = ParsedApdu::parse_mapped(&m.cmd).map_err(|e| {
                        XmlProfileError::InvalidApduCommand { command: m.cmd.clone(), error: e }
                    })?;
                    csim.push(ApduMapping { cmd: norm_cmd, response: m.response });
                }
                XmlApplicationDedicatedFileMember::FileOverride(f) => {
                    let file_cgla = f
                        .members
                        .into_iter()
                        .map(|file_member| match file_member {
                            XmlApplicationFileOverrideMember::Cgla(m) => {
                                let norm_cmd = ParsedApdu::parse_mapped(&m.cmd).map_err(|e| {
                                    XmlProfileError::InvalidApduCommand {
                                        command: m.cmd.clone(),
                                        error: e,
                                    }
                                })?;
                                Ok(ApduMapping { cmd: norm_cmd, response: m.response })
                            }
                        })
                        .collect::<Result<Vec<_>, XmlProfileError>>()?;
                    files.push(ApplicationFileOverride { id: f.id, cgla: file_cgla });
                }
                XmlApplicationDedicatedFileMember::DedicatedFile(xml_df) => {
                    let (dedicated_file, sub_adfs) = convert_xml_dedicated_file(xml_df)?;
                    fs_members.push(SimFile::DedicatedFile(dedicated_file));
                    nested_adfs.extend(sub_adfs);
                }
                XmlApplicationDedicatedFileMember::ElementaryFile(xml_ef) => {
                    if !xml_ef.is_file_not_found() {
                        fs_members.push(SimFile::ElementaryFile(ElementaryFile::try_from(xml_ef)?));
                    }
                }
            }
        }

        Ok(ParsedAdf {
            adf: ApplicationDedicatedFile { aid, cgla, csim, files },
            fs_members,
            nested_adfs,
        })
    }
}

/// Converts an XML-deserialized `XmlDedicatedFile` into the internal
/// `DedicatedFile` representation.
///
/// It also recursively parses nested Dedicated Files and extracts any nested
/// Application Dedicated Files (ADFs) found within the hierarchy, returning
/// them in a flat list.
pub fn convert_xml_dedicated_file(
    xml_df: XmlDedicatedFile,
) -> Result<(DedicatedFile, Vec<ApplicationDedicatedFile>), XmlProfileError> {
    let mut adfs = Vec::new();
    let mut sub_files = Vec::new();

    for member in xml_df.members {
        match member {
            XmlDedicatedFileMember::Dedicated(xml_sub_df) => {
                let (dedicated_file, nested_adfs) = convert_xml_dedicated_file(xml_sub_df)?;
                sub_files.push(SimFile::DedicatedFile(dedicated_file));
                adfs.extend(nested_adfs);
            }
            XmlDedicatedFileMember::Elementary(ef) => {
                if !ef.is_file_not_found() {
                    sub_files.push(SimFile::ElementaryFile(ElementaryFile::try_from(ef)?));
                }
            }
            XmlDedicatedFileMember::ApplicationDedicated(xml_adf) => {
                let file_id = xml_adf.path.unwrap_or(UiccFileId::AdfDefault.as_u16());
                let parsed_adf = ParsedAdf::try_from(xml_adf)?;

                adfs.push(parsed_adf.adf);
                adfs.extend(parsed_adf.nested_adfs);

                let df = DedicatedFile { file_id, files: parsed_adf.fs_members };
                sub_files.push(SimFile::DedicatedFile(df));
            }
        }
    }

    let file_id = xml_df.path.unwrap_or(UiccFileId::MasterFile.as_u16());
    Ok((DedicatedFile { file_id, files: sub_files }, adfs))
}

/// Holds record data and structure metadata inferred from a sequence of SIMIO
/// commands for a linear fixed Elementary File (EF).
///
/// Since the XML profile defines files by their response to low-level APDUs
/// (SIMIO), we must reconstruct the high-level file structure (record size and
/// count) by analyzing these commands.
struct InferredRecords {
    file_id: u16,
    /// The record data, mapping 1-indexed record number to its byte payload.
    records: Vec<(usize, Vec<u8>)>,
    /// The maximum record number observed (defines the file size in records).
    max_rec_num: usize,
    /// The inferred size of each record in bytes (must be consistent across all
    /// records).
    rec_len: usize,
}

impl InferredRecords {
    /// Compiles the individual record payloads into a single contiguous byte
    /// vector representing the entire file content.
    ///
    /// Empty or missing records are padded with 0xFF bytes.
    fn compile(self) -> Result<Vec<u8>, XmlProfileError> {
        let total_len = self.max_rec_num * self.rec_len;
        let mut data_bytes = vec![0xFF; total_len];
        for (num, rec_data) in self.records {
            if num == 0 {
                return Err(XmlProfileError::InvalidValue {
                    file_id: self.file_id,
                    field: "SIMIO p1 (record number)".to_string(),
                    value: "0".to_string(),
                    expected: "1-indexed record number between 1 and 255".to_string(),
                });
            }
            // 1-indexed record number to 0-indexed buffer offset
            let start = (num - 1) * self.rec_len;
            let record_slot = data_bytes.get_mut(start..start + self.rec_len).ok_or_else(|| {
                XmlProfileError::InvalidValue {
                    file_id: self.file_id,
                    field: "SIMIO record offset".to_string(),
                    value: format!("start={start}, rec_len={}", self.rec_len),
                    expected: format!("offset within total file size of {total_len} bytes"),
                }
            })?;
            for (dst, &src) in record_slot.iter_mut().zip(&rec_data) {
                *dst = src;
            }
        }
        Ok(data_bytes)
    }
}

/// Analyzes SIMIO read mappings (command B2) to extract record data and infer
/// the record size and count.
///
/// It validates that all read mappings use absolute addressing (P2 = 04) and
/// have consistent payload lengths.
fn infer_records_from_read(
    simio_mappings: &[SimIoMapping],
    file_id: u16,
) -> Result<InferredRecords, XmlProfileError> {
    let mut records = Vec::new();
    let mut max_rec_num = 0;
    let mut inferred_rec_len = 0;

    for m in simio_mappings {
        if m.cmd == apdu::Instruction::ReadRecord {
            let rec_num_usize = m.validate_record_addressing(file_id)?;
            max_rec_num = std::cmp::max(max_rec_num, rec_num_usize);

            let mut parts = m.response.split(',');
            if let (Some(_sw1), Some(_sw2), Some(payload), None) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            {
                let rec_data = payload.trim().to_ascii_uppercase();
                if !rec_data.len().is_multiple_of(2) {
                    return Err(XmlProfileError::InvalidValue {
                        file_id,
                        field: "SIMIO B2 response length".to_string(),
                        value: rec_data.len().to_string(),
                        expected: "even number of hex characters".to_string(),
                    });
                }
                let rec_bytes =
                    hex::decode(&rec_data).map_err(|e| XmlProfileError::InvalidValue {
                        file_id,
                        field: "SIMIO B2 response".to_string(),
                        value: rec_data.clone(),
                        expected: format!("valid hex string: {e}"),
                    })?;
                let current_len = rec_bytes.len();
                if current_len > 0 {
                    // Enforce that all records in the same file have the same length
                    if inferred_rec_len > 0 && current_len != inferred_rec_len {
                        return Err(XmlProfileError::InvalidValue {
                            file_id,
                            field: "SIMIO B2 response length".to_string(),
                            value: current_len.to_string(),
                            expected: format!("consistent record size of {inferred_rec_len} bytes"),
                        });
                    }
                    inferred_rec_len = current_len;
                }
                records.push((rec_num_usize, rec_bytes));
            } else {
                return Err(XmlProfileError::InvalidSimIoResponse {
                    file_id,
                    response: m.response.clone(),
                });
            }
        }
    }
    Ok(InferredRecords { file_id, records, max_rec_num, rec_len: inferred_rec_len })
}

/// Analyzes SIMIO update mappings (command DC) to infer record size and count
/// when read mappings are absent.
///
/// This serves as a fallback to determine the file structure metadata (record
/// size/count) even if the XML only defines write operations.
fn infer_records_from_update(
    simio_mappings: &[SimIoMapping],
    file_id: u16,
    existing_rec_len: usize,
) -> Result<(usize, usize), XmlProfileError> {
    let mut max_rec_num = 0;
    let mut inferred_rec_len = existing_rec_len;

    for m in simio_mappings {
        if m.cmd == apdu::Instruction::UpdateRecord {
            let rec_num_usize = m.validate_record_addressing(file_id)?;
            max_rec_num = std::cmp::max(max_rec_num, rec_num_usize);

            let current_len = m.p3 as usize; // P3 defines record length in UPDATE RECORD
            if inferred_rec_len > 0 && current_len != inferred_rec_len {
                return Err(XmlProfileError::InvalidValue {
                    file_id,
                    field: "SIMIO p3 (record size)".to_string(),
                    value: current_len.to_string(),
                    expected: format!("consistent record size of {inferred_rec_len} bytes"),
                });
            }
            inferred_rec_len = current_len;
        }
    }
    Ok((max_rec_num, inferred_rec_len))
}

struct FcpDescriptor {
    is_linear_fixed: bool,
    record_len: Option<usize>,
    num_records: Option<usize>,
    file_size: Option<usize>,
}

fn parse_fcp_response(response: &str) -> Option<FcpDescriptor> {
    let mut parts = response.split(',');
    let _sw1 = parts.next()?;
    let _sw2 = parts.next()?;
    let hex_data = parts.next()?.trim();
    let bytes = hex::decode(hex_data).ok()?;

    let &[0x62, _total_fcp_len, ref rest @ ..] = bytes.as_slice() else {
        return None;
    };
    let mut remaining = rest;

    let mut is_linear_fixed = false;
    let mut record_len = None;
    let mut num_records = None;
    let mut file_size = None;

    while let &[tag, len_u8, ref rest @ ..] = remaining {
        let Some((val, next)) = rest.split_at_checked(len_u8 as usize) else {
            break;
        };
        remaining = next;

        match tag {
            0x82 => {
                if let Some(&first) = val.first()
                    && (first & 0x07) == 0x02
                {
                    is_linear_fixed = true;
                }
                if let Some(&chunk) = val.get(2..).and_then(|s| s.first_chunk::<2>()) {
                    let r_len = u16::from_be_bytes(chunk) as usize;
                    if r_len > 0 {
                        record_len = Some(r_len);
                    }
                }
                if let Some(&count) = val.get(4) {
                    num_records = Some(count as usize);
                }
            }
            0x80 => {
                if let Some(&chunk) = val.first_chunk::<2>() {
                    file_size = Some(u16::from_be_bytes(chunk) as usize);
                }
            }
            _ => {}
        }
    }

    Some(FcpDescriptor { is_linear_fixed, record_len, num_records, file_size })
}

impl XmlSimIo {
    /// Extracts the APDU status word (SW1, SW2) from the response string.
    ///
    /// Status words in Android modem XML profiles are formatted as
    /// comma-separated decimal integers (e.g. "106,130" for 0x6A82).
    pub fn status_word(&self) -> Option<u16> {
        let mut parts = self.response.split(',');
        let sw1 = parts.next()?.trim().parse::<u8>().ok()?;
        let sw2 = parts.next()?.trim().parse::<u8>().ok()?;
        Some(u16::from_be_bytes([sw1, sw2]))
    }
}

impl XmlElementaryFile {
    /// Returns true if this EF indicates the file is not found on the SIM card.
    pub fn is_file_not_found(&self) -> bool {
        self.members.iter().any(|m| match m {
            XmlElementaryFileMember::Simio(s) => {
                s.status_word() == Some(crate::constants::SW_FILE_NOT_FOUND)
            }
            _ => false,
        })
    }
}

impl TryFrom<XmlElementaryFile> for ElementaryFile {
    type Error = XmlProfileError;
    fn try_from(xml_ef: XmlElementaryFile) -> Result<Self, Self::Error> {
        let mut simio_mappings = Vec::new();
        let mut data = Vec::new();
        let file_id = xml_ef.id;

        for member in xml_ef.members {
            match member {
                XmlElementaryFileMember::Simio(m) => {
                    simio_mappings.push(SimIoMapping {
                        cmd: m.command,
                        p1: m.p1,
                        p2: m.p2,
                        p3: m.p3,
                        response: m.response,
                    });
                }
                XmlElementaryFileMember::Ccid(c) => {
                    if !c.chars().all(|ch| ch.is_ascii_digit()) {
                        return Err(XmlProfileError::InvalidValue {
                            file_id,
                            field: "CCID".to_string(),
                            value: c,
                            expected: "decimal digits only".to_string(),
                        });
                    }
                    data = crate::pdu::bcd::string_to_bcd(&c);
                }
                XmlElementaryFileMember::Cimi(c) => {
                    let encoded = crate::pdu::bcd::encode_imsi(&c).ok_or_else(|| {
                        XmlProfileError::InvalidValue {
                            file_id,
                            field: "CIMI".to_string(),
                            value: c,
                            expected: "decimal digits only".to_string(),
                        }
                    })?;
                    data = encoded;
                }
            }
        }

        if let Some(b0_map) = simio_mappings.iter().find(|m| m.cmd == apdu::Instruction::ReadBinary)
        {
            if b0_map.p1 != 0 || b0_map.p2 != 0 {
                return Err(XmlProfileError::InvalidValue {
                    file_id,
                    field: "SIMIO p1/p2 (offset)".to_string(),
                    value: format!("p1={}, p2={}", b0_map.p1, b0_map.p2),
                    expected: "offset 0 (p1=0, p2=0)".to_string(),
                });
            }
            let mut parts = b0_map.response.split(',');
            if let (Some(_sw1), Some(_sw2), Some(data_part), None) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            {
                let trimmed = data_part.trim().to_ascii_uppercase();
                if !trimmed.len().is_multiple_of(2) {
                    return Err(XmlProfileError::InvalidValue {
                        file_id,
                        field: "SIMIO B0 response length".to_string(),
                        value: trimmed.len().to_string(),
                        expected: "even number of hex characters".to_string(),
                    });
                }
                data = hex::decode(&trimmed).map_err(|e| XmlProfileError::InvalidValue {
                    file_id,
                    field: "SIMIO B0 response".to_string(),
                    value: trimmed,
                    expected: format!("valid hex string: {e}"),
                })?;
            } else {
                return Err(XmlProfileError::InvalidSimIoResponse {
                    file_id,
                    response: b0_map.response.clone(),
                });
            }
        }

        let fcp = simio_mappings
            .iter()
            .find(|m| m.cmd == apdu::Instruction::GetResponse)
            .and_then(|m| parse_fcp_response(&m.response));

        let canonical_len =
            UiccFileId::try_from(file_id).ok().and_then(UiccFileId::default_record_len);

        let structure = xml_ef
            .structure
            .or_else(|| {
                if fcp.as_ref().is_some_and(|f| f.is_linear_fixed) || canonical_len.is_some() {
                    Some(XmlFileStructure::LinearFixed)
                } else {
                    None
                }
            })
            .unwrap_or(XmlFileStructure::Transparent);

        let mut record_len = None;

        if structure == XmlFileStructure::LinearFixed {
            let mut inferred = infer_records_from_read(&simio_mappings, file_id)?;

            let (update_max_rec_num, update_inferred_rec_len) =
                infer_records_from_update(&simio_mappings, file_id, inferred.rec_len)?;
            inferred.max_rec_num = std::cmp::max(inferred.max_rec_num, update_max_rec_num);
            inferred.rec_len = update_inferred_rec_len;

            if inferred.max_rec_num > 0 && inferred.rec_len > 0 {
                record_len = Some(inferred.rec_len);
                data = inferred.compile()?;
            } else if let Some(fcp_desc) = fcp
                && let Some(r_len) = fcp_desc.record_len
            {
                record_len = Some(r_len);
                let total_size = fcp_desc
                    .file_size
                    .or_else(|| fcp_desc.num_records.map(|n| n * r_len))
                    .unwrap_or(r_len);
                if data.is_empty() {
                    data = vec![0xFF; total_size];
                }
            }

            if record_len.is_none() {
                record_len = canonical_len;
            }

            if record_len.is_none() {
                return Err(XmlProfileError::InvalidValue {
                    file_id,
                    field: "structure".to_string(),
                    value: "linear fixed".to_string(),
                    expected: "determined record length (>0)".to_string(),
                });
            }
        }

        Ok(ElementaryFile { file_id, record_len, data })
    }
}

impl From<XmlSetupMenu> for StkMenuItem {
    fn from(xml_menu: XmlSetupMenu) -> Self {
        let items = xml_menu.items.into_iter().map(StkMenuItem::from).collect();
        StkMenuItem { id: 0, menu_id: 0, text: xml_menu.text, items }
    }
}

impl From<XmlDisplayText> for StkMenuItem {
    fn from(xml_text: XmlDisplayText) -> Self {
        StkMenuItem {
            id: xml_text.id.unwrap_or(0),
            menu_id: xml_text.menu_id,
            text: xml_text.text,
            items: Vec::new(),
        }
    }
}

impl From<XmlSelectItem> for StkMenuItem {
    fn from(xml_item: XmlSelectItem) -> Self {
        let items = xml_item
            .sub_items
            .into_iter()
            .map(|sub| match sub {
                XmlSelectItemOrDisplayText::SelectItem(it) => StkMenuItem::from(it),
                XmlSelectItemOrDisplayText::DisplayText(dt) => StkMenuItem::from(dt),
            })
            .collect();
        StkMenuItem {
            id: xml_item.id.unwrap_or(0),
            menu_id: xml_item.menu_id,
            text: xml_item.text,
            items,
        }
    }
}
