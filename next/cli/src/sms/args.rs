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

use clap::{Args, Subcommand};

#[derive(Debug, Subcommand, PartialEq)]
pub(crate) enum SmsCommand {
    /// Send an SMS message (sms send <sender> <text>)
    Send(SmsSend),
    /// Send a PDU SMS message (sms pdu <hex_pdu>)
    Pdu(SmsPdu),
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct SmsSend {
    /// Sender's phone number (digits, optional leading '+', max 15 digits)
    pub sender: String,
    /// SMS text content (options like --id must precede the text)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub text: Vec<String>,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}

#[derive(Debug, Args, PartialEq)]
pub(crate) struct SmsPdu {
    /// Hex-encoded PDU data
    pub pdu: String,
    /// ID of the cellular device
    #[arg(long)]
    pub id: Option<u32>,
}
