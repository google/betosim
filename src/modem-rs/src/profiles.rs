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

use crate::config::SimProfile;
pub use crate::xml_profile::{XmlProfileError, parse_xml_profile};

pub static PROFILE_DEFAULT_XML: &str = include_str!("profiles/iccprofile_for_sim0.xml");
pub static PROFILE_CTS_XML: &str =
    include_str!("profiles/iccprofile_for_sim0_for_CtsCarrierApiTestCases.xml");
pub static PROFILE_TEL_ALASKA_XML: &str =
    include_str!("profiles/iccprofile_for_sim_tel_alaska.xml");

pub const SIM_TYPE_DEFAULT: i32 = 1;
pub const SIM_TYPE_CTS: i32 = 2;
pub const SIM_TYPE_TEL_ALASKA: i32 = 3;

/// Returns a built-in `SimProfile` corresponding to a Netsim `sim_type` number.
///
/// Supported types:
/// - `SIM_TYPE_DEFAULT` (1): Standard Android emulator test profile
/// - `SIM_TYPE_CTS` (2): CTS Carrier API test profile (CtsCarrierApiTestCases)
/// - `SIM_TYPE_TEL_ALASKA` (3): Tel Alaska test carrier profile
pub fn get_builtin_profile(sim_type: i32) -> Option<SimProfile> {
    let xml = match sim_type {
        SIM_TYPE_DEFAULT => PROFILE_DEFAULT_XML,
        SIM_TYPE_CTS => PROFILE_CTS_XML,
        SIM_TYPE_TEL_ALASKA => PROFILE_TEL_ALASKA_XML,
        _ => return None,
    };
    parse_xml_profile(xml).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_builtin_profiles() {
        let default_prof =
            get_builtin_profile(SIM_TYPE_DEFAULT).expect("default profile should parse");
        assert_eq!(default_prof.imsi, "310260000000000");

        let cts_prof = get_builtin_profile(SIM_TYPE_CTS).expect("cts profile should parse");
        assert_eq!(cts_prof.home_plmn().as_ref().map(crate::types::Plmn::as_str), Some("310260"));

        let alaska_prof =
            get_builtin_profile(SIM_TYPE_TEL_ALASKA).expect("tel_alaska profile should parse");
        assert_eq!(alaska_prof.imsi, "311740123456789");
        assert_eq!(
            alaska_prof.home_plmn().as_ref().map(crate::types::Plmn::as_str),
            Some("311740")
        );

        assert!(get_builtin_profile(999).is_none());
    }
}
