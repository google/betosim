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

use netsim_rest_api::NetsimRestClient;

use super::{
    args::ApCommand,
    display::{print_ap, print_list_ap_response},
};
use crate::error::Result;

fn parse_channel(ch: u32) -> Result<u8> {
    u8::try_from(ch)
        .map_err(|_err| crate::error::Error::from(format!("Invalid channel {ch}: must be 0..=255")))
}

pub(crate) fn execute(
    cmd: &ApCommand,
    rest_client: &NetsimRestClient,
    verbose: bool,
) -> Result<()> {
    match cmd {
        ApCommand::List(args) => {
            crate::repeat(args.continuous, || {
                print_list_ap_response(&rest_client.get_aps().send_blocking()?, verbose);
                Ok(())
            })?;
        }
        ApCommand::Create(args) => {
            let mut req = netsim_model::ApCreate::default();
            if let Some(ssid) = &args.ssid {
                req.ssid = ssid.clone();
            }
            if let Some(channel) = args.channel {
                req.channel = parse_channel(channel)?;
            }
            if let Some(bssid) = &args.bssid {
                req.bssid = bssid.clone();
            }

            if let Some(password) = &args.password {
                req.wpa_passphrase = Some(password.clone());
            }
            if let Some(protocol) = &args.protocol {
                req.hw_mode = protocol
                    .parse::<netsim_model::WifiMode>()
                    .map_err(crate::error::Error::from)?;
            }

            let res = rest_client.create_ap(&req).send_blocking()?;
            if verbose {
                println!(
                    "Successfully created Access Point '{}' with ID {}",
                    res.config.ssid, res.id
                );
                print_ap(&res);
            }
        }
        ApCommand::Patch(args) => {
            let req = netsim_model::ApUpdate {
                ssid: args.ssid.clone(),
                channel: args.channel.map(parse_channel).transpose()?,
                ..Default::default()
            };

            let res = rest_client.patch_ap(args.id, &req).send_blocking()?;
            if verbose {
                println!(
                    "Successfully updated Access Point '{}' with ID {}",
                    res.config.ssid, res.id
                );
                print_ap(&res);
            }
        }
        ApCommand::Remove(args) => {
            rest_client.delete_ap(args.id).send_blocking()?;
            if verbose {
                println!("Successfully removed Access Point");
            }
        }
        ApCommand::Disconnect(args) => {
            let req =
                netsim_rest_api::DisconnectApRequest { mac_address: args.mac_address.clone() };
            rest_client.disconnect_ap(args.id, &req).send_blocking()?;
            if verbose {
                println!("Successfully disconnected client from Access Point");
            }
        }
    }
    Ok(())
}
