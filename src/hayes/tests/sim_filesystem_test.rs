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

use crate::{common::constants::*, world::World};

// Scenario: Read ICCID from SIM Filesystem
//   Given a modem "A"
//   When AT command 'AT+CRSM=176,12258,0,0,10' is sent to "A"
//   Then response from "A" is '+CRSM: 144,0,89012345678901234567'
//   And response from "A" is "OK"
#[test]
fn test_read_iccid() {
    let mut world = World::new();
    world.given_modem_with_sim_profile("A");
    world.send_and_expect(
        "A",
        "AT+CRSM=176,12258,0,0,10",
        &[&format!("+CRSM: 144,0,{}", TEST_ICCID_SWAPPED), "OK"],
    );
}

// Scenario: Select Master File
//   Given a modem "A"
//   When AT command 'AT+CRSM=164,16128,0,0,0' is sent to "A"
//   Then response from "A" is '+CRSM: 144,0,6210'
//   And response from "A" is "OK"
#[test]
fn test_select_mf() {
    let mut world = World::new();
    world.given_modem_with_sim_profile("A");
    world.send_and_expect("A", "AT+CRSM=164,16128,0,0,0", &["+CRSM: 144,0,6210", "OK"]);
}

#[test]
fn test_sim_io_file_not_found() {
    let mut world = World::new();
    world.given_modem_with_sim_profile("A");
    // Query a dummy file ID 9999 (0x270F) which doesn't exist
    world.send_and_expect("A", "AT+CRSM=176,9999,0,0,10", &["+CRSM: 106,130", "OK"]);
}

#[test]
fn test_read_iccid_from_xml() {
    let mut world = World::new();
    let xml = r#"<IccProfile>
    <MF path="3F00">
        <EF name="EF_ICCID" id="2FE2" structure="transparent">
            <SIMIO cmd="B0" p1="0" p2="0" p3="A" data="">144,0,89860318640220133897</SIMIO>
            <CCID>89860318640220133897</CCID>
        </EF>
    </MF>
</IccProfile>"#;
    world.given_modem_with_xml_profile("A", xml);
    world.send_and_expect(
        "A",
        "AT+CRSM=176,12258,0,0,10",
        &["+CRSM: 144,0,98683081462002318379", "OK"],
    );
}
