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

use modem_rs_derive::CommandParser;

use crate::{
    CardState,
    config::{Stk, StkMenuItem},
    parser::QuotedString,
    types::{ExecutionResult, HandledCommand, Parsable},
};

// Constants
const TAG_MENU_SELECTION: u8 = 0xD3;
const TAG_ITEM_IDENTIFIER_1: u8 = 0x10;
const TAG_ITEM_IDENTIFIER_2: u8 = 0x90;
const TAG_COMMAND_DETAILS: u8 = 0x81;
const TAG_COMMAND_DETAILS_ALT: u8 = 0x01;
const TAG_RESULT: u8 = 0x83;
const TAG_RESULT_ALT: u8 = 0x03;

const RESULT_SUCCESS: u8 = 0x00;
const RESULT_SESSION_TERMINATED: u8 = 0x10;
const RESULT_BACKWARD_MOVE: u8 = 0x11;

// Envelope Hex ASCII Tags (used in AT+CUSATE)
const HEX_TAG_SMS_PP_DOWNLOAD: &str = "D1";
const HEX_TAG_MENU_SELECTION: &str = "D3";

// TLV Structure Constants
const TLV_HEADER_LEN: usize = 2; // 1 byte tag + 1 byte length

// Simplified Envelope Fallback Constants (used in tests)
const SIMPLIFIED_ENVELOPE_CMD_TYPE_INDEX: usize = 3;

/// SIM Toolkit (USAT / STK) proactive command type.
///
/// Ref: ETSI TS 102 223 § 9.4 ("Type of command and next action indicator") /
/// 3GPP TS 31.111 § 9.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ProactiveCommandType {
    DisplayText = 0x21,
    GetInput = 0x23,
    SelectItem = 0x24,
    SetUpMenu = 0x25,
}

impl TryFrom<u8> for ProactiveCommandType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x21 => Ok(Self::DisplayText),
            0x23 => Ok(Self::GetInput),
            0x24 => Ok(Self::SelectItem),
            0x25 => Ok(Self::SetUpMenu),
            _ => Err(()),
        }
    }
}

impl From<ProactiveCommandType> for u8 {
    fn from(cmd: ProactiveCommandType) -> Self {
        cmd as u8
    }
}

/// STK (SIM Toolkit) service AT commands.
#[derive(Debug, PartialEq, Clone, Copy, CommandParser)]
pub enum StkCommand<'a> {
    #[command(tag = "AT+CUSATD?")]
    QueryStkReady,
    #[command(tag = "AT+CUSATD=")]
    SetStkReady(bool, Option<QuotedString<'a>>),
    #[command(tag = "AT+CUSATE=")]
    SendStkEnvelope(QuotedString<'a>),
    #[command(tag = "AT+CUSATT=")]
    SendStkTerminalResponse(QuotedString<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StkResponse {
    StkReady { ready: u8, support: u8 },
    UsatEnvelopeResponse(String),
    UsatTerminalResponse(u8),
    UsatProactiveCommand(String),
    UsatSessionEnd,
}

impl std::fmt::Display for StkResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StkResponse::StkReady { ready, support } => {
                write!(f, "+CUSATD: {ready}, {support}\r\n")
            }
            StkResponse::UsatEnvelopeResponse(val) => {
                write!(f, "+CUSATE: {val}\r\n")
            }
            StkResponse::UsatTerminalResponse(val) => {
                write!(f, "+CUSATT: {val}\r\n")
            }
            StkResponse::UsatProactiveCommand(val) => {
                write!(f, "+CUSATP: {val}\r\n")
            }
            StkResponse::UsatSessionEnd => {
                write!(f, "+CUSATEND\r\n")
            }
        }
    }
}

#[derive(Default)]
struct StkExecutionResult {
    response: Option<StkResponse>,
    urcs: Vec<StkResponse>,
}

type StkResult = Result<StkExecutionResult, ExecutionResult>;

impl From<StkExecutionResult> for ExecutionResult {
    fn from(stk_res: StkExecutionResult) -> Self {
        let mut handled = HandledCommand::ok();
        if let Some(resp) = stk_res.response {
            handled.responses.insert(0, resp.into());
        }
        for urc in stk_res.urcs {
            handled.responses.push(urc.into());
        }
        ExecutionResult::Success(handled)
    }
}

pub struct StkService {
    stk_config: Stk,
    // Path of selected menu_ids from root. Empty means main menu.
    current_path: Vec<u8>,
    stk_enabled: bool,
}

#[derive(Debug, PartialEq)]
enum TerminalResponseAction {
    Select(u8),
    Terminate,
    Back,
    EndSession,
    None,
}

impl Default for StkService {
    fn default() -> Self {
        Self::new(Stk::default())
    }
}

impl StkService {
    pub fn new(stk_config: Stk) -> Self {
        let has_menu = !stk_config.setup_menu.text.is_empty();
        Self { stk_config, current_path: Vec::new(), stk_enabled: has_menu }
    }

    fn get_current_items(&self) -> &[StkMenuItem] {
        let mut items = &self.stk_config.setup_menu.items;
        for &item_id in &self.current_path {
            if let Some(item) = items.iter().find(|it| it.menu_id == item_id) {
                items = &item.items;
            } else {
                return &self.stk_config.setup_menu.items;
            }
        }
        items
    }

    fn get_item_at_path(&self, path: &[u8]) -> Option<&StkMenuItem> {
        let mut current_items = &self.stk_config.setup_menu.items;
        let mut last_item = None;
        for &id in path {
            let matched = current_items.iter().find(|item| item.menu_id == id)?;
            last_item = Some(matched);
            current_items = &matched.items;
        }
        last_item
    }

    /// Advances `remaining` over a single BER-TLV element, returning `Some((tag, value))`
    /// if a valid tag-length-value element is present and complete, or `None` otherwise.
    fn next_tlv<'a>(remaining: &mut &'a [u8]) -> Option<(u8, &'a [u8])> {
        let &[tag, len_u8, ref rest @ ..] = *remaining else {
            return None;
        };
        let len = len_u8 as usize;
        if rest.len() < len {
            *remaining = &[];
            return None;
        }
        let (value, next) = rest.split_at(len);
        *remaining = next;
        Some((tag, value))
    }

    fn parse_menu_selection(command: &str) -> Option<u8> {
        let bytes = hex::decode(command).ok()?;

        if bytes.first() != Some(&TAG_MENU_SELECTION) {
            return None;
        }
        let length = *bytes.get(1)? as usize;
        let payload = bytes.get(TLV_HEADER_LEN..TLV_HEADER_LEN + length)?;

        let mut remaining = payload;
        let mut item_id = None;

        while let Some((tag, value)) = Self::next_tlv(&mut remaining) {
            match tag {
                TAG_ITEM_IDENTIFIER_1 | TAG_ITEM_IDENTIFIER_2 => {
                    if let [id] = value {
                        item_id = Some(*id);
                    }
                }
                _ => {}
            }
        }

        item_id
    }

    fn parse_terminal_response(command: &str) -> Option<TerminalResponseAction> {
        let bytes = hex::decode(command).ok()?;

        let mut remaining = bytes.as_slice();
        let mut command_type = None;
        let mut general_result = None;
        let mut additional_info = None;

        while let Some((tag, value)) = Self::next_tlv(&mut remaining) {
            match tag {
                TAG_COMMAND_DETAILS | TAG_COMMAND_DETAILS_ALT => {
                    if let [_, cmd_type, ..] = value {
                        command_type = ProactiveCommandType::try_from(*cmd_type).ok();
                    }
                }
                TAG_RESULT | TAG_RESULT_ALT => match value {
                    [res, info, ..] => {
                        general_result = Some(*res);
                        additional_info = Some(*info);
                    }
                    [res] => {
                        general_result = Some(*res);
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        match (general_result?, command_type) {
            (RESULT_SUCCESS, Some(ProactiveCommandType::SelectItem)) => {
                additional_info.map(TerminalResponseAction::Select)
            }
            (RESULT_SUCCESS, _) => Some(TerminalResponseAction::EndSession),
            (RESULT_SESSION_TERMINATED, _) => Some(TerminalResponseAction::Terminate),
            (RESULT_BACKWARD_MOVE, _) => Some(TerminalResponseAction::Back),
            _ => Some(TerminalResponseAction::None),
        }
    }

    fn handle_envelope_command(&mut self, command: &str) -> StkResult {
        if command.starts_with(HEX_TAG_SMS_PP_DOWNLOAD) {
            // Fallback for Terminal Response simulation in tests (e.g. Display Text, Get
            // Input)
            let bytes = hex::decode(command).map_err(|_err| ExecutionResult::error())?;
            if let Some(&raw_cmd_type) = bytes.get(SIMPLIFIED_ENVELOPE_CMD_TYPE_INDEX) {
                let cmd_type = ProactiveCommandType::try_from(raw_cmd_type);
                if matches!(
                    cmd_type,
                    Ok(ProactiveCommandType::DisplayText | ProactiveCommandType::GetInput)
                ) {
                    return Ok(StkExecutionResult {
                        response: Some(StkResponse::UsatEnvelopeResponse("0".to_string())),
                        urcs: vec![StkResponse::UsatProactiveCommand("9000".to_string())],
                    });
                }
            }
            return Err(ExecutionResult::error());
        }

        if command.starts_with(HEX_TAG_MENU_SELECTION) {
            // Envelope resets path to main menu
            self.current_path.clear();

            let item_id = Self::parse_menu_selection(command).ok_or_else(ExecutionResult::error)?;
            let items = self.get_current_items();

            if let Some(matched_item) = items.iter().find(|item| item.menu_id == item_id) {
                let proactive_cmd = matched_item.text.clone();

                if !matched_item.items.is_empty() {
                    self.current_path.push(item_id);
                }

                let urcs = vec![StkResponse::UsatProactiveCommand(proactive_cmd)];

                return Ok(StkExecutionResult {
                    response: Some(StkResponse::UsatEnvelopeResponse("0".to_string())),
                    urcs,
                });
            }
            return Err(ExecutionResult::error());
        }

        Err(ExecutionResult::error())
    }

    fn sync_stk_menu_urc(&self) -> StkExecutionResult {
        let mut urcs = Vec::new();
        if self.stk_enabled && !self.stk_config.setup_menu.text.is_empty() {
            urcs.push(StkResponse::UsatProactiveCommand(self.stk_config.setup_menu.text.clone()));
        }
        StkExecutionResult { response: None, urcs }
    }

    fn handle_query_stk_ready(&self) -> StkResult {
        let mut urcs = Vec::new();
        if self.stk_enabled && !self.stk_config.setup_menu.text.is_empty() {
            urcs.push(StkResponse::UsatProactiveCommand(self.stk_config.setup_menu.text.clone()));
        }
        Ok(StkExecutionResult {
            response: Some(StkResponse::StkReady { ready: self.stk_enabled as u8, support: 1 }),
            urcs,
        })
    }

    fn handle_send_stk_terminal_response(&mut self, response: QuotedString) -> StkResult {
        let response_str = response.as_str();
        let mut urcs = Vec::new();

        if let Some(action) = Self::parse_terminal_response(response_str) {
            match action {
                TerminalResponseAction::Select(item_id) => {
                    let proactive_cmd = {
                        let items = self.get_current_items();
                        items
                            .iter()
                            .find(|item| item.menu_id == item_id)
                            .map(|item| item.text.clone())
                    };

                    if let Some(cmd) = proactive_cmd {
                        self.current_path.push(item_id);
                        urcs.push(StkResponse::UsatProactiveCommand(cmd));
                    }
                }
                TerminalResponseAction::Terminate => {
                    self.current_path.clear();
                    urcs.push(StkResponse::UsatSessionEnd);
                }
                TerminalResponseAction::Back => {
                    self.current_path.pop();
                    if self.current_path.is_empty() {
                        urcs.push(StkResponse::UsatSessionEnd);
                    } else if let Some(parent_item) = self.get_item_at_path(&self.current_path) {
                        urcs.push(StkResponse::UsatProactiveCommand(parent_item.text.clone()));
                    }
                }
                TerminalResponseAction::EndSession => {
                    self.current_path.clear();
                    urcs.push(StkResponse::UsatSessionEnd);
                }
                TerminalResponseAction::None => {}
            }
        }

        Ok(StkExecutionResult { response: Some(StkResponse::UsatTerminalResponse(0)), urcs })
    }

    pub(crate) fn execute<'a>(
        &mut self,
        command: &StkCommand<'a>,
        sim: &impl CardState,
    ) -> ExecutionResult {
        if !sim.is_present() {
            return ExecutionResult::error();
        }
        let res = match command {
            StkCommand::QueryStkReady => self.handle_query_stk_ready(),
            StkCommand::SetStkReady(download, _profile) => {
                self.stk_enabled = *download;
                Ok(self.sync_stk_menu_urc())
            }
            StkCommand::SendStkEnvelope(envelope_command) => {
                self.handle_envelope_command(envelope_command.as_str())
            }
            StkCommand::SendStkTerminalResponse(response) => {
                self.handle_send_stk_terminal_response(*response)
            }
        };
        res.map_or_else(|e| e, ExecutionResult::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Response;

    fn create_mock_stk() -> Stk {
        Stk {
            setup_menu: StkMenuItem {
                id: 0,
                menu_id: 0,
                text: "SETUP_MENU_HEX".to_string(),
                items: vec![
                    StkMenuItem {
                        id: 1,
                        menu_id: 0x50,
                        text: "SELECT_ITEM_SIM_HEX".to_string(),
                        items: vec![StkMenuItem {
                            id: 1,
                            menu_id: 0x01,
                            text: "SELECT_ITEM_SUB1_HEX".to_string(),
                            items: vec![StkMenuItem {
                                id: 1,
                                menu_id: 0x01,
                                text: "DISPLAY_TEXT_SUB1_1_HEX".to_string(),
                                items: Vec::new(),
                            }],
                        }],
                    },
                    StkMenuItem {
                        id: 2,
                        menu_id: 0x4E,
                        text: "SELECT_ITEM_USIM_HEX".to_string(),
                        items: Vec::new(),
                    },
                ],
            },
        }
    }

    #[test]
    fn test_stk_result_to_execution_result() {
        let res: StkResult = Ok(StkExecutionResult {
            response: Some(StkResponse::UsatEnvelopeResponse("9000".to_string())),
            urcs: vec![StkResponse::UsatProactiveCommand("9000".to_string())],
        });
        let exec_res: ExecutionResult = res.map_or_else(|e| e, ExecutionResult::from);
        if let ExecutionResult::Success(handled) = exec_res {
            assert_eq!(
                handled.responses,
                vec![
                    Response::Stk(StkResponse::UsatEnvelopeResponse("9000".to_string())),
                    Response::Ok,
                    Response::Stk(StkResponse::UsatProactiveCommand("9000".to_string())),
                ]
            );
        } else {
            panic!("Expected Success");
        }
    }

    #[test]
    fn test_menu_selection_parsing() {
        // Real envelope hex string: D30782028281100150
        // Decoded bytes: D3 07 82 02 82 81 10 01 50
        let command = concat!(
            "D3", "07", // Envelope: Tag D3 (Menu Selection), Length 07
            "82", "02", "8281", // Device IDs: Tag 82, Len 02, ME (82) to UICC (81)
            "10", "01", "50" // Item Identifier: Tag 10, Len 01, Item ID 50
        );
        let item_id = StkService::parse_menu_selection(command);
        assert_eq!(item_id, Some(0x50));
    }

    #[test]
    fn test_terminal_response_selection_parsing() {
        // TR payload: Command Details (Type 24, Cmd Num 01), Device IDs (ME to UICC),
        // Result (Success, Item ID 01) Hex: 81030124008202828183020001
        let command = concat!(
            "81", "03",
            "012400", // Command Details: Tag 81, Len 03, Num 01, Type 24 (SELECT ITEM), Qual 00
            "82", "02", "8281", // Device IDs: Tag 82, Len 02, ME (82) to UICC (81)
            "83", "02", "0001" // Result: Tag 83, Len 02, Success (00), Item ID (01)
        );
        let action = StkService::parse_terminal_response(command);
        assert_eq!(action, Some(TerminalResponseAction::Select(0x01)));

        // Different Item ID: 0x50
        let command2 = concat!(
            "81", "03", "012400", // Command Details: SELECT ITEM
            "82", "02", "8281", // Device IDs: ME to UICC
            "83", "02", "0050" // Result: Success (00), Item ID (50)
        );
        let action2 = StkService::parse_terminal_response(command2);
        assert_eq!(action2, Some(TerminalResponseAction::Select(0x50)));

        // Session terminated: Result 0x10
        let command_term = concat!(
            "81", "03", "012400", // Command Details: SELECT ITEM
            "82", "02", "8281", // Device IDs: ME to UICC
            "83", "01", "10" // Result: Session Terminated by User (0x10)
        );
        let action_term = StkService::parse_terminal_response(command_term);
        assert_eq!(action_term, Some(TerminalResponseAction::Terminate));

        // Backward move: Result 0x11
        let command_back = concat!(
            "81", "03", "012400", // Command Details: SELECT ITEM
            "82", "02", "8281", // Device IDs: ME to UICC
            "83", "01", "11" // Result: Backward Move (0x11)
        );
        let action_back = StkService::parse_terminal_response(command_back);
        assert_eq!(action_back, Some(TerminalResponseAction::Back));

        // Not SELECT ITEM command (e.g. DISPLAY TEXT Type 21) - returns EndSession on
        // success
        let command_other = concat!(
            "81", "03",
            "012100", // Command Details: Tag 81, Len 03, Num 01, Type 21 (DISPLAY TEXT), Qual 00
            "82", "02", "8281", // Device IDs: ME to UICC
            "83", "01", "00" // Result: Success (00)
        );
        let action_other = StkService::parse_terminal_response(command_other);
        assert_eq!(action_other, Some(TerminalResponseAction::EndSession));
    }

    #[test]
    fn test_stk_service_navigation_state_machine() {
        let stk_config = create_mock_stk();
        let mut service = StkService::new(stk_config);

        // 1. Initial state
        assert_eq!(service.current_path.len(), 0);
        assert_eq!(service.get_current_items().len(), 2);
        assert_eq!(service.get_current_items()[0].menu_id, 0x50);

        // 2. Select SIM (0x50) via Envelope
        // Envelope payload with Item ID 0x50: D30782028281100150
        let res = service
            .handle_envelope_command(concat!(
                "D3", "07", // Envelope: Tag D3 (Menu Selection), Length 07
                "82", "02", "8281", // Device IDs: ME to UICC
                "10", "01", "50" // Item ID 0x50
            ))
            .unwrap();
        assert_eq!(res.response, Some(StkResponse::UsatEnvelopeResponse("0".to_string())));
        assert_eq!(
            res.urcs,
            vec![StkResponse::UsatProactiveCommand("SELECT_ITEM_SIM_HEX".to_string())]
        );
        assert_eq!(service.current_path, vec![0x50]);
        assert_eq!(service.get_current_items().len(), 1);
        assert_eq!(service.get_current_items()[0].menu_id, 0x01); // SubMenu1

        // 3. Select SubMenu1 (0x01) via TR
        // TR payload: Select Item, Success, Item ID 01 -> 81030124008202828183020001
        let res = service
            .handle_send_stk_terminal_response(QuotedString(concat!(
                "81", "03", "012400", // Command Details: SELECT ITEM (0x24)
                "82", "02", "8281", // Device IDs: ME (82) to UICC (81)
                "83", "02", "0001" // Result: Success (00), Item ID 01
            )))
            .unwrap();
        assert_eq!(res.response, Some(StkResponse::UsatTerminalResponse(0)));
        assert_eq!(
            res.urcs,
            vec![StkResponse::UsatProactiveCommand("SELECT_ITEM_SUB1_HEX".to_string())]
        );
        assert_eq!(service.current_path, vec![0x50, 0x01]);
        assert_eq!(service.get_current_items().len(), 1);
        assert_eq!(service.get_current_items()[0].menu_id, 0x01); // DisplayText1

        // 4. Backward move via TR
        // TR payload: Result Backward Move 0x11 -> 810301240082028281830111
        let res = service
            .handle_send_stk_terminal_response(QuotedString(concat!(
                "81", "03", "012400", // Command Details: SELECT ITEM
                "82", "02", "8281", // Device IDs: ME to UICC
                "83", "01", "11" // Result: Backward Move (0x11)
            )))
            .unwrap();
        assert_eq!(res.response, Some(StkResponse::UsatTerminalResponse(0)));
        assert_eq!(
            res.urcs,
            vec![StkResponse::UsatProactiveCommand("SELECT_ITEM_SIM_HEX".to_string())]
        );
        assert_eq!(service.current_path, vec![0x50]); // Popped one level
        assert_eq!(service.get_current_items().len(), 1);
        assert_eq!(service.get_current_items()[0].menu_id, 0x01); // Back to SubMenu1

        // 5. Select SubMenu1 (0x01) again
        let _ = service
            .handle_send_stk_terminal_response(QuotedString(concat!(
                "81", "03", "012400", // Command Details: SELECT ITEM (0x24)
                "82", "02", "8281", // Device IDs: ME (82) to UICC (81)
                "83", "02", "0001" // Result: Success (00), Item ID 01
            )))
            .unwrap();
        assert_eq!(service.current_path, vec![0x50, 0x01]);

        // 6. Select DisplayText1 (0x01) from sub-submenu (leaf command text is sent)
        // TR payload: Select Item, Success, Item ID 01 -> 81030124008202828183020001
        let res = service
            .handle_send_stk_terminal_response(QuotedString(concat!(
                "81", "03", "012400", // Command Details: SELECT ITEM (0x24)
                "82", "02", "8281", // Device IDs: ME (82) to UICC (81)
                "83", "02", "0001" // Result: Success (00), Item ID 01
            )))
            .unwrap();
        assert_eq!(res.response, Some(StkResponse::UsatTerminalResponse(0)));
        assert_eq!(
            res.urcs,
            vec![StkResponse::UsatProactiveCommand("DISPLAY_TEXT_SUB1_1_HEX".to_string())]
        );
        assert_eq!(service.current_path, vec![0x50, 0x01, 0x01]); // Stays pointing to leaf

        // 6b. Send TR for DISPLAY TEXT (Type 21, Success)
        // TR payload: Command Details (Type 21), Result (Success) ->
        // 810301210082028281830100
        let res = service
            .handle_send_stk_terminal_response(QuotedString(concat!(
                "81", "03", "012100", // Command Details: DISPLAY TEXT (0x21)
                "82", "02", "8281", // Device IDs: ME to UICC
                "83", "01", "00" // Result: Success (00)
            )))
            .unwrap();
        assert_eq!(res.response, Some(StkResponse::UsatTerminalResponse(0)));
        assert_eq!(res.urcs, vec![StkResponse::UsatSessionEnd]); // Expect session end URC
        assert_eq!(service.current_path, Vec::<u8>::new()); // Now cleared

        // 7. Select USIM (0x4E) from main menu (leaf, no sub-items)
        // Envelope payload: D3078202828110014E
        let res = service
            .handle_envelope_command(concat!(
                "D3", "07", // Envelope: Tag D3 (Menu Selection), Length 07
                "82", "02", "8281", // Device IDs: ME to UICC
                "10", "01", "4E" // Item ID 0x4E (USIM)
            ))
            .unwrap();
        assert_eq!(
            res.urcs,
            vec![StkResponse::UsatProactiveCommand("SELECT_ITEM_USIM_HEX".to_string())]
        );
        assert_eq!(service.current_path, Vec::<u8>::new()); // Leaf, stays at main menu
    }

    #[test]
    fn test_stk_service_session_termination() {
        let stk_config = create_mock_stk();
        let mut service = StkService::new(stk_config);

        // Select SIM (0x50) to go to submenu
        let _ = service
            .handle_envelope_command(concat!(
                "D3", "07", // Envelope: Tag D3 (Menu Selection), Length 07
                "82", "02", "8281", // Device IDs: ME to UICC
                "10", "01", "50" // Item ID 0x50
            ))
            .unwrap();
        assert_eq!(service.current_path, vec![0x50]);

        // Send Terminal Response with Session Terminated by User (0x10)
        // TR payload: Result 0x10 -> 810301240082028281830110
        let res = service
            .handle_send_stk_terminal_response(QuotedString(concat!(
                "81", "03", "012400", // Command Details: SELECT ITEM
                "82", "02", "8281", // Device IDs: ME to UICC
                "83", "01", "10" // Result: Session Terminated by User (0x10)
            )))
            .unwrap();
        assert_eq!(res.response, Some(StkResponse::UsatTerminalResponse(0)));
        assert_eq!(res.urcs, vec![StkResponse::UsatSessionEnd]);
        assert_eq!(service.current_path.len(), 0); // Reset to main menu
    }

    #[test]
    fn test_proactive_command_type_conversions() {
        assert_eq!(ProactiveCommandType::try_from(0x21), Ok(ProactiveCommandType::DisplayText));
        assert_eq!(ProactiveCommandType::try_from(0x23), Ok(ProactiveCommandType::GetInput));
        assert_eq!(ProactiveCommandType::try_from(0x24), Ok(ProactiveCommandType::SelectItem));
        assert_eq!(ProactiveCommandType::try_from(0x25), Ok(ProactiveCommandType::SetUpMenu));
        assert_eq!(ProactiveCommandType::try_from(0x13), Err(()));
        assert_eq!(ProactiveCommandType::try_from(0xFF), Err(()));

        assert_eq!(u8::from(ProactiveCommandType::SelectItem), 0x24);
        assert_eq!(u8::from(ProactiveCommandType::DisplayText), 0x21);
    }

    #[test]
    fn test_stk_service_sim_not_present_early_return() {
        use crate::tests::test_utils::MockCardState;

        let mut service = StkService::default();
        let sim_absent = MockCardState { is_present: false, ..Default::default() };
        let res = service.execute(&StkCommand::QueryStkReady, &sim_absent);
        assert_eq!(res, ExecutionResult::error());

        let sim_present = MockCardState::default();
        let res = service.execute(&StkCommand::QueryStkReady, &sim_present);
        assert!(matches!(res, ExecutionResult::Success(_)));
    }

    #[test]
    fn test_stk_service_next_tlv() {
        // Empty slice returns None
        let mut empty = &[][..];
        assert_eq!(StkService::next_tlv(&mut empty), None);

        // Valid single TLV
        let bytes = [0x10, 0x01, 0x50];
        let mut slice = &bytes[..];
        assert_eq!(StkService::next_tlv(&mut slice), Some((0x10, &[0x50][..])));
        assert!(slice.is_empty());

        // Multiple TLVs in sequence
        let multi = [0x82, 0x02, 0x82, 0x81, 0x10, 0x01, 0x50];
        let mut multi_slice = &multi[..];
        assert_eq!(StkService::next_tlv(&mut multi_slice), Some((0x82, &[0x82, 0x81][..])));
        assert_eq!(StkService::next_tlv(&mut multi_slice), Some((0x10, &[0x50][..])));
        assert_eq!(StkService::next_tlv(&mut multi_slice), None);
        assert!(multi_slice.is_empty());

        // Truncated TLV (length declares 3 bytes, only 1 byte present)
        let truncated = [0x82, 0x03, 0x82];
        let mut trunc_slice = &truncated[..];
        assert_eq!(StkService::next_tlv(&mut trunc_slice), None);
        assert!(trunc_slice.is_empty());
    }
}
