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

use netsim_proto::nfc_service::{
    GetStatusResponse, PollResponse, SendApduResponse, SetPowerResponse,
};

use crate::error::Result;

fn get_power_status(chip: &netsim_proto::model::Chip) -> &'static str {
    if let Some(netsim_proto::model::chip::Chip::Nfc(nfc)) = chip.chip.as_ref() {
        if nfc.state.unwrap_or(true) { "ON" } else { "OFF" }
    } else {
        "UNKNOWN"
    }
}

pub(crate) fn print_status(resp: &GetStatusResponse, json: bool) -> Result<()> {
    if json {
        let mut chips_json = Vec::new();
        for chip in &resp.chips {
            chips_json.push(serde_json::json!({
                "chip_id": chip.id,
                "name": chip.name,
                "power": get_power_status(chip)
            }));
        }
        println!("{}", serde_json::to_string_pretty(&chips_json)?);
    } else {
        println!("{:<10} {:<20} {:<10}", "CHIP_ID", "NAME", "POWER");
        println!("{}", "-".repeat(42));
        for chip in &resp.chips {
            println!("{:<10} {:<20} {:<10}", chip.id, chip.name, get_power_status(chip));
        }
    }
    Ok(())
}

pub(crate) fn print_poll(resp: &PollResponse) {
    println!("Discovered target ID: {}", resp.target_id);
    println!("Tag Data (HEX): {}", hex::encode(&resp.tag_data));
}

pub(crate) fn print_apdu(resp: &SendApduResponse) {
    println!("APDU Response (HEX): {}", hex::encode(&resp.response));
}

pub(crate) fn print_set_power(resp: &SetPowerResponse) {
    if let Some(ref chip) = resp.chip.0 {
        println!(
            "Updated NFC chip {} ({}) power to {}",
            chip.id,
            chip.name,
            get_power_status(chip)
        );
    }
}
