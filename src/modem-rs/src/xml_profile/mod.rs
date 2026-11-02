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

/// Conversion logic from XML deserialization models to internal representation
/// models.
mod conversion;
/// Error types and implementations for XML profile parsing.
pub mod error;
/// XML deserialization structs matching the SIM XML profile schema.
mod schema;
/// Recursive search helper functions to locate CCID and IMSI within the XML
/// profile filesystem.
mod search;
#[cfg(test)]
mod tests;

use conversion::{ParsedAdf, convert_xml_dedicated_file};
pub use error::XmlProfileError;
use schema::XmlIccProfile;
use search::{find_ccid, find_imsi};
use serde::Deserialize;
use serde_xml_rs::{Deserializer, EventReader, ParserConfig};

use crate::config::{FacilityLocks, FileSystem, PinProfile, SimIo, SimProfile, Stk, StkMenuItem};

/// Parses an XML SIM ICC profile into a SimProfile.
pub fn parse_xml_profile(xml: &str) -> Result<SimProfile, XmlProfileError> {
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

    let xml_profile: XmlIccProfile = XmlIccProfile::deserialize(&mut deserializer)?;

    let XmlIccProfile {
        master_file,
        application_dedicated_files,
        pin_profile,
        facility_lock,
        setup_menu,
        card_profile,
    } = xml_profile;

    let master_file = master_file.ok_or(XmlProfileError::MissingMasterFile)?;
    let iccid = find_ccid(&master_file).unwrap_or_default();
    let imsi = find_imsi(&application_dedicated_files, Some(&master_file)).unwrap_or_default();

    let pin_profile = pin_profile
        .map(|p| PinProfile {
            state: p.pin_state.unwrap_or_default(),
            pin1: p.pin_code.unwrap_or_default(),
            puk1: p.puk_code.unwrap_or_default(),
            pin2: p.pin2_code.unwrap_or_default(),
            puk2: p.puk2_code.unwrap_or_default(),
            pin1_retries: p.pin_remain_times,
            puk1_retries: p.puk_remain_times,
            pin2_retries: p.pin2_remain_times,
            puk2_retries: p.puk2_remain_times,
        })
        .unwrap_or_default();

    let facility_locks = facility_lock
        .map(|l| FacilityLocks {
            sim_lock: l.sim_lock.unwrap_or_else(|| "DISABLE".to_string()),
            fixed_dialing: l.fixed_dialing.unwrap_or_else(|| "DISABLE".to_string()),
        })
        .unwrap_or_default();

    let stk = Stk { setup_menu: setup_menu.map(StkMenuItem::from).unwrap_or_default() };

    let (master_file, mut adfs) = convert_xml_dedicated_file(master_file)?;
    let sim_io = SimIo { file_system: FileSystem { master_file } };

    // Convert flat XML ADFs to config ADFs and merge with nested ones
    for xml_adf in application_dedicated_files {
        let parsed_adf = ParsedAdf::try_from(xml_adf)?;
        adfs.push(parsed_adf.adf);
    }

    let (eid, atr) = card_profile.map(|cp| (cp.eid, cp.atr)).unwrap_or((None, None));

    Ok(SimProfile {
        iccid,
        imsi,
        msisdn: None, /* MSISDN is assigned dynamically by the simulator and
                       * intentionally ignored from XML */
        pin_profile,
        facility_locks,
        stk,
        sim_io,
        enable_unsolicited_urcs: None,
        eid,
        atr,
        adfs,
    })
}
