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

use std::{sync::Arc, time::Duration};

use netsim_model::{CellNetworkConfig, Quirks, RadioTechnology, RegistrationStatus};
use tracing::{debug, error};

use crate::{
    CardState, RadioAdmission,
    call_service::{CallCommand, CallResponse, CallService, RingToken},
    config::{ProfileMetadata, SimProfile},
    constants::CALL_RING_TIMEOUT,
    data_service::DataService,
    misc_service::MiscService,
    network_service::{NetworkCommand, NetworkService, RegistrationType},
    parser::Command,
    sim_service::SimService,
    sms_service::SmsService,
    stk_service::StkService,
    sup_service::SupService,
    time::Clock,
    types::{
        AT_OK, CallForwardCondition, CmeError, CommandAction, CopsMode, ExecutionResult,
        HangupReason, ModemError, ModemId, NumberPresentation, PhoneNumber, RadioPowerLevel,
        RegistrationUnsolicitedMode, Response, SignalQuality,
    },
};

/// Represents a single modem device.
pub struct ModemImpl {
    pub id: ModemId,
    pub enable_unsolicited_urcs: bool,
    sim_service: SimService,
    network_service: NetworkService,
    sms_service: SmsService,
    call_service: CallService,
    stk_service: StkService,
    sup_service: SupService,
    misc_service: MiscService,
    data_service: DataService,
    pub quirks: Quirks,
    _state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModemEvent {
    CallRingTimeout { ring_token: RingToken },
    AttachNetwork,
    TestEvent,
}

#[derive(Debug)]
pub enum ModemEffect {
    Action(CommandAction),
    Schedule { delay: Duration, event: ModemEvent },
    Response(Vec<u8>),
}

impl ModemImpl {
    pub(crate) fn new(
        id: ModemId,
        profile: SimProfile,
        quirks: Quirks,
        clock: Arc<dyn Clock>,
        network_configs: Vec<CellNetworkConfig>,
    ) -> Self {
        let enable_unsol = profile.enable_unsolicited_urcs.unwrap_or(true);
        let home_plmn = profile.home_plmn();
        let sim_service = SimService::from_profile(&profile);
        let mut misc_service = MiscService::new(clock.clone());
        if quirks.auto_ctzv {
            misc_service.set_ctzv_mode(true);
        }
        Self {
            id,
            enable_unsolicited_urcs: enable_unsol,
            sim_service,
            network_service: NetworkService::new(quirks, home_plmn),
            sms_service: SmsService::new(quirks, clock),
            stk_service: StkService::new(profile.stk.clone()),
            sup_service: SupService::new(quirks),
            misc_service,
            call_service: CallService::default(),
            data_service: DataService::new(quirks, network_configs),
            quirks,
            _state: State::Idle,
        }
    }

    pub fn trigger_incoming_call(
        &mut self,
        number: Option<&PhoneNumber>,
        number_presentation: NumberPresentation,
        peer_id: Option<ModemId>,
    ) -> Option<Vec<ModemEffect>> {
        if !self.network_service.can_terminate_voice_call() {
            debug!("[Modem {}] radio off / unregistered, suppressing MT call", self.id);
            return None;
        }
        let ring_token =
            self.call_service.ring(number.cloned(), number_presentation, peer_id).ok()?;
        let mut effects = Vec::new();
        effects.push(ModemEffect::Response(CallResponse::Ring.to_string().into_bytes()));

        if self.sup_service.clip_enabled() {
            let val = number_presentation.format_number(number);
            let mode = if number_presentation == NumberPresentation::Allowed && number.is_none() {
                NumberPresentation::NotAvailable as u8
            } else {
                number_presentation as u8
            };
            let number_str = val.number;
            let clip = format!("+CLIP: \"{number_str}\",{},,,,{mode}\r\n", val.toa);
            effects.push(ModemEffect::Response(clip.into_bytes()));
        }

        let delay = self.sup_service.no_reply_timeout().unwrap_or(CALL_RING_TIMEOUT);
        effects.push(ModemEffect::Schedule {
            delay,
            event: ModemEvent::CallRingTimeout { ring_token },
        });

        Some(effects)
    }

    pub(crate) fn fail_outbound_call(&mut self) -> bool {
        self.call_service.fail_outbound_call()
    }

    pub fn trigger_remote_answer(&mut self) -> Vec<ModemEffect> {
        let mut effects = Vec::new();
        if self.call_service.remote_answer() {
            effects.push(ModemEffect::Response(AT_OK.to_vec()));
        }
        effects
    }

    pub fn trigger_remote_hold(&mut self, on_hold: bool) -> Vec<ModemEffect> {
        self.call_service.trigger_remote_hold(on_hold);
        Vec::new()
    }

    pub fn trigger_remote_hangup(&mut self) -> Vec<ModemEffect> {
        let mut effects = Vec::new();
        self.call_service.receive_hangup();
        let urc = HangupReason::Normal.as_urc_str(self.quirks.goldfish_ril_37_or_earlier);
        effects.push(ModemEffect::Response(urc.as_bytes().to_vec()));
        effects
    }

    pub fn trigger_incoming_sms(&mut self, sender: &str, text: &str) -> Vec<ModemEffect> {
        if !self.network_service.can_terminate_sms() {
            return Vec::new();
        }
        debug!(
            "trigger_incoming_sms: id = {}, message_format = {:?}",
            self.id, self.sms_service.message_format
        );
        if self.sms_service.message_format == crate::sms_service::MessageFormat::Pdu {
            let pdu_hex = crate::pdu::create_deliver_pdu_ucs2(sender, text);
            return self.trigger_incoming_pdu(&pdu_hex);
        }
        let mut effects = Vec::new();
        // Format: +CMT: "<sender>",,"<timestamp>"\r\n<text>
        let timestamp = "22/01/01,12:00:00+00";
        let response = format!("+CMT: \"{sender}\",,\"{timestamp}\"\r\n{text}\r\n");
        effects.push(ModemEffect::Response(response.as_bytes().to_vec()));
        effects
    }

    pub fn trigger_incoming_pdu(&mut self, pdu: &str) -> Vec<ModemEffect> {
        if !self.network_service.can_terminate_sms() {
            return Vec::new();
        }
        // Calculate TPDU length
        let mut effects = Vec::new();
        if let Ok(bytes) = hex::decode(pdu)
            && let Some(&sca_len_byte) = bytes.first()
        {
            let sca_len = sca_len_byte as usize;
            if bytes.len() > 1 + sca_len {
                let tpdu_len = bytes.len() - 1 - sca_len;
                let omit_cmt_leading_comma = self.quirks.goldfish_ril_37_or_earlier;
                let comma = if omit_cmt_leading_comma { "" } else { "," };
                let response = format!("+CMT: {comma}{tpdu_len}\r\n{pdu}\r\n");
                effects.push(ModemEffect::Response(response.as_bytes().to_vec()));
            }
        }
        effects
    }

    pub fn trigger_incoming_cell_broadcast(
        &mut self,
        serial_number: u16,
        message_id: u16,
        dcs: u8,
        text: &str,
    ) -> Vec<ModemEffect> {
        if !self.network_service.can_terminate_sms()
            || !self.sms_service.accepts_broadcast(message_id, dcs)
        {
            return Vec::new();
        }
        let Ok(pages) = crate::pdu::CbsPdu::encode_pages(serial_number, message_id, dcs, text)
        else {
            return Vec::new();
        };
        pages.into_iter().map(|page| ModemEffect::Response(page.to_urc().into_bytes())).collect()
    }

    pub fn trigger_incoming_cell_broadcast_pdu(&mut self, pdu: &str) -> Vec<ModemEffect> {
        if !self.network_service.can_terminate_sms() {
            return Vec::new();
        }
        let Ok(cbs_pdu) = crate::pdu::CbsPdu::parse(pdu) else {
            return Vec::new();
        };
        if !self.sms_service.accepts_broadcast(cbs_pdu.message_id(), cbs_pdu.dcs()) {
            return Vec::new();
        }
        vec![ModemEffect::Response(cbs_pdu.to_urc().into_bytes())]
    }

    pub fn trigger_network_time_update(&mut self) -> Vec<ModemEffect> {
        let mut effects = Vec::new();
        let response = self.misc_service.current_time_update();
        effects.push(ModemEffect::Response(response.to_string().into_bytes()));
        effects
    }

    /// Sets Emergency Callback Mode (ECBM) and emits a `+WSOS` URC.
    ///
    /// TODO(b/562993662): Expose via cell.proto / ModemAction when external
    /// ECBM simulation is needed.
    pub fn trigger_emergency_callback_mode(&mut self, enabled: bool) -> Vec<ModemEffect> {
        let response = self.call_service.set_emergency_mode(enabled);
        vec![ModemEffect::Response(response.to_string().into_bytes())]
    }

    pub fn set_phone_number(&mut self, phone: PhoneNumber) {
        self.sim_service.set_msisdn(Some(&phone));
    }

    pub fn phone_number(&self) -> Option<PhoneNumber> {
        self.sim_service.get_msisdn()
    }

    /// Updates network configs for future data calls.
    ///
    /// Active calls retain their existing addresses until reactivated.
    /// Do not signal updates via `+CGEV`: the Goldfish/Cuttlefish parser lacks
    /// `CGEV` support, causing channel teardown (b/562098771).
    pub fn update_network_configs(&mut self, configs: Vec<CellNetworkConfig>) {
        self.data_service.update_network_configs(configs);
    }

    pub fn network_configs(&self) -> &[CellNetworkConfig] {
        self.data_service.network_configs()
    }

    pub(crate) fn signal_quality(&self) -> SignalQuality {
        self.network_service.signal_quality()
    }

    pub(crate) fn voice_registration(&self) -> RegistrationStatus {
        self.network_service.voice_registration()
    }

    pub(crate) fn data_registration(&self) -> RegistrationStatus {
        self.network_service.data_registration()
    }

    pub(crate) fn get_profile_metadata(&self) -> Option<ProfileMetadata> {
        self.sim_service.get_profile_metadata()
    }

    pub(crate) fn has_outbound_call(&self) -> bool {
        self.call_service.has_outbound()
    }

    pub(crate) fn has_outbound_to_peer(&self, peer_id: ModemId) -> bool {
        self.call_service.has_outbound_to_peer(peer_id)
    }

    pub(crate) fn receive_hangup_from_peer_id(&mut self, peer_id: ModemId) -> bool {
        self.call_service.receive_hangup_from_peer_id(peer_id)
    }

    pub(crate) fn has_calls(&self) -> bool {
        self.call_service.has_calls()
    }

    pub(crate) fn receive_peer_hold(&mut self, peer_id: ModemId, on_hold: bool) {
        self.call_service.receive_peer_hold(peer_id, on_hold);
    }

    pub(crate) fn update_physical_channel_configs(&self) -> Vec<ModemEffect> {
        self.data_service.on_update_physical_channel_configs()
    }

    pub(crate) fn remote_answer(&mut self) -> bool {
        self.call_service.remote_answer()
    }

    pub(crate) fn set_outbound_call_peer_id(&mut self, peer_id: Option<ModemId>) {
        self.call_service.set_outbound_peer_id(peer_id);
    }

    pub(crate) fn incoming_call_forward_target(&self) -> Option<PhoneNumber> {
        if let Some(target) =
            self.sup_service.active_forward_number(CallForwardCondition::Unconditional)
        {
            return Some(target);
        }
        if !self.network_service.can_terminate_voice_call() {
            return self.sup_service.active_forward_number(CallForwardCondition::NotReachable);
        }
        let is_busy = self.call_service.has_inbound()
            || (self.call_service.has_active()
                && !self.sup_service.is_voice_call_waiting_enabled());
        if is_busy {
            return self.sup_service.active_forward_number(CallForwardCondition::Busy);
        }
        None
    }

    pub(crate) fn forward_number_on_release(&self, reason: HangupReason) -> Option<PhoneNumber> {
        let condition = match reason {
            HangupReason::Busy => CallForwardCondition::Busy,
            HangupReason::NoAnswer => CallForwardCondition::NoReply,
            HangupReason::Normal => return None,
        };
        self.sup_service.active_forward_number(condition)
    }

    pub(crate) fn calls(&self) -> &[crate::call_service::CallStatus] {
        self.call_service.calls()
    }

    pub fn set_signal_strength(&mut self, rssi: u8, ber: u8) -> Vec<ModemEffect> {
        self.network_service
            .set_signal_strength(rssi, ber)
            .into_iter()
            .map(|r| ModemEffect::Response(r.into_bytes()))
            .collect()
    }

    pub fn set_registration(
        &mut self,
        reg_type: RegistrationType,
        status: RegistrationStatus,
    ) -> Vec<ModemEffect> {
        if let Some(response) = self.network_service.set_registration(reg_type, status) {
            vec![ModemEffect::Response(response.into_bytes())]
        } else {
            Vec::new()
        }
    }

    pub fn set_voice_registration(&mut self, status: RegistrationStatus) -> Vec<ModemEffect> {
        self.set_registration(RegistrationType::Voice, status)
    }

    pub fn set_data_registration(&mut self, status: RegistrationStatus) -> Vec<ModemEffect> {
        self.set_registration(RegistrationType::Data, status)
    }

    pub(crate) fn set_sim_status(&mut self, present: bool) -> Vec<ModemEffect> {
        if present && !self.sim_service.is_provisioned() {
            return Vec::new();
        }
        let changed = self.sim_service.set_present(present);
        let mut effects = Vec::new();
        if changed {
            if !present {
                effects.extend(
                    self.set_registration(
                        RegistrationType::Voice,
                        RegistrationStatus::NotRegistered,
                    ),
                );
                effects.extend(
                    self.set_registration(
                        RegistrationType::Data,
                        RegistrationStatus::NotRegistered,
                    ),
                );
                self.network_service.detach_network();
                self.network_service.set_home_plmn(None);
                self.stk_service = StkService::default();
                self.call_service.clear_calls();
            } else {
                let home_plmn = self.sim_service.home_plmn();
                self.network_service.set_home_plmn(home_plmn);
                if home_plmn.is_some() {
                    effects.push(ModemEffect::Schedule {
                        delay: Duration::from_millis(10),
                        event: ModemEvent::AttachNetwork,
                    });
                }
            }
            if !self.quirks.goldfish_ril_37_or_earlier
                && let Some(urc) = self.sim_service.get_cpin_urc()
            {
                effects.push(ModemEffect::Response(urc.as_bytes().to_vec()));
            }
        }
        effects
    }

    /// Ejects the SIM tray, cutting electrical power to the card while
    /// preserving non-volatile data on the card.
    pub(crate) fn eject_sim(&mut self) -> Vec<ModemEffect> {
        self.set_sim_status(false)
    }

    /// Re-inserts the SIM tray with the existing provisioned card.
    ///
    /// Fails if no SIM card is provisioned in the tray.
    pub(crate) fn reinsert_sim(&mut self) -> Result<Vec<ModemEffect>, ModemError> {
        if !self.sim_service.is_provisioned() {
            return Err(ModemError::InvalidConfig(
                "Cannot re-insert: SIM tray is empty".to_string(),
            ));
        }
        Ok(self.set_sim_status(true))
    }

    /// Removes and unprovisions the SIM card completely, wiping the filesystem
    /// and credentials.
    pub(crate) fn remove_sim(&mut self) -> Vec<ModemEffect> {
        let effects = self.set_sim_status(false);
        self.sim_service.remove_sim();
        effects
    }

    /// Synchronizes STK service and network registration state after a SIM
    /// change (profile switch or card insertion).
    fn sync_network_after_sim_change(&mut self, profile: &SimProfile) -> Vec<ModemEffect> {
        let mut effects = Vec::new();
        let home_plmn = profile.home_plmn();

        self.stk_service = StkService::new(profile.stk.clone());
        self.network_service.set_home_plmn(home_plmn);

        // Reset network registration to trigger fresh attachment to new home
        // PLMN
        effects.extend(
            self.set_registration(RegistrationType::Voice, RegistrationStatus::NotRegistered),
        );
        effects.extend(
            self.set_registration(RegistrationType::Data, RegistrationStatus::NotRegistered),
        );
        self.network_service.detach_network();

        if home_plmn.is_some() {
            effects.push(ModemEffect::Schedule {
                delay: Duration::from_millis(10),
                event: ModemEvent::AttachNetwork,
            });
        }

        if !self.quirks.goldfish_ril_37_or_earlier
            && let Some(urc) = self.sim_service.get_cpin_urc()
        {
            effects.push(ModemEffect::Response(urc.as_bytes().to_vec()));
        }

        effects
    }

    /// Switches the active SIM profile, rebuilding the SIM filesystem and
    /// resynchronizing network operator, MSISDN, STK, and registration
    /// states.
    pub(crate) fn switch_sim_profile(&mut self, profile: SimProfile) -> Vec<ModemEffect> {
        self.sim_service.load_profile(&profile);
        self.sync_network_after_sim_change(&profile)
    }

    /// Inserts a SIM card into the modem by applying the provided profile.
    /// Fails if a SIM card is already provisioned.
    pub(crate) fn insert_sim(
        &mut self,
        profile: SimProfile,
    ) -> Result<Vec<ModemEffect>, ModemError> {
        if !self.sim_service.insert_sim(&profile) {
            return Err(ModemError::InvalidConfig("SIM card is already inserted".to_string()));
        }

        Ok(self.sync_network_after_sim_change(&profile))
    }

    pub fn set_network_technology(&mut self, tech: RadioTechnology) -> Vec<ModemEffect> {
        let mut effects = Vec::new();
        if let Some(response) = self.network_service.set_network_technology(tech) {
            effects.push(ModemEffect::Response(response.as_bytes().to_vec()));
        }
        effects
    }

    // Removed set_waiting_for_sms_pdu, moved to SmsService

    /// Receives an AT command from the modem.
    pub fn receive_at_command(&mut self, command_bytes: &[u8]) -> Vec<ModemEffect> {
        let sms_pdu_action = if self.sms_service.is_waiting_for_prompt() {
            let sender = self.phone_number();
            self.sms_service.handle_prompt_input(
                &mut self.sim_service,
                &self.network_service,
                command_bytes,
                sender.as_ref().map(PhoneNumber::as_str),
            )
        } else {
            None
        };

        if let Some(result) = sms_pdu_action {
            let mut effects = Vec::new();
            match result {
                ExecutionResult::Success(handled) => {
                    let combined =
                        handled.responses.iter().map(ToString::to_string).collect::<String>();
                    if !combined.is_empty() {
                        effects.push(ModemEffect::Response(combined.into_bytes()));
                    }
                    for act in handled.actions {
                        effects.push(ModemEffect::Action(act));
                    }
                }
                err => {
                    let mut combined = String::new();
                    err.format_error_into(&mut combined, self.misc_service.cmee_mode());
                    effects.push(ModemEffect::Response(combined.into_bytes()));
                }
            }
            return effects;
        }

        let mut command_clean = command_bytes;
        while let Some(rest) =
            command_clean.strip_suffix(b"\r").or_else(|| command_clean.strip_suffix(b"\n"))
        {
            command_clean = rest;
        }
        let sub_commands = split_chained_commands(command_clean);
        if sub_commands.is_empty() {
            return Vec::new();
        }
        self.execute_chained_commands(&sub_commands)
    }

    pub fn tick(&mut self) -> Vec<ModemEffect> {
        Vec::new()
    }

    pub fn handle_event(&mut self, event: ModemEvent) -> Vec<ModemEffect> {
        let mut effects = Vec::new();
        match event {
            ModemEvent::TestEvent => {
                effects.push(ModemEffect::Response(b"TEST_EVENT_FIRED\r\n".to_vec()));
            }
            ModemEvent::CallRingTimeout { ring_token } => {
                let res = self.call_service.handle_ring_timeout(self.id, ring_token);
                let result: ExecutionResult = res.into();
                if let ExecutionResult::Success(handled) = result {
                    for action in handled.actions {
                        effects.push(ModemEffect::Action(action));
                    }
                }
            }
            ModemEvent::AttachNetwork => {
                if self.sim_service.is_present() {
                    let sim_ready = self.sim_service.is_ready();
                    let mut responses = self.network_service.attach_network(&self.sim_service);
                    // NITZ requires normal-service registration unless auto_ctzv quirk is enabled.
                    if (sim_ready || self.quirks.auto_ctzv)
                        && self.misc_service.ctzv_enabled()
                        && self.enable_unsolicited_urcs
                    {
                        responses.push(self.misc_service.current_time_update().to_string());
                    }
                    if self.enable_unsolicited_urcs {
                        let combined = responses.concat();
                        if !combined.is_empty() {
                            effects.push(ModemEffect::Response(combined.into_bytes()));
                        }
                    }
                }
            }
        }
        effects
    }

    pub fn get_sms_count(&self) -> u32 {
        (self.sim_service.get_sms_count() + self.sms_service.get_sms_count()) as u32
    }

    pub fn is_ringing(&self) -> bool {
        self.call_service.has_incoming() || self.call_service.has_alerting()
    }

    pub fn set_operator(&mut self, operator: &str) -> Vec<ModemEffect> {
        let mode = if operator.is_empty() { CopsMode::Automatic } else { CopsMode::Manual };
        let oper = if operator.is_empty() { None } else { Some(operator) };
        let mut effects = Vec::new();
        if let Ok(Some(crate::network_service::NetworkResponse::Urcs(urcs))) =
            self.network_service.set_operator_manual(mode, oper, &self.sim_service)
        {
            effects.extend(
                urcs.into_iter().map(|u| ModemEffect::Response(u.to_string().into_bytes())),
            );
        }
        effects
    }

    pub(crate) fn connect_call(&mut self, peer: ModemId) -> Option<Vec<u8>> {
        self.call_service.connect(self.id, peer)
    }

    /// Executes a single command and schedules any associated side-effects
    /// (e.g. network attachment).
    fn execute_and_schedule(
        &mut self,
        command: &Command,
        effects: &mut Vec<ModemEffect>,
    ) -> ExecutionResult {
        let was_sim_ready = self.sim_service.is_ready();
        let mut result = self.execute(command);
        if let ExecutionResult::Success(ref mut handled) = result {
            // Any transition into readiness (PIN, PUK, profile switch,
            // AT+CLCK disable) upgrades an emergency-only attach.
            if !was_sim_ready && self.sim_service.is_ready() && self.network_service.is_radio_on() {
                self.network_service.detach_network();
                effects.push(ModemEffect::Schedule {
                    delay: Duration::from_millis(10),
                    event: ModemEvent::AttachNetwork,
                });
            }
            if let Command::Network(NetworkCommand::SetRadioPower(power, reset)) = command {
                let should_reset = reset.unwrap_or(false);
                if should_reset || *power != RadioPowerLevel::Full {
                    for action in self.call_service.hangup_all(self.id) {
                        effects.push(ModemEffect::Action(action));
                    }
                    self.data_service.deactivate_all();
                }
                if *power == RadioPowerLevel::Full
                    && (!self.network_service.is_attached() || should_reset)
                {
                    effects.push(ModemEffect::Schedule {
                        delay: std::time::Duration::from_millis(10),
                        event: ModemEvent::AttachNetwork,
                    });
                }
            }
            let mode_active = match command {
                Command::Network(
                    NetworkCommand::SetVoiceNetworkRegistration(m)
                    | NetworkCommand::SetDataNetworkRegistration(m)
                    | NetworkCommand::SetLteNetworkRegistration(m),
                ) => *m != RegistrationUnsolicitedMode::Disable,
                _ => false,
            };
            if mode_active && !self.network_service.is_attached() {
                effects.push(ModemEffect::Schedule {
                    delay: std::time::Duration::from_millis(10),
                    event: ModemEvent::AttachNetwork,
                });
            }
            for action in std::mem::take(&mut handled.actions) {
                effects.push(ModemEffect::Action(action));
            }
        }
        result
    }

    /// Executes a list of chained commands sequentially, halting on error and
    /// merging responses.
    fn execute_chained_commands(&mut self, sub_commands: &[Vec<u8>]) -> Vec<ModemEffect> {
        let mut all_responses = Vec::new();
        let mut combined_effects = Vec::new();
        let mut stop_chain = false;

        for (i, cmd_bytes) in sub_commands.iter().enumerate() {
            let is_last = i == sub_commands.len() - 1;
            match Command::parse(cmd_bytes) {
                Ok((rem, command)) => {
                    let clean_rem = if rem == b"0"
                        && matches!(
                            command,
                            Command::Call(crate::call_service::CallCommand::Hangup)
                        ) {
                        b""
                    } else {
                        rem
                    };
                    if !clean_rem.is_empty() {
                        error!(
                            "Failed to parse AT command {:?} (trailing garbage: {:?})",
                            String::from_utf8_lossy(cmd_bytes),
                            String::from_utf8_lossy(rem)
                        );
                        all_responses.push(
                            CmeError::IncorrectParameters
                                .format_response(self.misc_service.cmee_mode()),
                        );
                        stop_chain = true;
                    } else {
                        let exec_res = self.execute_and_schedule(&command, &mut combined_effects);
                        match exec_res {
                            ExecutionResult::Success(handled) => {
                                let mut responses = handled.responses;
                                let success = responses.last() == Some(&Response::Ok);
                                if !is_last && success {
                                    responses.pop();
                                }
                                for r in responses {
                                    all_responses.push(r.to_string());
                                }
                                if !success {
                                    stop_chain = true;
                                }
                            }
                            err => {
                                let mut err_str = String::new();
                                err.format_error_into(&mut err_str, self.misc_service.cmee_mode());
                                all_responses.push(err_str);
                                stop_chain = true;
                            }
                        }
                    }
                }
                Err(error) => {
                    error!(
                        ?error,
                        "Failed to parse AT command {:?}",
                        String::from_utf8_lossy(cmd_bytes)
                    );
                    all_responses.push("ERROR\r\n".to_string());
                    stop_chain = true;
                }
            }
            if stop_chain {
                break;
            }
        }

        if !all_responses.is_empty() {
            let combined = all_responses.concat();
            combined_effects.insert(0, ModemEffect::Response(combined.into_bytes()));
        }
        combined_effects
    }

    pub fn execute(&mut self, command: &Command) -> ExecutionResult {
        match command {
            Command::Sim(c) => self.sim_service.execute(c),
            Command::Call(CallCommand::Dial(args)) if args.number.is_gprs_dial() => self
                .data_service
                .handle_gprs_dial(args.number.as_str(), &self.network_service)
                .into(),
            Command::Call(c) => {
                let clir_default = self.sup_service.clir_mode();
                self.call_service.execute(
                    c,
                    self.id,
                    clir_default,
                    &self.sim_service,
                    &self.network_service,
                )
            }
            Command::Sms(c) => {
                self.sms_service.execute(c, &mut self.sim_service, &self.network_service)
            }
            Command::Network(c) => {
                self.network_service.execute(c, &self.sim_service, self.enable_unsolicited_urcs)
            }
            Command::Data(c) => self.data_service.execute(c, &self.network_service),
            Command::Misc(c) => self.misc_service.execute(c),
            Command::Sup(c) => self.sup_service.execute(c),
            Command::Stk(c) => self.stk_service.execute(c, &self.sim_service),
        }
    }
}

/// Splits a chained command line by semicolons (ignoring them inside quotes)
/// and normalizes prefixes.
fn split_chained_commands(input: &[u8]) -> Vec<Vec<u8>> {
    let mut commands = Vec::new();
    let mut current = Vec::new();
    let mut in_quotes = false;
    for &b in input {
        if b == b'"' {
            in_quotes = !in_quotes;
            current.push(b);
        } else if b == b';' && !in_quotes {
            if !current.is_empty() {
                commands.push(current);
                current = Vec::new();
            }
        } else {
            current.push(b);
        }
    }
    if !current.is_empty() {
        commands.push(current);
    }

    let mut processed = Vec::new();
    for cmd in commands {
        let trimmed = trim_slice(&cmd);
        if trimmed.is_empty() {
            continue;
        }
        if starts_with_ignore_case(trimmed, b"AT") || starts_with_ignore_case(trimmed, b"RING") {
            processed.push(trimmed.to_vec());
        } else if trimmed.eq_ignore_ascii_case(b"OK") || trimmed.eq_ignore_ascii_case(b"ERROR") {
            // Ignore echoed responses/URCs
            debug!("Ignoring echoed response/URC: {:?}", std::str::from_utf8(trimmed));
        } else {
            let mut new_cmd = b"AT".to_vec();
            new_cmd.extend_from_slice(trimmed);
            processed.push(new_cmd);
        }
    }
    processed
}

/// Trims leading and trailing ASCII whitespace from a byte slice.
fn trim_slice(s: &[u8]) -> &[u8] {
    s.trim_ascii()
}

/// Checks if a byte slice starts with a prefix, ignoring ASCII case.
fn starts_with_ignore_case(s: &[u8], prefix: &[u8]) -> bool {
    s.get(..prefix.len()).is_some_and(|sub| sub.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::SimProfile, time::SystemClock};

    #[test]
    fn test_execute_chained_commands_parse_error() {
        let mut modem = ModemImpl::new(
            1,
            SimProfile::default(),
            Quirks::default(),
            Arc::new(SystemClock),
            Vec::new(),
        );
        // We pass a command Y that returns Err on Command::parse(Y).
        // Since Y does not start with AT or RING, and we bypass
        // split_chained_commands, we can pass it directly to
        // execute_chained_commands.
        let sub_commands = vec![b"INVALID".to_vec()];
        let effects = modem.execute_chained_commands(&sub_commands);

        assert_eq!(effects.len(), 1);
        if let ModemEffect::Response(resp) = &effects[0] {
            assert_eq!(resp, b"ERROR\r\n");
        } else {
            panic!("Expected Response effect");
        }
    }

    #[test]
    fn test_trigger_incoming_call_presentation_not_available() {
        let mut modem = ModemImpl::new(
            1,
            SimProfile::default(),
            Quirks::default(),
            Arc::new(SystemClock),
            Vec::new(),
        );
        modem.set_voice_registration(RegistrationStatus::RegisteredHome);
        // Enable CLIP via AT command
        modem.execute_chained_commands(&[b"AT+CLIP=1".to_vec()]);

        let phone = PhoneNumber::new_for_test("123456");
        let effects = modem
            .trigger_incoming_call(Some(&phone), NumberPresentation::NotAvailable, None)
            .expect("incoming call should be admitted");

        assert_eq!(effects.len(), 3);

        if let ModemEffect::Response(resp) = &effects[0] {
            assert_eq!(std::str::from_utf8(resp).unwrap(), "RING\r\n");
        } else {
            panic!("Expected Response effect for RING");
        }

        if let ModemEffect::Response(resp) = &effects[1] {
            assert_eq!(std::str::from_utf8(resp).unwrap(), "+CLIP: \"\",129,,,,2\r\n");
        } else {
            panic!("Expected Response effect for CLIP");
        }
    }
}
