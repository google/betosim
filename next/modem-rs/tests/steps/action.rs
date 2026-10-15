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

use std::time::Duration;

use modem_rs::{PhoneNumber, RegistrationStatus};
use netsim_model::{ChipId, ModemAction, RadioTechnology};

use crate::world::World;

/// Sends an AT command string to the specified modem.
///
/// This function automatically appends `\r\n` if the command string does not
/// already end with it. The command is sent as bytes.
///
/// # Panics
///
/// Panics if the modem name is not found.
pub fn when_at_command_sent(world: &mut World, name: &str, command: &str) {
    let (id, _) = world.get_modem(name);
    let full_command =
        if command.ends_with("\r\n") { command.to_string() } else { format!("{command}\r\n") };

    world.manager.send_at_command(id, full_command.as_bytes());
}

/// Sends raw bytes, specified as a hex string, to the modem.
///
/// This is useful for sending non-ASCII sequences like PDU bytes or Ctrl-Z.
///
/// # Panics
///
/// Panics if the hex string is invalid or the modem is not found.
pub fn when_hex_bytes_sent(world: &mut World, name: &str, hex_bytes: &str) {
    let (id, _) = world.get_modem(name);
    let bytes = hex::decode(hex_bytes).expect("Invalid hex string provided to when_hex_bytes_sent");

    world.manager.send_at_command(id, &bytes);
}

/// Advances the simulated time by `ms` milliseconds and ticks the manager.
///
/// This ensures any time-dependent events (like timeouts) are processed.
pub fn when_time_advances_ms(world: &mut World, ms: u64) {
    world.clock.advance(Duration::from_millis(ms));
    // Ticking allows the manager to process expired timers immediately.
    world.manager.tick();
}

/// Triggers a physical channel configuration update on the modem.
pub fn when_physical_channel_configs_updated(world: &mut World, name: &str) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::UpdatePhysicalChannelConfigs { id: ChipId(id) };
    world.manager.dispatch(action);
}

pub fn when_incoming_sms_received(world: &mut World, name: &str, sender: &str, text: &str) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::IncomingSms {
        id: ChipId(id),
        sender: sender.to_string(),
        text: text.to_string(),
    };
    world.manager.dispatch(action);
}

pub fn when_incoming_pdu_received(world: &mut World, name: &str, pdu: &str) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::IncomingPdu { id: ChipId(id), pdu: pdu.to_string() };
    world.manager.dispatch(action);
}

pub fn when_incoming_call_received(world: &mut World, name: &str, number: &str) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::IncomingCall { target_id: ChipId(id), number: number.to_string() };
    world.manager.dispatch(action);
}

pub fn when_external_call_answered(world: &mut World, name: &str) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::RemoteAnswer { id: ChipId(id) };
    world.manager.dispatch(action);
}

pub fn when_external_call_hungup(world: &mut World, name: &str) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::RemoteHangup { id: ChipId(id) };
    world.manager.dispatch(action);
}

pub fn when_external_call_held(world: &mut World, name: &str, on_hold: bool) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::RemoteHold { id: ChipId(id), on_hold };
    world.manager.dispatch(action);
}

pub fn when_network_time_updated(world: &mut World, name: &str) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::UpdateNetworkTime { id: ChipId(id) };
    world.manager.dispatch(action);
}

pub fn when_voice_registration_set(world: &mut World, name: &str, status: RegistrationStatus) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::SetVoiceRegistration { id: ChipId(id), status };
    world.manager.dispatch(action);
}

pub fn when_data_registration_set(world: &mut World, name: &str, status: RegistrationStatus) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::SetDataRegistration { id: ChipId(id), status };
    world.manager.dispatch(action);
}

pub fn when_signal_strength_set(world: &mut World, name: &str, rssi: u8, ber: u8) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::SetSignalStrength { id: ChipId(id), rssi, ber };
    world.manager.dispatch(action);
}

pub fn when_network_technology_changes(world: &mut World, name: &str, tech: RadioTechnology) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::SetNetworkTechnology { id: ChipId(id), tech };
    world.manager.dispatch(action);
}

pub fn when_sim_status_set(world: &mut World, name: &str, present: bool) {
    let (id, _) = world.get_modem(name);
    let action = ModemAction::SetSimStatus { id: ChipId(id), present };
    world.manager.dispatch(action);
}

/// Updates the phone number on the specified modem (mirroring the Netsim gRPC
/// path).
pub fn when_phone_number_set(world: &mut World, name: &str, number: &str) {
    let (id, _) = world.get_modem(name);
    if let Some(modem) = world.manager.get_modem_mut(id) {
        let phone = PhoneNumber::new_for_test(number);
        modem.set_phone_number(phone);
    } else {
        panic!("Failed to retrieve modem '{name}'");
    }
}
