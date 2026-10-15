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

use netsim_model::Ap;
use netsim_proto::access_point;
use netsim_rest_api::ListApResponse;

pub(crate) fn print_list_ap_response(response: &ListApResponse, _verbose: bool) {
    if response.aps.is_empty() {
        println!("No available Access Points found.");
        return;
    }

    println!(
        "{:<2} | {:<20} | {:<17} | {:<20} | {:<4} | {:<2} | {:<9} | {:<6} | {:<7}",
        "ID", "SSID", "BSSID", "Security", "Chan", "CC", "PHY Mode", "Hidden", "Clients"
    );
    println!("{:-<108}", "-");

    let mut aps: Vec<&Ap> = response.aps.iter().collect();
    aps.sort_by_key(|ap| ap.id);
    for ap in aps {
        print_rest_ap(ap);
    }
}

fn print_rest_ap(ap: &Ap) {
    let cfg = &ap.config;
    let security = if cfg.enterprise_enabled {
        "WPA2/WPA3 Enterprise"
    } else if cfg.sae {
        "WPA3 Personal"
    } else if cfg.wpa_passphrase.as_deref().is_some_and(|p| !p.is_empty()) {
        "WPA2 Personal"
    } else {
        "None"
    };

    let phy_mode = format!("802.11{}", cfg.hw_mode);
    let hidden = if cfg.hidden_ssid { "Yes" } else { "No" };
    let clients = ap.associations.len();
    let country_code = cfg.country_code.as_deref().unwrap_or("");

    print!(
        "{:<2} | {:<20} | {:<17} | {:<20} | {:<4} | {:<2} | {:<9} | {:<6} | {:<7}",
        ap.id, cfg.ssid, cfg.bssid, security, cfg.channel, country_code, phy_mode, hidden, clients
    );

    if !cfg.mac_acl_list.is_empty() {
        print!(" | Client MAC ACL List: {:?}", cfg.mac_acl_list);
    }
    println!();
}

pub(crate) fn print_ap(ap: &access_point::AccessPoint) {
    let security = if ap.enterprise_enabled {
        "WPA2/WPA3 Enterprise"
    } else if ap.sae {
        "WPA3 Personal"
    } else if !ap.wpa_passphrase.is_empty() {
        "WPA2 Personal"
    } else {
        "None"
    };

    let phy_mode =
        if ap.hw_mode.is_empty() { "802.11".to_string() } else { format!("802.11{}", ap.hw_mode) };

    let hidden = if ap.hidden_ssid { "Yes" } else { "No" };
    let clients = ap.connected_devices.len();

    print!(
        "{:<2} | {:<20} | {:<17} | {:<20} | {:<4} | {:<2} | {:<9} | {:<6} | {:<7}",
        ap.id, ap.ssid, ap.bssid, security, ap.channel, ap.country_code, phy_mode, hidden, clients
    );

    if !ap.mac_acl_list.is_empty() {
        print!(" | Client MAC ACL List: {:?}", ap.mac_acl_list);
    }
    println!();
}
