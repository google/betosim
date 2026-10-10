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

use futures_util::StreamExt;
use netsim_proto::{
    nfc_service::{GetStatusRequest, PollRequest, SendApduRequest, SetPowerRequest},
    nfc_service_grpc::NfcServiceClient,
};

use super::{args::NfcCommand, display};
use crate::error::Result;

pub(crate) fn execute(cmd: &NfcCommand, client: &NfcServiceClient, _verbose: bool) -> Result<()> {
    match cmd {
        NfcCommand::List { chip_id, json } => {
            let mut req = GetStatusRequest::new();
            if let Some(id) = chip_id {
                req.chip_id = *id;
            }
            let resp = client.get_status(&req)?;
            display::print_status(&resp, *json)?;
        }
        NfcCommand::Poll { chip_id } => {
            let mut req = PollRequest::new();
            req.chip_id = *chip_id;
            let mut receiver = client.poll(&req)?;
            futures::executor::block_on(async {
                while let Some(item) = receiver.next().await {
                    match item {
                        Ok(resp) => display::print_poll(&resp),
                        Err(e) => eprintln!("Poll stream error: {e}"),
                    }
                }
            });
        }
        NfcCommand::Apdu { chip_id, payload } => {
            let bytes = hex::decode(payload).map_err(|e| format!("Invalid hex APDU: {e}"))?;
            let mut req = SendApduRequest::new();
            req.chip_id = *chip_id;
            req.apdu = bytes;
            let resp = client.send_apdu(&req)?;
            display::print_apdu(&resp);
        }
        NfcCommand::Radio { chip_id, state } => {
            let power_on = match state.to_lowercase().as_str() {
                "on" | "true" | "1" => true,
                "off" | "false" | "0" => false,
                _ => return Err(format!("Invalid state: {state}. Use on or off.").into()),
            };
            let mut req = SetPowerRequest::new();
            req.chip_id = *chip_id;
            req.power_on = power_on;
            let resp = client.set_power(&req)?;
            display::print_set_power(&resp);
        }
    }
    Ok(())
}
