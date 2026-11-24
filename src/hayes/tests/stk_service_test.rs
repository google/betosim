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

use crate::{steps::*, world::World};

const SETUP_MENU_URC: &str = concat!(
    "+CUSATP: ",
    "D0",
    "48", // Proactive Command: Tag D0, Len 72 (0x48)
    "81",
    "03",
    "012500", // Command Details: Tag 81, Len 03, Type 25 (SETUP MENU)
    "82",
    "02",
    "8182", // Device IDs: UICC (81) to ME (82)
    "85",
    "0D",
    "800054004D006F0062006C0065", // Alpha ID: "TMoble" (upstream profile typo)
    "8F",
    "18",
    "50",
    "8000530049004D00200054006F006F006C006B00690074", // Item 1: ID 0x50, "SIM Toolkit"
    "8F",
    "14",
    "4E",
    "80005500530049004D00200043006100720064" // Item 2: ID 0x4E, "USIM Card"
);

// Scenario: STK Display Text
//   Given a modem "A"
//   When AT command 'AT+CUSATE="D1150121810D050448656C6C6F20576F726C64"' is
// sent to "A"   Then response from "A" is '+CUSATE: 0'
//   And response from "A" is 'OK'
//   And response from "A" is '+CUSATP: "9000"'
#[test]
fn test_stk_display_text() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // This is a simplified "Display Text" proactive command envelope.
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATE=\"",
            "D1",
            "15", // Mock Envelope: Tag D1 (SMS-PP), Length 21 (0x15)
            "01",
            "21",                     // Simplified Cmd Details: Tag 01, Type 21 (DISPLAY TEXT)
            "810D0504",               // Mock tags (Device IDs, Alpha ID)
            "48656C6C6F20576F726C64", // "Hello World" (ASCII Hex)
            "\""
        ),
    );

    then_response_is(&mut world, "A", "+CUSATE: 0");
    then_response_is(&mut world, "A", "OK");
    then_response_is(&mut world, "A", "+CUSATP: 9000");
}

// Scenario: Send STK Envelope Command
//   Given a modem "A"
//   When AT command 'AT+CUSATE="D30782028281100150"' is sent to "A"
//   Then response from "A" is '+CUSATE: 0'
//   And response from "A" is 'OK'
//   And response from "A" is '+CUSATP:
// "D02D8103012400820281828F0A018053E34EE48BBE7F6E8F0A02804E1A52A14ECB7ECD8F0A0380724867434FE1606F"
// '
#[test]
fn test_send_stk_envelope_command() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Select item 0x50 (SIM) from main menu (Source: ME/82, Dest: UICC/81)
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATE=\"",
            "D3",
            "07", // Envelope: Tag D3 (Menu Selection), Length 07
            "82",
            "02",
            "8281", // Device IDs: Tag 82, Len 02, ME (82) to UICC (81)
            "10",
            "01",
            "50", // Item Identifier: Tag 10, Len 01, Item ID 50 (SIM)
            "\""
        ),
    );

    then_response_is(&mut world, "A", "+CUSATE: 0");
    then_response_is(&mut world, "A", "OK");
    then_response_is(
        &mut world,
        "A",
        concat!(
            "+CUSATP: ",
            "D0",
            "2D", // Proactive Command: Tag D0, Len 45 (0x2D)
            "81",
            "03",
            "012400", // Command Details: Tag 81, Len 03, Num 01, Type 24 (SELECT ITEM), Qual 00
            "82",
            "02",
            "8182", // Device IDs: Tag 82, Len 02, UICC (81) to ME (82)
            "8F",
            "0A",
            "01",
            "80006D0065006E0075", // Item 1: ID 01, "menu"
            "8F",
            "0A",
            "02",
            "800049006E0066006F", // Item 2: ID 02, "Info"
            "8F",
            "0A",
            "03",
            "8000480065006C0070" // Item 3: ID 03, "Help"
        ),
    );
}

// Scenario: STK Get Input
//   Given a modem "A"
//   When AT command 'AT+CUSATE="D1150123810D0504456E7465722054657874"' is sent
// to "A"   Then response from "A" is '+CUSATE: 0'
//   And response from "A" is 'OK'
//   And response from "A" is '+CUSATP: "9000"'
#[test]
fn test_stk_get_input() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // This is a simplified "Get Input" proactive command envelope.
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATE=\"",
            "D1",
            "15", // Mock Envelope: Tag D1 (SMS-PP), Length 21 (0x15)
            "01",
            "23",                   // Simplified Cmd Details: Tag 01, Type 23 (GET INPUT)
            "810D0504",             // Mock tags (Device IDs, Alpha ID)
            "456E7465722054657874", // "Enter Text" (ASCII Hex)
            "\""
        ),
    );

    then_response_is(&mut world, "A", "+CUSATE: 0");
    then_response_is(&mut world, "A", "OK");
    then_response_is(&mut world, "A", "+CUSATP: 9000");
}

// Scenario: Query STK Ready
//   Given a modem "A"
//   When AT command "AT+CUSATD?" is sent to "A"
//   Then response from "A" is "+CUSATD: 1, 1"
//   And response from "A" is "OK"
#[test]
fn test_query_stk_ready() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(&mut world, "A", "AT+CUSATD?");

    then_response_is(&mut world, "A", "+CUSATD: 1, 1");
    then_response_is(&mut world, "A", "OK");
    then_response_is(
        &mut world,
        "A",
        concat!(
            "+CUSATP: ",
            "D0",
            "48", // Proactive Command: Tag D0, Len 72 (0x48)
            "81",
            "03",
            "012500", // Command Details: Tag 81, Len 03, Type 25 (SETUP MENU)
            "82",
            "02",
            "8182", // Device IDs: UICC (81) to ME (82)
            "85",
            "0D",
            "800054004D006F0062006C0065", // Alpha ID: "TMoble" (upstream profile typo)
            "8F",
            "18",
            "50",
            "8000530049004D00200054006F006F006C006B00690074", // Item 1: ID 0x50, "SIM Toolkit"
            "8F",
            "14",
            "4E",
            "80005500530049004D00200043006100720064" // Item 2: ID 0x4E, "USIM Card"
        ),
    );
}

#[test]
fn test_set_stk_ready() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Enable STK and pass profile hex string, should trigger SET UP MENU URC
    when_at_command_sent(&mut world, "A", "AT+CUSATD=1,\"010203\"");

    then_response_is(&mut world, "A", "OK");
    // Expect SET UP MENU URC from the default profile
    then_response_is(&mut world, "A", SETUP_MENU_URC);
}

#[test]
fn test_send_stk_terminal_response() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATT=\"",
            "81",
            "03",
            "012100", // Command Details: Tag 81, Len 03, Num 01, Type 21 (DISPLAY TEXT), Qual 00
            "82",
            "02",
            "8281", // Device IDs: Tag 82, Len 02, ME (82) to UICC (81)
            "83",
            "01",
            "00", // Result: Tag 83, Len 01, Success (00)
            "\""
        ),
    );

    // Expect +CUSATT: 0, OK, and +CUSATEND (since it's a success TR for DISPLAY
    // TEXT)
    then_response_is(&mut world, "A", "+CUSATT: 0");
    then_response_is(&mut world, "A", "OK");
    then_response_is(&mut world, "A", "+CUSATEND");
}

#[test]
fn test_stk_submenu_navigation() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // 1. Enable STK
    when_at_command_sent(&mut world, "A", "AT+CUSATD=1,\"010203\"");
    then_response_is(&mut world, "A", "OK");
    // Setup Menu URC
    then_response_is(&mut world, "A", SETUP_MENU_URC);

    // 2. Select "SIM" (Item 0x50) from main menu via Envelope (Source ME: 82)
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATE=\"",
            "D3",
            "07", // Envelope: Tag D3 (Menu Selection), Length 07
            "82",
            "02",
            "8281", // Device IDs: Tag 82, Len 02, ME (82) to UICC (81)
            "10",
            "01",
            "50", // Item Identifier: Tag 10, Len 01, Item ID 50 (SIM)
            "\""
        ),
    );
    then_response_is(&mut world, "A", "+CUSATE: 0");
    then_response_is(&mut world, "A", "OK");
    // Expect SELECT ITEM URC for submenu
    then_response_is(
        &mut world,
        "A",
        concat!(
            "+CUSATP: ",
            "D0",
            "2D", // Proactive Command: Tag D0, Len 45 (0x2D)
            "81",
            "03",
            "012400", // Command Details: Tag 81, Len 03, Num 01, Type 24 (SELECT ITEM), Qual 00
            "82",
            "02",
            "8182", // Device IDs: Tag 82, Len 02, UICC (81) to ME (82)
            "8F",
            "0A",
            "01",
            "80006D0065006E0075", // Item 1: ID 01, "menu"
            "8F",
            "0A",
            "02",
            "800049006E0066006F", // Item 2: ID 02, "Info"
            "8F",
            "0A",
            "03",
            "8000480065006C0070" // Item 3: ID 03, "Help"
        ),
    );

    // 3. Select "SubMenu1" (Item 0x01) from the submenu via Terminal Response
    // TR payload: Command Details (Type 24, Cmd Num 01), Device IDs (ME to UICC),
    // Result (Success, Item ID 01) Hex: 81030124008202828183020001
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATT=\"",
            "81",
            "03",
            "012400", // Command Details: SELECT ITEM (0x24)
            "82",
            "02",
            "8281", // Device IDs: ME (82) to UICC (81)
            "83",
            "02",
            "0001", // Result: Success (00), Item ID (01)
            "\""
        ),
    );
    then_response_is(&mut world, "A", "+CUSATT: 0");
    then_response_is(&mut world, "A", "OK");
    // Expect SELECT ITEM URC for sub-submenu
    then_response_is(
        &mut world,
        "A",
        concat!(
            "+CUSATP: ",
            "D0",
            "31", // Proactive Command: Tag D0, Len 49 (0x31)
            "81",
            "03",
            "012400", // Command Details: SELECT ITEM (0x24)
            "82",
            "02",
            "8182", // Device IDs: UICC (81) to ME (82)
            "8F",
            "12",
            "01",
            "800041006400640020006D0065006E0075", // Item 1: ID 01, "Add menu"
            "8F",
            "12",
            "02",
            "8000440065006C0020006D0065006E0075" // Item 2: ID 02, "Del menu"
        ),
    );

    // 4. Select "DisplayText1" (Item 0x01) from sub-submenu via Terminal Response
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATT=\"",
            "81",
            "03",
            "012400", // Command Details: SELECT ITEM (0x24)
            "82",
            "02",
            "8281", // Device IDs: ME (82) to UICC (81)
            "83",
            "02",
            "0001", // Result: Success (00), Item ID (01)
            "\""
        ),
    );
    then_response_is(&mut world, "A", "+CUSATT: 0");
    then_response_is(&mut world, "A", "OK");
    // Expect DISPLAY TEXT URC (leaf action)
    then_response_is(
        &mut world,
        "A",
        concat!(
            "+CUSATP: ",
            "D0",
            "2A", // Proactive Command: Tag D0, Len 42 (0x2A)
            "81",
            "03",
            "012181", // Command Details: Tag 81, Len 03, Type 21 (DISPLAY TEXT), Qual 81
            "82",
            "02",
            "8102", // Device IDs: UICC (81) to Display (02)
            "8D",
            "1F", // Text String: Tag 8D, Len 31
            "08", // DCS: 08 (UCS2)
            "004E006F007400200069006D0070006C0065006D0065006E007400650064"  /* Text: "Not
                   * implemented" */
        ),
    );

    // 5. Send Terminal Response for DISPLAY TEXT (Type 21)
    // TR payload: Command Details (Type 21, Cmd Num 01), Device IDs (ME to UICC),
    // Result (Success) Hex: 810301210082028281830100
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATT=\"",
            "81",
            "03",
            "012100", // Command Details: DISPLAY TEXT (0x21)
            "82",
            "02",
            "8281", // Device IDs: ME (82) to UICC (81)
            "83",
            "01",
            "00", // Result: Success (00)
            "\""
        ),
    );
    then_response_is(&mut world, "A", "+CUSATT: 0");
    then_response_is(&mut world, "A", "OK");
    // Expect +CUSATEND URC because the leaf action completed
    then_response_is(&mut world, "A", "+CUSATEND");
}

#[test]
fn test_stk_session_terminated_by_user() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // 1. Enable STK
    when_at_command_sent(&mut world, "A", "AT+CUSATD=1,\"010203\"");
    then_response_is(&mut world, "A", "OK");
    then_response_is(&mut world, "A", SETUP_MENU_URC);

    // 2. Select "SIM" (Item 0x50) to go to submenu
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATE=\"",
            "D3",
            "07", // Envelope: Tag D3 (Menu Selection), Length 07
            "82",
            "02",
            "8281", // Device IDs: Tag 82, Len 02, ME (82) to UICC (81)
            "10",
            "01",
            "50", // Item Identifier: Tag 10, Len 01, Item ID 50 (SIM)
            "\""
        ),
    );
    then_response_is(&mut world, "A", "+CUSATE: 0");
    then_response_is(&mut world, "A", "OK");
    then_response_is(
        &mut world,
        "A",
        concat!(
            "+CUSATP: ",
            "D0",
            "2D", // Proactive Command: Tag D0, Len 45 (0x2D)
            "81",
            "03",
            "012400", // Command Details: Tag 81, Len 03, Num 01, Type 24 (SELECT ITEM), Qual 00
            "82",
            "02",
            "8182", // Device IDs: Tag 82, Len 02, UICC (81) to ME (82)
            "8F",
            "0A",
            "01",
            "80006D0065006E0075", // Item 1: ID 01, "menu"
            "8F",
            "0A",
            "02",
            "800049006E0066006F", // Item 2: ID 02, "Info"
            "8F",
            "0A",
            "03",
            "8000480065006C0070" // Item 3: ID 03, "Help"
        ),
    );

    // 3. Send Terminal Response with Session Terminated by User (Result 0x10)
    // Hex: 810301240082028281830110
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATT=\"",
            "81",
            "03",
            "012400", // Command Details: SELECT ITEM (0x24)
            "82",
            "02",
            "8281", // Device IDs: ME (82) to UICC (81)
            "83",
            "01",
            "10", // Result: Session Terminated by User (0x10)
            "\""
        ),
    );
    then_response_is(&mut world, "A", "+CUSATT: 0");
    then_response_is(&mut world, "A", "OK");
    // Expect +CUSATEND URC
    then_response_is(&mut world, "A", "+CUSATEND");

    // 4. Verify we are back to main menu. Selecting "SIM" (0x50) again should
    //    succeed.
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATE=\"",
            "D3",
            "07", // Envelope: Tag D3 (Menu Selection), Length 07
            "82",
            "02",
            "8281", // Device IDs: Tag 82, Len 02, ME (82) to UICC (81)
            "10",
            "01",
            "50", // Item Identifier: Tag 10, Len 01, Item ID 50 (SIM)
            "\""
        ),
    );
    then_response_is(&mut world, "A", "+CUSATE: 0");
    then_response_is(&mut world, "A", "OK");
}

#[test]
fn test_stk_backward_move() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // 1. Enable STK
    when_at_command_sent(&mut world, "A", "AT+CUSATD=1,\"010203\"");
    then_response_is(&mut world, "A", "OK");
    then_response_is(&mut world, "A", SETUP_MENU_URC);

    // 2. Select "SIM" (Item 0x50) to go to submenu
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATE=\"",
            "D3",
            "07", // Envelope: Tag D3 (Menu Selection), Length 07
            "82",
            "02",
            "8281", // Device IDs: Tag 82, Len 02, ME (82) to UICC (81)
            "10",
            "01",
            "50", // Item Identifier: Tag 10, Len 01, Item ID 50 (SIM)
            "\""
        ),
    );
    then_response_is(&mut world, "A", "+CUSATE: 0");
    then_response_is(&mut world, "A", "OK");
    then_response_is(
        &mut world,
        "A",
        concat!(
            "+CUSATP: ",
            "D0",
            "2D", // Proactive Command: Tag D0, Len 45 (0x2D)
            "81",
            "03",
            "012400", // Command Details: Tag 81, Len 03, Num 01, Type 24 (SELECT ITEM), Qual 00
            "82",
            "02",
            "8182", // Device IDs: Tag 82, Len 02, UICC (81) to ME (82)
            "8F",
            "0A",
            "01",
            "80006D0065006E0075", // Item 1: ID 01, "menu"
            "8F",
            "0A",
            "02",
            "800049006E0066006F", // Item 2: ID 02, "Info"
            "8F",
            "0A",
            "03",
            "8000480065006C0070" // Item 3: ID 03, "Help"
        ),
    );

    // 3. Select "SubMenu1" (Item 0x01) to go to sub-submenu
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATT=\"",
            "81",
            "03",
            "012400", // Command Details: SELECT ITEM (0x24)
            "82",
            "02",
            "8281", // Device IDs: ME (82) to UICC (81)
            "83",
            "02",
            "0001", // Result: Success (00), Item ID (01)
            "\""
        ),
    );
    then_response_is(&mut world, "A", "+CUSATT: 0");
    then_response_is(&mut world, "A", "OK");
    then_response_is(
        &mut world,
        "A",
        concat!(
            "+CUSATP: ",
            "D0",
            "31", // Proactive Command: Tag D0, Len 49 (0x31)
            "81",
            "03",
            "012400", // Command Details: SELECT ITEM (0x24)
            "82",
            "02",
            "8182", // Device IDs: UICC (81) to ME (82)
            "8F",
            "12",
            "01",
            "800041006400640020006D0065006E0075", // Item 1: ID 0x01, "Add menu"
            "8F",
            "12",
            "02",
            "8000440065006C0020006D0065006E0075" // Item 2: ID 0x02, "Del menu"
        ),
    );

    // 4. Send Terminal Response with Backward Move (Result 0x11)
    // Hex: 810301240082028281830111
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATT=\"",
            "81",
            "03",
            "012400", // Command Details: SELECT ITEM (0x24)
            "82",
            "02",
            "8281", // Device IDs: ME (82) to UICC (81)
            "83",
            "01",
            "11", // Result: Backward Move (0x11)
            "\""
        ),
    );
    then_response_is(&mut world, "A", "+CUSATT: 0");
    then_response_is(&mut world, "A", "OK");
    // Expect SELECT ITEM URC for the parent menu (SIM 0x50)
    then_response_is(
        &mut world,
        "A",
        concat!(
            "+CUSATP: ",
            "D0",
            "2D", // Proactive Command: Tag D0, Len 45 (0x2D)
            "81",
            "03",
            "012400", // Command Details: Tag 81, Len 03, Num 01, Type 24 (SELECT ITEM), Qual 00
            "82",
            "02",
            "8182", // Device IDs: Tag 82, Len 02, UICC (81) to ME (82)
            "8F",
            "0A",
            "01",
            "80006D0065006E0075", // Item 1: ID 01, "menu"
            "8F",
            "0A",
            "02",
            "800049006E0066006F", // Item 2: ID 02, "Info"
            "8F",
            "0A",
            "03",
            "8000480065006C0070" // Item 3: ID 03, "Help"
        ),
    );

    // 5. Verify we are back to submenu. We should be able to select "SubMenu1"
    //    (0x01) again.
    when_at_command_sent(
        &mut world,
        "A",
        concat!(
            "AT+CUSATT=\"",
            "81",
            "03",
            "012400", // Command Details: SELECT ITEM (0x24)
            "82",
            "02",
            "8281", // Device IDs: ME (82) to UICC (81)
            "83",
            "02",
            "0001", // Result: Success (00), Item ID (01)
            "\""
        ),
    );
    then_response_is(&mut world, "A", "+CUSATT: 0");
    then_response_is(&mut world, "A", "OK");
    then_response_is(
        &mut world,
        "A",
        concat!(
            "+CUSATP: ",
            "D0",
            "31", // Proactive Command: Tag D0, Len 49 (0x31)
            "81",
            "03",
            "012400", // Command Details: SELECT ITEM (0x24)
            "82",
            "02",
            "8182", // Device IDs: UICC (81) to ME (82)
            "8F",
            "12",
            "01",
            "800041006400640020006D0065006E0075", // Item 1: ID 0x01, "Add menu"
            "8F",
            "12",
            "02",
            "8000440065006C0020006D0065006E0075" // Item 2: ID 0x02, "Del menu"
        ),
    );
}

#[test]
fn test_stk_command_sim_absent() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Remove SIM using the action step
    when_sim_status_set(&mut world, "A", false);
    then_response_is(&mut world, "A", "+CPIN: ABSENT");

    // Send STK command, should fail with ERROR
    when_at_command_sent(&mut world, "A", "AT+CUSATD?");
    then_response_is(&mut world, "A", "ERROR");
}

#[test]
fn test_stk_envelope_invalid_format() {
    let mut world = World::new();
    given_modem(&mut world, "A");

    // Send malformed envelope (too short)
    when_at_command_sent(&mut world, "A", "AT+CUSATE=\"D3\"");
    then_response_is(&mut world, "A", "ERROR");

    // Send malformed envelope (invalid hex)
    when_at_command_sent(&mut world, "A", "AT+CUSATE=\"D3ZZ\"");
    then_response_is(&mut world, "A", "ERROR");
}
