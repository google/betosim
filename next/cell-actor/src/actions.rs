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

use netsim_model::{ChipError, RadioTechnology, RegistrationStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CellAction {
    IncomingCall { number: String },
    UpdateCall,
    EndCall,
    ReceiveSms { sender: String, text: String },
    ReceivePdu { pdu: String },
    InjectCellBroadcast { serial_number: u16, message_id: u16, dcs: u8, text: String },
    SetSignalStrength { rssi: u32, ber: u32 },
    SetVoiceRegistration { status: RegistrationStatus },
    SetDataRegistration { status: RegistrationStatus },
    RemoteAnswer,
    RemoteHold { on_hold: bool },
    SetSimStatus { present: bool },
    SetNetworkTechnology { tech: RadioTechnology },
    SetOperator { operator: String },
}

impl CellAction {
    /// Validates whether a phone number contains only allowed dialing characters.
    fn is_valid_phone_number(s: &str) -> bool {
        !s.is_empty()
            && s.chars().all(|c| {
                c.is_ascii_digit()
                    || c == '+'
                    || c == '-'
                    || c == ' '
                    || c == '('
                    || c == ')'
                    || c == '*'
                    || c == '#'
            })
    }

    /// Validates whether an SMS sender identifier contains only allowed characters.
    fn is_valid_sms_sender(s: &str) -> bool {
        !s.is_empty()
            && s.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || c == '+'
                    || c == '-'
                    || c == ' '
                    || c == '('
                    || c == ')'
            })
    }

    /// Validates whether a PDU string is a non-empty even-length hex string.
    fn is_valid_hex_pdu(s: &str) -> bool {
        !s.is_empty() && s.len().is_multiple_of(2) && s.chars().all(|c| c.is_ascii_hexdigit())
    }

    /// Validates the action parameters, returning an error if validation fails.
    pub(crate) fn validate(&self) -> Result<(), ChipError> {
        match self {
            Self::IncomingCall { number } => {
                if !Self::is_valid_phone_number(number) {
                    return Err(ChipError::InvalidArguments(
                        format!("Invalid phone number: {number}").into(),
                    ));
                }
            }
            Self::ReceiveSms { sender, .. } => {
                if !Self::is_valid_sms_sender(sender) {
                    return Err(ChipError::InvalidArguments(
                        format!("Invalid SMS sender: {sender}").into(),
                    ));
                }
            }
            Self::ReceivePdu { pdu } => {
                if !Self::is_valid_hex_pdu(pdu) {
                    return Err(ChipError::InvalidArguments(
                        format!("Invalid PDU format (must be even-length hex string): {pdu}")
                            .into(),
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CellActionResult {
    Success,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_phone_numbers() {
        assert!(CellAction::is_valid_phone_number("+1234567890"));
        assert!(CellAction::is_valid_phone_number("123-456-7890"));
        assert!(CellAction::is_valid_phone_number("(555) 123 4567"));
        assert!(CellAction::is_valid_phone_number("*#06#"));
        assert!(!CellAction::is_valid_phone_number(""));
        assert!(!CellAction::is_valid_phone_number("123abc456"));
    }

    #[test]
    fn test_valid_sms_sender() {
        assert!(CellAction::is_valid_sms_sender("+15551234567"));
        assert!(CellAction::is_valid_sms_sender("GoogleAlert"));
        assert!(CellAction::is_valid_sms_sender("123-456"));
        assert!(!CellAction::is_valid_sms_sender(""));
        assert!(!CellAction::is_valid_sms_sender("Sender@domain"));
    }

    #[test]
    fn test_valid_hex_pdu() {
        assert!(CellAction::is_valid_hex_pdu("07914400000000F0"));
        assert!(CellAction::is_valid_hex_pdu("ABCD"));
        assert!(!CellAction::is_valid_hex_pdu(""));
        assert!(!CellAction::is_valid_hex_pdu("ABC"));
        assert!(!CellAction::is_valid_hex_pdu("07914400ZZ"));
    }

    #[test]
    fn test_validate_action() {
        let valid_call = CellAction::IncomingCall { number: "+15551234567".into() };
        assert!(valid_call.validate().is_ok());

        let invalid_call = CellAction::IncomingCall { number: "bad#phone$".into() };
        assert!(invalid_call.validate().is_err());

        let valid_sms = CellAction::ReceiveSms { sender: "12345".into(), text: "Hello".into() };
        assert!(valid_sms.validate().is_ok());

        let invalid_sms = CellAction::ReceiveSms { sender: "".into(), text: "Hello".into() };
        assert!(invalid_sms.validate().is_err());

        let valid_pdu = CellAction::ReceivePdu { pdu: "ABCD".into() };
        assert!(valid_pdu.validate().is_ok());

        let invalid_pdu = CellAction::ReceivePdu { pdu: "ABC".into() };
        assert!(invalid_pdu.validate().is_err());
    }
}
