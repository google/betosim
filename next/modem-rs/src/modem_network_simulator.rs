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

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, VecDeque, hash_map::Entry},
    sync::{Arc, atomic::Ordering as AtomicOrdering},
    time::{Duration, Instant},
};

use bytes::Bytes;
use netsim_model::{CellNetworkConfig, ModemAction, Quirks, RadioTechnology, RegistrationStatus};
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

use crate::{
    call_service::CallResponse,
    config::{ProfileMetadata, SimProfile},
    constants::{DEFAULT_FALLBACK_MSISDN, DEFAULT_MSISDN_PREFIX},
    metrics::{Metrics, MetricsSnapshot},
    modem::{ModemEffect, ModemEvent, ModemImpl},
    network_service::RegistrationType,
    profiles::get_builtin_profile,
    time::{Clock, SystemClock},
    types::{
        CommandAction, HangupReason, HostEvent, ModemError, ModemId, ModemSink, NumberPresentation,
        Parsable, PhoneNumber,
    },
};

#[derive(Debug)]
pub enum NetworkEvent {
    Response { id: ModemId, packet: Vec<u8> },
    ModemHungUp { id: ModemId },
    SinkError { id: ModemId },
}

// Internal representation of a scheduled event
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScheduledEvent {
    // Must be first so derived Ord orders primarily by deadline.
    pub when: Instant,
    pub modem_id: ModemId,
    pub event: ModemEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingStatusReport {
    sender_id: ModemId,
    report: crate::pdu::StatusReport,
}

#[derive(Debug, Clone)]
struct PendingIncomingSms {
    cmt_response: Vec<u8>,
    status_report: Option<PendingStatusReport>,
}

/// Represents the link state for incoming SMS delivery and acknowledgment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum IncomingSmsLinkState {
    /// No incoming SMS is currently in-flight on the link.
    #[default]
    Idle,
    /// An incoming SMS is in-flight to the modem awaiting recipient
    /// acknowledgment (`AT+CNMA`), with an optional pending status report
    /// to dispatch back to the sender if one was requested.
    Active(Option<PendingStatusReport>),
}

impl IncomingSmsLinkState {
    fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
}

#[derive(Debug)]
struct AcknowledgedSms {
    status_report: Option<PendingStatusReport>,
    next_cmt_response: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
struct IncomingSmsQueue {
    link_state: IncomingSmsLinkState,
    pending: VecDeque<PendingIncomingSms>,
}

impl IncomingSmsQueue {
    /// Enqueues an incoming SMS. If the link is idle, immediately activates it
    /// and returns `Some(cmt_response)` to deliver. Otherwise, queues it
    /// and returns `None`.
    fn enqueue(
        &mut self,
        cmt_response: Vec<u8>,
        status_report: Option<PendingStatusReport>,
    ) -> Option<Vec<u8>> {
        if self.link_state.is_idle() {
            self.link_state = IncomingSmsLinkState::Active(status_report);
            Some(cmt_response)
        } else {
            self.pending.push_back(PendingIncomingSms { cmt_response, status_report });
            None
        }
    }

    /// Acknowledges the active in-flight SMS, transitioning the link to the
    /// next queued SMS or back to `Idle`. Returns `Some(AcknowledgedSms)`
    /// if an SMS was active, or `None` if idle.
    fn acknowledge(&mut self) -> Option<AcknowledgedSms> {
        match std::mem::take(&mut self.link_state) {
            IncomingSmsLinkState::Active(status_report) => {
                let next_cmt_response = self.pending.pop_front().map(|next| {
                    self.link_state = IncomingSmsLinkState::Active(next.status_report);
                    next.cmt_response
                });
                Some(AcknowledgedSms { status_report, next_cmt_response })
            }
            IncomingSmsLinkState::Idle => None,
        }
    }

    /// Drops any pending status reports addressed to the removed sender modem.
    fn remove_sender(&mut self, sender_id: ModemId) {
        if let IncomingSmsLinkState::Active(Some(report)) = &self.link_state
            && report.sender_id == sender_id
        {
            self.link_state = IncomingSmsLinkState::Active(None);
        }
        for pending in &mut self.pending {
            if pending.status_report.as_ref().is_some_and(|r| r.sender_id == sender_id) {
                pending.status_report = None;
            }
        }
    }

    /// Returns `true` if the link is idle and no incoming messages are pending.
    fn is_empty(&self) -> bool {
        self.link_state.is_idle() && self.pending.is_empty()
    }
}

/// Maximum number of call diversions allowed for a single call (3GPP TS 24.082).
const MAX_CALL_FORWARD_HOPS: usize = 5;

/// Manages multiple modems and simulates network interactions.
pub struct ModemNetworkSimulator {
    event_queue: BinaryHeap<Reverse<ScheduledEvent>>,
    modems: HashMap<ModemId, ModemImpl>,
    sinks: HashMap<ModemId, ModemSink>,
    host_event_tx: mpsc::UnboundedSender<HostEvent>,
    metrics: Arc<Metrics>,
    clock: Arc<dyn Clock>,
    modem_chip_count: usize,
    incoming_sms: HashMap<ModemId, IncomingSmsQueue>,
}

impl ModemNetworkSimulator {
    /// Creates a new `ModemNetworkSimulator` with a real system clock.
    pub fn new(host_event_tx: tokio::sync::mpsc::UnboundedSender<HostEvent>) -> Self {
        Self::new_with_clock(Arc::new(SystemClock), host_event_tx)
    }

    /// Creates a new `ModemNetworkSimulator` with a specific clock for testing.
    pub fn new_with_clock(
        clock: Arc<dyn Clock>,
        host_event_tx: tokio::sync::mpsc::UnboundedSender<HostEvent>,
    ) -> Self {
        Self {
            modems: HashMap::new(),
            sinks: HashMap::new(),
            host_event_tx,
            event_queue: BinaryHeap::new(),
            metrics: Arc::new(Metrics::default()),
            clock,
            modem_chip_count: 0,
            incoming_sms: HashMap::new(),
        }
    }

    pub fn on_timer(&mut self, _id: ModemId) -> Vec<NetworkEvent> {
        self.tick().0
    }

    pub fn dispatch(&mut self, command: ModemAction) -> Vec<NetworkEvent> {
        match command {
            ModemAction::ProcessAtCommand { id, command } => self.send_at_command(id.0, &command),
            ModemAction::SetSignalStrength { id, rssi, ber } => {
                self.set_signal_strength(id.0, rssi, ber)
            }
            ModemAction::SetVoiceRegistration { id, status } => {
                self.set_registration(id.0, RegistrationType::Voice, status)
            }
            ModemAction::SetDataRegistration { id, status } => {
                self.set_registration(id.0, RegistrationType::Data, status)
            }
            ModemAction::IncomingCall { target_id, number } => {
                self.initiate_external_incoming_call(target_id.0, &number)
            }
            ModemAction::RemoteAnswer { id } => self.initiate_external_answer(id.0),
            ModemAction::RemoteHold { id, on_hold } => self.set_call_hold(id.0, on_hold),
            ModemAction::RemoteHangup { id } => self.initiate_external_hangup(id.0),
            ModemAction::IncomingSms { id, sender, text } => {
                self.send_incoming_sms(id.0, &sender, &text)
            }
            ModemAction::IncomingPdu { id, pdu } => self.send_incoming_pdu(id.0, &pdu),
            ModemAction::InjectCellBroadcast { id, serial_number, message_id, dcs, text } => {
                self.send_cell_broadcast(id.0, serial_number, message_id, dcs, &text)
            }
            ModemAction::UpdateNetworkTime { id } => self.update_network_time(id.0),
            ModemAction::UpdatePhysicalChannelConfigs { id } => {
                self.update_physical_channel_configs(id.0)
            }
            ModemAction::SetSimStatus { id, present } => {
                if present {
                    self.reinsert_sim(id.0).unwrap_or_default()
                } else {
                    self.eject_sim(id.0).unwrap_or_default()
                }
            }
            ModemAction::SetNetworkTechnology { id, tech } => {
                self.set_network_technology(id.0, tech)
            }
            ModemAction::SetOperator { id, operator } => self.set_operator(id.0, &operator),
        }
    }

    pub fn set_sim_status(&mut self, id: ModemId, present: bool) -> Vec<NetworkEvent> {
        if present {
            self.reinsert_sim(id).unwrap_or_default()
        } else {
            self.eject_sim(id).unwrap_or_default()
        }
    }

    pub fn set_network_technology(
        &mut self,
        id: ModemId,
        tech: RadioTechnology,
    ) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.set_network_technology(tech))
    }

    pub fn set_operator(&mut self, id: ModemId, operator: &str) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.set_operator(operator))
    }

    /// Creates a new modem instance.
    pub fn new_modem(
        &mut self,
        id: ModemId,
        sink: ModemSink,
        sim_type: Option<i32>,
        sim_profile: Option<String>,
        quirks: Quirks,
        network_configs: Vec<CellNetworkConfig>,
    ) -> Result<(), ModemError> {
        let profile = match sim_profile {
            Some(xml) => crate::xml_profile::parse_xml_profile(&xml).map_err(|e| {
                ModemError::InvalidConfig(format!("Failed to parse SIM profile XML: {e}"))
            })?,
            None => {
                let numeric_type = match sim_type {
                    Some(0) => crate::profiles::SIM_TYPE_DEFAULT,
                    Some(t) => t,
                    None => {
                        if quirks.is_cuttlefish {
                            crate::profiles::SIM_TYPE_TEL_ALASKA
                        } else {
                            crate::profiles::SIM_TYPE_DEFAULT
                        }
                    }
                };
                get_builtin_profile(numeric_type).ok_or_else(|| {
                    ModemError::InvalidProfile(format!("Unknown sim_type: {numeric_type}"))
                })?
            }
        };

        self.new_modem_with_profile(id, sink, Some(profile), sim_type, quirks, network_configs)
    }

    /// Creates a new modem instance with a specific SIM profile.
    pub fn new_modem_with_profile(
        &mut self,
        id: ModemId,
        sink: ModemSink,
        profile: Option<SimProfile>,
        sim_type: Option<i32>,
        quirks: Quirks,
        network_configs: Vec<CellNetworkConfig>,
    ) -> Result<(), ModemError> {
        if self.modems.contains_key(&id) {
            return Err(ModemError::DuplicateModemId(id));
        }
        self.modem_chip_count += 1;
        let profile = profile.unwrap_or_default();
        let target_msisdn = match profile.msisdn.clone() {
            Some(msisdn) => msisdn,
            None => {
                let s = format!("{}{:03}", DEFAULT_MSISDN_PREFIX, self.modem_chip_count);
                PhoneNumber::try_from(s.clone()).map_err(|e| {
                    ModemError::InvalidProfile(format!("Invalid generated phone number '{s}': {e}"))
                })?
            }
        };
        let mut modem = ModemImpl::new(id, profile, quirks, self.clock.clone(), network_configs);
        // Override the default dummy number with a unique generated one to prevent
        // conflicts when launching multiple default emulators. Custom profiles are
        // preserved.
        if modem.phone_number().is_none()
            || modem
                .phone_number()
                .as_ref()
                .is_some_and(|n| n.normalized() == DEFAULT_FALLBACK_MSISDN)
        {
            modem.set_phone_number(target_msisdn);
        }
        if let Some(t) = sim_type {
            modem.set_sim_status(t > 0);
        }
        self.modems.insert(id, modem);
        self.sinks.insert(id, sink);
        Ok(())
    }

    /// Removes a modem instance.
    pub fn remove_modem(&mut self, id: ModemId) {
        self.modems.remove(&id);
        self.sinks.remove(&id);
        self.incoming_sms.remove(&id);
        for queue in self.incoming_sms.values_mut() {
            queue.remove_sender(id);
        }
    }

    /// Switches the active SIM profile of a modem instance.
    pub fn switch_sim_profile(
        &mut self,
        id: ModemId,
        profile: SimProfile,
    ) -> Result<Vec<NetworkEvent>, ModemError> {
        let modem = self.modems.get_mut(&id).ok_or(ModemError::UnknownModemId(id))?;
        self.incoming_sms.remove(&id);
        let modem_effects = modem.switch_sim_profile(profile);
        let effects = modem_effects.into_iter().map(|e| (id, e)).collect();
        Ok(self.process_effects(effects))
    }

    /// Switches the active SIM profile of a modem instance by numeric
    /// `sim_type`.
    pub fn switch_sim_profile_by_type(
        &mut self,
        id: ModemId,
        sim_type: i32,
    ) -> Result<Vec<NetworkEvent>, ModemError> {
        let profile = get_builtin_profile(sim_type)
            .ok_or_else(|| ModemError::InvalidProfile(format!("Unknown sim_type: {sim_type}")))?;
        self.switch_sim_profile(id, profile)
    }

    /// Inserts a SIM card into a modem instance. Fails if a SIM card is already
    /// provisioned.
    pub fn insert_sim(
        &mut self,
        id: ModemId,
        profile: SimProfile,
    ) -> Result<Vec<NetworkEvent>, ModemError> {
        let modem = self.modems.get_mut(&id).ok_or(ModemError::UnknownModemId(id))?;
        let modem_effects = modem.insert_sim(profile)?;
        let effects = modem_effects.into_iter().map(|e| (id, e)).collect();
        Ok(self.process_effects(effects))
    }

    /// Ejects the SIM tray of a modem instance, cutting power while retaining
    /// card credentials.
    pub fn eject_sim(&mut self, id: ModemId) -> Result<Vec<NetworkEvent>, ModemError> {
        let modem = self.modems.get_mut(&id).ok_or(ModemError::UnknownModemId(id))?;
        let modem_effects = modem.eject_sim();
        let effects = modem_effects.into_iter().map(|e| (id, e)).collect();
        Ok(self.process_effects(effects))
    }

    /// Re-inserts the SIM tray with the existing provisioned card.
    pub fn reinsert_sim(&mut self, id: ModemId) -> Result<Vec<NetworkEvent>, ModemError> {
        let modem = self.modems.get_mut(&id).ok_or(ModemError::UnknownModemId(id))?;
        let modem_effects = modem.reinsert_sim()?;
        let effects = modem_effects.into_iter().map(|e| (id, e)).collect();
        Ok(self.process_effects(effects))
    }

    /// Removes and unprovisions the SIM card completely from a modem instance.
    pub fn remove_sim(&mut self, id: ModemId) -> Result<Vec<NetworkEvent>, ModemError> {
        let modem = self.modems.get_mut(&id).ok_or(ModemError::UnknownModemId(id))?;
        self.incoming_sms.remove(&id);
        let modem_effects = modem.remove_sim();
        let effects = modem_effects.into_iter().map(|e| (id, e)).collect();
        Ok(self.process_effects(effects))
    }

    /// Returns summary metadata for the active SIM profile of a modem instance,
    /// or `None` if the modem does not exist or has no active SIM inserted.
    pub fn get_sim_metadata(&self, id: ModemId) -> Option<ProfileMetadata> {
        self.modems.get(&id).and_then(|m| m.get_profile_metadata())
    }

    pub fn update_network_configs(
        &mut self,
        id: ModemId,
        configs: Vec<CellNetworkConfig>,
    ) -> Result<(), ModemError> {
        let modem = self.modems.get_mut(&id).ok_or(ModemError::UnknownModemId(id))?;
        modem.update_network_configs(configs);
        Ok(())
    }

    /// Sends an AT command to a modem instance.
    pub fn send_at_command(&mut self, id: ModemId, cmd: &[u8]) -> Vec<NetworkEvent> {
        self.metrics.at_commands_received.fetch_add(1, AtomicOrdering::Relaxed);
        debug!("Received AT command for {}: {:?}", id, std::str::from_utf8(cmd));
        self.tick();
        self.apply_to_modem(id, |modem| modem.receive_at_command(cmd))
    }

    fn process_effects(&mut self, effects: Vec<(ModemId, ModemEffect)>) -> Vec<NetworkEvent> {
        let mut network_events = Vec::new();
        let mut queue = VecDeque::from(effects);

        while let Some((id, effect)) = queue.pop_front() {
            match effect {
                ModemEffect::Schedule { delay, event } => {
                    let msg = HostEvent::TimerRequest { chip_id: id, duration: delay };
                    if let Err(error) = self.host_event_tx.send(msg) {
                        error!(%error, "Failed to send timer request");
                    }
                    self.schedule_event(id, delay, event);
                }
                ModemEffect::Response(packet) => {
                    let mut packet = packet;
                    if let Some(modem) = self.modems.get(&id) {
                        // Goldfish RIL in SDK 37 and earlier (without TTY raw mode) translates \r
                        // to \n on input, turning \r\n into \n\n and
                        // corrupting stream. We work around this by
                        // only sending \r (which translates to a single \n on the guest).
                        let translate_crlf_to_cr = modem.quirks.goldfish_ril_37_or_earlier;
                        if translate_crlf_to_cr {
                            packet = replace_crlf_with_cr(&packet);
                        }
                    }
                    debug!("Sending response to {}: {:?}", id, std::str::from_utf8(&packet));
                    if let Some(sink) = self.sinks.get_mut(&id)
                        && let Err(error) = sink.send(Bytes::from(packet))
                    {
                        error!(%error, "Failed to send response to modem {}", id);
                        let event = HostEvent::SinkError(id);
                        if let Err(error) = self.host_event_tx.send(event) {
                            error!(%error, "Failed to send client sink error");
                        }
                        network_events.push(NetworkEvent::SinkError { id });
                    }
                }
                ModemEffect::Action(action) => {
                    let (new_effects, events) = self.handle_command_action(id, action);
                    queue.extend(new_effects);
                    network_events.extend(events);
                }
            }
        }
        network_events
    }

    fn apply_to_modem<F>(&mut self, id: ModemId, f: F) -> Vec<NetworkEvent>
    where
        F: FnOnce(&mut ModemImpl) -> Vec<ModemEffect>,
    {
        let effects = if let Some(modem) = self.modems.get_mut(&id) {
            f(modem).into_iter().map(|e| (id, e)).collect()
        } else {
            Vec::new()
        };
        self.process_effects(effects)
    }

    // Helper to find peer ID.
    fn find_peer_id<P>(&self, source_id: ModemId, predicate: P) -> Option<ModemId>
    where
        P: Fn(&ModemImpl) -> bool,
    {
        self.modems.values().find(|m| m.id != source_id && predicate(m)).map(|m| m.id)
    }

    pub(crate) fn handle_command_action(
        &mut self,
        id: ModemId,
        action: CommandAction,
    ) -> (Vec<(ModemId, ModemEffect)>, Vec<NetworkEvent>) {
        debug!("[Network] Handling action from {}: {:?}", id, action);
        let mut effects = Vec::new();
        let mut events = Vec::new();

        match action {
            CommandAction::InitiateCall(args) => {
                self.metrics.calls_initiated.fetch_add(1, AtomicOrdering::Relaxed);
                effects.extend(self.initiate_call(id, &args.number, args.is_emergency));
            }
            CommandAction::AnswerCall(answered_modem_id) => {
                self.metrics.calls_answered.fetch_add(1, AtomicOrdering::Relaxed);

                let caller_id = self
                    .find_peer_id(answered_modem_id, |m| m.has_outbound_to_peer(answered_modem_id))
                    .or_else(|| self.find_peer_id(answered_modem_id, |m| m.has_outbound_call()));

                if let Some(cid) = caller_id {
                    // Caller connects
                    if let Some(caller) = self.modems.get_mut(&cid)
                        && let Some(response) = caller.connect_call(answered_modem_id)
                    {
                        effects.push((cid, ModemEffect::Response(response)));
                    }
                    // Callee connects
                    if let Some(callee) = self.modems.get_mut(&answered_modem_id)
                        && let Some(response) = callee.connect_call(cid)
                    {
                        effects.push((answered_modem_id, ModemEffect::Response(response)));
                    }
                }
            }
            CommandAction::HangupCall { initiator, target_peer, reason } => {
                self.metrics.calls_hung_up.fetch_add(1, AtomicOrdering::Relaxed);

                if let Some(forward_effects) =
                    self.try_forward_hangup(initiator, target_peer, reason)
                {
                    effects.extend(forward_effects);
                } else if let Some(target) = self.modems.get_mut(&target_peer)
                    && target.receive_hangup_from_peer_id(initiator)
                {
                    let urc = reason.as_urc_str(target.quirks.goldfish_ril_37_or_earlier);
                    effects.push((target_peer, ModemEffect::Response(urc.as_bytes().to_vec())));
                }

                if let Some(initiator_modem) = self.modems.get(&initiator)
                    && !initiator_modem.has_calls()
                {
                    events.push(NetworkEvent::ModemHungUp { id: initiator });
                }
            }
            CommandAction::HoldCall { holder, target } => {
                if let Some(hold_modem) = self.modems.get_mut(&target) {
                    hold_modem.receive_peer_hold(holder, true);
                    effects.push((target, ModemEffect::Response(b"RING\r\n".to_vec())));
                }
            }
            CommandAction::ResumeCall { resumer, target } => {
                if let Some(resume_modem) = self.modems.get_mut(&target) {
                    resume_modem.receive_peer_hold(resumer, false);
                    effects.push((target, ModemEffect::Response(b"RING\r\n".to_vec())));
                }
            }
            CommandAction::ReceiveSms { to, pdu, status_report } => {
                self.metrics.sms_sent.fetch_add(1, AtomicOrdering::Relaxed);
                let peer_id = if let Some(ref num) = to {
                    let sender_num = self.modems.get(&id).and_then(|m| m.phone_number());
                    if sender_num.as_ref().map(|n| n.normalized()) == Some(normalize_number(num)) {
                        Some(id)
                    } else if let Some(peer) = self.find_peer_id(id, |m| {
                        m.phone_number()
                            .as_ref()
                            .is_some_and(|n| n.normalized() == normalize_number(num))
                    }) {
                        Some(peer)
                    } else {
                        warn!("No peer found with number {}, dropping SMS", num);
                        None
                    }
                } else {
                    debug!("No destination number, falling back to loopback (self)");
                    Some(id)
                };

                if let Some(peer_id) = peer_id {
                    let tpdu_len = crate::pdu::calculate_tpdu_len(&pdu);
                    let peer_is_goldfish_37 = self
                        .modems
                        .get(&peer_id)
                        .is_some_and(|m| m.quirks.goldfish_ril_37_or_earlier);

                    // Goldfish RIL in SDK 37 and earlier expects "+CMT: <len>" (no leading comma)
                    // for incoming PDU.
                    let omit_cmt_leading_comma = peer_is_goldfish_37;
                    let mut response = b"+CMT: ".to_vec();
                    if !omit_cmt_leading_comma {
                        response.push(b',');
                    }
                    response.extend_from_slice(tpdu_len.to_string().as_bytes());
                    response.extend_from_slice(b"\r\n");
                    response.extend_from_slice(&pdu);
                    response.extend_from_slice(b"\r\n");
                    let status_report_pending =
                        status_report.map(|report| PendingStatusReport { sender_id: id, report });

                    let queue = self.incoming_sms.entry(peer_id).or_default();
                    if let Some(deliver_response) = queue.enqueue(response, status_report_pending) {
                        effects.push((peer_id, ModemEffect::Response(deliver_response)));
                    }
                }
            }
            CommandAction::AcknowledgeIncomingSms { ack } => {
                if let Entry::Occupied(mut entry) = self.incoming_sms.entry(id) {
                    let queue = entry.get_mut();
                    if let Some(ack_sms) = queue.acknowledge() {
                        if let Some(pending) = ack_sms.status_report {
                            if ack.is_success() {
                                let report_pdu = pending.report.to_pdu_hex(&self.clock.now_zoned());
                                let report_tpdu_len =
                                    crate::pdu::calculate_tpdu_len(report_pdu.as_bytes());
                                let report_response =
                                    format!("+CDS: {report_tpdu_len}\r\n{report_pdu}\r\n")
                                        .into_bytes();
                                effects.push((
                                    pending.sender_id,
                                    ModemEffect::Response(report_response),
                                ));
                            } else {
                                debug!(
                                    "Incoming SMS negatively acknowledged by modem {id}, dropping status report"
                                );
                            }
                        }
                        if let Some(response) = ack_sms.next_cmt_response {
                            effects.push((id, ModemEffect::Response(response)));
                        }
                    }
                    if queue.is_empty() {
                        entry.remove();
                    }
                }
            }
            CommandAction::ReceiveTextSms { to, text } => {
                self.metrics.sms_sent.fetch_add(1, AtomicOrdering::Relaxed);
                let sender_num = self.modems.get(&id).and_then(|m| m.phone_number());
                let peer_id =
                    if sender_num.as_ref().map(|n| n.normalized()) == Some(normalize_number(&to)) {
                        Some(id)
                    } else {
                        self.find_peer_id(id, |m| {
                            m.phone_number()
                                .as_ref()
                                .is_some_and(|n| n.normalized() == normalize_number(&to))
                        })
                    };

                if let Some(pid) = peer_id {
                    let sender_num_str = sender_num.as_ref().map(|n| n.as_str()).unwrap_or("");
                    if let Some(peer_modem) = self.modems.get_mut(&pid) {
                        let peer_effects = peer_modem.trigger_incoming_sms(sender_num_str, &text);
                        effects.extend(peer_effects.into_iter().map(|e| (pid, e)));
                    }
                }
            }
            CommandAction::None => {} // No-op
        }
        (effects, events)
    }

    pub fn set_signal_strength(&mut self, id: ModemId, rssi: u8, ber: u8) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.set_signal_strength(rssi, ber))
    }

    pub fn set_registration(
        &mut self,
        id: ModemId,
        reg_type: RegistrationType,
        status: RegistrationStatus,
    ) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.set_registration(reg_type, status))
    }

    pub fn set_voice_registration(
        &mut self,
        id: ModemId,
        status: RegistrationStatus,
    ) -> Vec<NetworkEvent> {
        self.set_registration(id, RegistrationType::Voice, status)
    }

    pub fn set_data_registration(
        &mut self,
        id: ModemId,
        status: RegistrationStatus,
    ) -> Vec<NetworkEvent> {
        self.set_registration(id, RegistrationType::Data, status)
    }

    pub fn initiate_external_incoming_call(
        &mut self,
        target_id: ModemId,
        number: &str,
    ) -> Vec<NetworkEvent> {
        let Ok((rem, phone)) = PhoneNumber::parse(number.as_bytes()) else {
            warn!("Invalid incoming call number: {}", number);
            return Vec::new();
        };
        if !rem.is_empty() {
            warn!("Invalid incoming call number: {}", number);
            return Vec::new();
        }
        self.apply_to_modem(target_id, |modem| {
            modem
                .trigger_incoming_call(Some(&phone), NumberPresentation::Allowed, None)
                .unwrap_or_default()
        })
    }

    pub fn initiate_external_answer(&mut self, id: ModemId) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.trigger_remote_answer())
    }

    pub fn set_call_hold(&mut self, id: ModemId, on_hold: bool) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.trigger_remote_hold(on_hold))
    }

    pub fn initiate_external_hangup(&mut self, id: ModemId) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.trigger_remote_hangup())
    }

    pub fn send_incoming_sms(
        &mut self,
        id: ModemId,
        sender: &str,
        text: &str,
    ) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.trigger_incoming_sms(sender, text))
    }

    pub fn send_incoming_pdu(&mut self, id: ModemId, pdu: &str) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.trigger_incoming_pdu(pdu))
    }

    pub fn send_cell_broadcast(
        &mut self,
        id: ModemId,
        serial_number: u16,
        message_id: u16,
        dcs: u8,
        text: &str,
    ) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| {
            modem.trigger_incoming_cell_broadcast(serial_number, message_id, dcs, text)
        })
    }

    pub fn send_cell_broadcast_pdu(&mut self, id: ModemId, pdu: &str) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.trigger_incoming_cell_broadcast_pdu(pdu))
    }

    pub fn update_network_time(&mut self, id: ModemId) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.trigger_network_time_update())
    }

    // TODO(b/562993662): Plumb through ModemAction when exposed in cell.proto /
    // netsim gsm.
    pub fn trigger_emergency_callback_mode(
        &mut self,
        id: ModemId,
        enabled: bool,
    ) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.trigger_emergency_callback_mode(enabled))
    }

    pub fn update_physical_channel_configs(&mut self, id: ModemId) -> Vec<NetworkEvent> {
        self.apply_to_modem(id, |modem| modem.update_physical_channel_configs())
    }

    fn try_forward_hangup(
        &mut self,
        initiator: ModemId,
        caller_id: ModemId,
        reason: HangupReason,
    ) -> Option<Vec<(ModemId, ModemEffect)>> {
        let fwd_num = self.modems.get(&initiator)?.forward_number_on_release(reason)?;
        let caller = self.modems.get(&caller_id)?;
        let outbound = caller
            .calls()
            .iter()
            .find(|c| c.state.is_outbound() && c.peer_id == Some(initiator))?;
        let caller_number = caller.phone_number();
        let presentation = outbound.number_presentation;

        self.route_call_to_number(
            caller_id,
            caller_number.as_ref(),
            presentation,
            &fwd_num,
            vec![caller_id, initiator],
        )
    }

    fn route_call_to_number(
        &mut self,
        caller_id: ModemId,
        caller_number: Option<&PhoneNumber>,
        presentation: NumberPresentation,
        initial_target: &PhoneNumber,
        mut visited: Vec<ModemId>,
    ) -> Option<Vec<(ModemId, ModemEffect)>> {
        let mut current_target = initial_target.clone();
        loop {
            let normalized_target = current_target.normalized();
            if visited.len() > 1
                && caller_number.is_some_and(|n| n.normalized() == normalized_target)
            {
                return None;
            }
            let target_id = self.find_peer_id(caller_id, |m| {
                m.phone_number().as_ref().is_some_and(|n| n.normalized() == normalized_target)
            });

            let Some(tid) = target_id else {
                if let Some(caller) = self.modems.get_mut(&caller_id) {
                    caller.set_outbound_call_peer_id(None);
                }
                return Some(Vec::new());
            };

            if visited.contains(&tid) {
                return None;
            }

            if let Some(next_number) = self.modems.get(&tid)?.incoming_call_forward_target() {
                if visited.len() > MAX_CALL_FORWARD_HOPS {
                    return None;
                }
                visited.push(tid);
                current_target = next_number;
                continue;
            }

            let callee_effects = self.modems.get_mut(&tid)?.trigger_incoming_call(
                caller_number,
                presentation,
                Some(caller_id),
            )?;

            if let Some(caller) = self.modems.get_mut(&caller_id) {
                caller.set_outbound_call_peer_id(Some(tid));
            }

            return Some(callee_effects.into_iter().map(|e| (tid, e)).collect());
        }
    }

    /// Initiates a call from one modem to another (Internal).
    fn initiate_call(
        &mut self,
        caller_id: ModemId,
        phone_number: &PhoneNumber,
        is_emergency: bool,
    ) -> Vec<(ModemId, ModemEffect)> {
        debug!(
            "[Network] initiate_call: caller_id={}, phone_number={}, is_emergency={}",
            caller_id,
            phone_number.as_str(),
            is_emergency,
        );
        if is_emergency {
            debug!("[Network] Emergency call initiated for caller {}", caller_id);
            if let Some(caller) = self.modems.get_mut(&caller_id) {
                if caller.remote_answer() {
                    self.metrics.calls_answered.fetch_add(1, AtomicOrdering::Relaxed);
                    return vec![(
                        caller_id,
                        ModemEffect::Response(CallResponse::Ring.to_string().into_bytes()),
                    )];
                }
                warn!("[Network] Emergency call remote_answer failed for caller {}", caller_id);
            }
            return Vec::new();
        }
        if tracing::enabled!(tracing::Level::DEBUG) {
            for (id, modem) in &self.modems {
                debug!(
                    "[Network]   registered modem id={}, phone_number='{}'",
                    id,
                    modem.phone_number().as_ref().map(|n| n.as_str()).unwrap_or("None")
                );
            }
        }

        let (caller_number, presentation) = match self.modems.get(&caller_id) {
            Some(caller) => {
                let number = caller.phone_number();
                let presentation = caller
                    .calls()
                    .iter()
                    .find(|c| c.state.is_outbound())
                    .map_or(NumberPresentation::Allowed, |c| c.number_presentation);
                (number, presentation)
            }
            None => (None, NumberPresentation::Allowed),
        };

        match self.route_call_to_number(
            caller_id,
            caller_number.as_ref(),
            presentation,
            phone_number,
            vec![caller_id],
        ) {
            Some(effects) => effects,
            None => {
                if let Some(caller) = self.modems.get_mut(&caller_id)
                    && caller.fail_outbound_call()
                {
                    let urc =
                        HangupReason::Normal.as_urc_str(caller.quirks.goldfish_ril_37_or_earlier);
                    vec![(caller_id, ModemEffect::Response(urc.as_bytes().to_vec()))]
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// Ticks the event loop.
    pub fn tick(&mut self) -> (Vec<NetworkEvent>, Option<Duration>) {
        let now = self.clock.now();
        let mut effects = Vec::new();

        while let Some(event) = self.event_queue.peek()
            && event.0.when <= now
        {
            if let Some(std::cmp::Reverse(event)) = self.event_queue.pop()
                && let Some(modem) = self.modems.get_mut(&event.modem_id)
            {
                let new_effects: Vec<_> = modem
                    .handle_event(event.event)
                    .into_iter()
                    .map(|e| (event.modem_id, e))
                    .collect();
                effects.extend(new_effects);
            }
        }
        let events = self.process_effects(effects);

        let next_duration = self.event_queue.peek().map(|e| {
            let when = e.0.when;
            if when > now { when - now } else { Duration::ZERO }
        });

        (events, next_duration)
    }

    /// Returns a snapshot of the current metrics.
    pub fn get_metrics(&self) -> MetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Returns the number of modem instances.
    pub fn get_modem_count(&self) -> usize {
        self.modems.len()
    }

    /// Returns a list of modem IDs.
    pub fn get_modem_ids(&self) -> Vec<ModemId> {
        self.modems.keys().copied().collect()
    }

    /// Returns a modem instance.
    pub fn get_modem(&self, id: ModemId) -> Option<&ModemImpl> {
        self.modems.get(&id)
    }

    pub fn get_modem_mut(&mut self, id: ModemId) -> Option<&mut ModemImpl> {
        self.modems.get_mut(&id)
    }

    pub fn schedule_event(&mut self, modem_id: ModemId, delay: Duration, event: ModemEvent) {
        let when = self.clock.now() + delay;
        self.event_queue.push(Reverse(ScheduledEvent { when, modem_id, event }));
    }
}

fn normalize_number(num: &str) -> &str {
    num.strip_prefix('+').unwrap_or(num)
}

fn replace_crlf_with_cr(packet: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(packet.len());
    let mut iter = packet.iter().peekable();

    while let Some(&b) = iter.next() {
        if b == b'\r' && iter.peek() == Some(&&b'\n') {
            iter.next(); // Consume and skip the '\n'
        }
        result.push(b);
    }

    result.shrink_to_fit();
    result
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use netsim_model::{ChipId, ModemAction};

    use super::*;
    use crate::{
        modem_network::ModemNetworkInterface, test_utils::MockModemHandler, time::MockClock,
    };

    #[test]
    fn test_event_loop_tick_and_duration() {
        let clock = Arc::new(MockClock::default());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut simulator = ModemNetworkSimulator::new_with_clock(clock.clone(), tx);

        let modem_id: ModemId = 1;
        let (mut modem_handler, sink) = MockModemHandler::new(false);
        simulator.new_modem(modem_id, sink, None, None, Quirks::default(), Vec::new()).unwrap();

        // 1. Schedule an event 100ms in the future.
        let event_duration = Duration::from_millis(100);
        simulator.schedule_event(modem_id, event_duration, ModemEvent::TestEvent);

        // 2. Tick before the event is due.
        // It should return the duration until the next event.
        let (events, next_duration) = simulator.tick();
        assert!(events.is_empty());
        assert_eq!(next_duration, Some(event_duration));

        // 3. Advance the clock manually.
        clock.advance(event_duration);

        // 4. Tick again. The event should fire now.
        let (_events_after, next_duration_after) = simulator.tick();
        assert!(next_duration_after.is_none());

        // 5. Check that the event was handled.
        let response = modem_handler.wait_for_response();
        assert_eq!(response, b"TEST_EVENT_FIRED\r\n");
    }

    #[test]
    fn test_event_loop_fires_earliest_event_first() {
        let clock = Arc::new(MockClock::default());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut simulator = ModemNetworkSimulator::new_with_clock(clock.clone(), tx);

        let modem_id: ModemId = 1;
        let (mut modem_handler, sink) = MockModemHandler::new(false);
        simulator.new_modem(modem_id, sink, None, None, Quirks::default(), Vec::new()).unwrap();

        simulator.schedule_event(modem_id, Duration::from_millis(200), ModemEvent::TestEvent);
        simulator.schedule_event(modem_id, Duration::from_millis(10), ModemEvent::TestEvent);

        let (events, next_duration) = simulator.tick();
        assert!(events.is_empty());
        assert_eq!(next_duration, Some(Duration::from_millis(10)));
        assert_eq!(modem_handler.try_get_response(), None);

        clock.advance(Duration::from_millis(10));
        let (_events, next_duration) = simulator.tick();
        assert_eq!(next_duration, Some(Duration::from_millis(190)));
        assert_eq!(modem_handler.wait_for_response(), b"TEST_EVENT_FIRED\r\n");
        assert_eq!(modem_handler.try_get_response(), None);

        clock.advance(Duration::from_millis(190));
        let (_events, next_duration) = simulator.tick();
        assert!(next_duration.is_none());
        assert_eq!(modem_handler.wait_for_response(), b"TEST_EVENT_FIRED\r\n");
        assert_eq!(modem_handler.try_get_response(), None);
    }

    #[test]
    fn test_modem_simulator_constructor_real_clock() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let sim = ModemNetworkSimulator::new(tx);
        assert_eq!(sim.get_modem_count(), 0);
    }

    #[test]
    fn test_add_duplicate_modem() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut simulator = ModemNetworkSimulator::new(tx);
        let id = 1;
        let (_, sink1) = MockModemHandler::new(false);
        let (_, sink2) = MockModemHandler::new(false);

        simulator.new_modem(id, sink1, None, None, Quirks::default(), Vec::new()).unwrap();
        let res = simulator.new_modem(id, sink2, None, None, Quirks::default(), Vec::new());
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(matches!(err, ModemError::DuplicateModemId(1)));
    }

    #[test]
    fn test_tick_missing_modem() {
        let clock = Arc::new(MockClock::default());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut simulator = ModemNetworkSimulator::new_with_clock(clock.clone(), tx);
        let id = 1;
        let (_, sink) = MockModemHandler::new(false);
        simulator.new_modem(id, sink, None, None, Quirks::default(), Vec::new()).unwrap();

        // Schedule an event
        simulator.schedule_event(id, Duration::from_millis(10), ModemEvent::TestEvent);

        // Remove modem
        simulator.remove_modem(id);

        // Advance clock and tick
        clock.advance(Duration::from_millis(10));
        let (events, _) = simulator.tick();

        // Verify it doesn't panic and returns no events
        assert!(events.is_empty());
    }

    #[test]
    fn test_simulator_getters_and_debug() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut simulator = ModemNetworkSimulator::new(tx);
        let (_, sink1) = MockModemHandler::new(false);
        let (_, sink2) = MockModemHandler::new(false);
        simulator.new_modem(1, sink1, None, None, Quirks::default(), Vec::new()).unwrap();
        simulator.new_modem(2, sink2, None, None, Quirks::default(), Vec::new()).unwrap();

        let ids = simulator.get_modem_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
    }

    #[test]
    fn test_modem_network_interface_delegation() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut simulator = ModemNetworkSimulator::new(tx);

        // 1. Add modem
        let chip_id = 99;
        let (mut handler, sink) = MockModemHandler::new(false);
        let res = (&mut simulator as &mut dyn ModemNetworkInterface).add_modem(
            chip_id,
            sink,
            None,
            None,
            Quirks::default(),
            Vec::new(),
        );
        assert!(res.is_ok());
        simulator.set_voice_registration(chip_id, RegistrationStatus::RegisteredHome);

        let interface: &mut dyn ModemNetworkInterface = &mut simulator;

        // 2. Get modem info
        let info = interface.get_modem_info(chip_id).unwrap();
        assert_eq!(info.id, chip_id);
        assert!(info.calls.is_empty());
        assert!(!info.ringing);
        assert_eq!(info.sms_count, 0);

        // 3. Send data
        let res = interface.send_data(chip_id, b"AT\r\n");
        assert!(res.is_ok());

        let response = handler.wait_for_response();
        assert_eq!(response, b"OK\r\n");
        assert_eq!(handler.try_get_response(), None);

        // 4. Perform action
        let action =
            ModemAction::IncomingCall { target_id: ChipId(chip_id), number: "12345".to_string() };
        let _events = interface.perform_action(action).unwrap();
        let response = handler.wait_for_response();
        assert!(String::from_utf8_lossy(&response).contains("RING"));

        let info = interface.get_modem_info(chip_id).unwrap();
        assert!(info.ringing);

        let res = interface.on_timer(chip_id);
        assert!(res.is_ok());

        // 5. Remove modem
        let res = interface.remove_modem(chip_id);
        assert!(res.is_ok());

        let res = interface.get_modem_info(chip_id);
        assert!(res.is_err());
    }

    #[test]
    fn test_initiate_call_send_data_returns_ok() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut simulator = ModemNetworkSimulator::new(tx);
        let chip_id = 1;
        let (mut handler, sink) = MockModemHandler::new(false);
        simulator.add_modem(chip_id, sink, None, None, Quirks::default(), Vec::new()).unwrap();
        simulator.set_voice_registration(chip_id, RegistrationStatus::RegisteredHome);

        let interface: &mut dyn ModemNetworkInterface = &mut simulator;
        interface.send_data(chip_id, b"ATD12345;\r\n").unwrap();
        let response = handler.wait_for_response();
        assert_eq!(response, b"OK\r\n");
        assert_eq!(handler.try_get_response(), None);
    }

    #[test]
    fn test_incoming_sms_link_state_and_remove_modem() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut simulator = ModemNetworkSimulator::new(tx);

        let sender_id = 1;
        let recipient_id = 2;
        let (_handler1, sink1) = MockModemHandler::new(false);
        let (_handler2, sink2) = MockModemHandler::new(false);
        simulator.new_modem(sender_id, sink1, None, None, Quirks::default(), Vec::new()).unwrap();
        simulator
            .new_modem(recipient_id, sink2, None, None, Quirks::default(), Vec::new())
            .unwrap();

        let queue = simulator.incoming_sms.entry(recipient_id).or_default();
        assert!(queue.is_empty());

        let dummy_report = crate::pdu::StatusReport {
            message_reference: 1,
            recipient_address: vec![0x0B, 0x91, 0x51, 0x55, 0x25, 0x51, 0x55, 0xF4],
            scts_bcd: [0x26, 0x06, 0x15, 0x19, 0x30, 0x45, 0x00],
        };
        let immediate = queue.enqueue(
            b"+CMT: 1\r\n...".to_vec(),
            Some(PendingStatusReport { sender_id, report: dummy_report.clone() }),
        );
        assert_eq!(immediate, Some(b"+CMT: 1\r\n...".to_vec()));
        assert!(!queue.is_empty());

        let queued = queue.enqueue(
            b"+CMT: 2\r\n...".to_vec(),
            Some(PendingStatusReport { sender_id, report: dummy_report }),
        );
        assert_eq!(queued, None);

        // Removing the sender modem drops the status reports in Active and Pending,
        // while preserving the active in-flight link state and pending message.
        simulator.remove_modem(sender_id);

        let queue = simulator.incoming_sms.get_mut(&recipient_id).unwrap();
        assert_eq!(queue.link_state, IncomingSmsLinkState::Active(None));
        assert_eq!(queue.pending.len(), 1);
        assert!(queue.pending[0].status_report.is_none());

        // Acknowledge the first SMS
        let ack_sms = queue.acknowledge().unwrap();
        assert!(ack_sms.status_report.is_none());
        assert_eq!(ack_sms.next_cmt_response, Some(b"+CMT: 2\r\n...".to_vec()));

        // Acknowledge the second SMS
        let ack_sms = queue.acknowledge().unwrap();
        assert!(ack_sms.status_report.is_none());
        assert_eq!(ack_sms.next_cmt_response, None);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_simulator_sim_profile_management() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let clock = Arc::new(crate::time::MockClock::default());
        let mut simulator = ModemNetworkSimulator::new_with_clock(clock.clone(), tx);

        let chip_id = 1;
        let (mut handler, sink) = MockModemHandler::new(false);
        simulator.new_modem(chip_id, sink, None, None, Quirks::default(), Vec::new()).unwrap();

        // Check initial default profile
        let meta = simulator.get_sim_metadata(chip_id).expect("metadata should exist");
        assert_eq!(meta.imsi, "310260000000000");
        assert_eq!(meta.home_plmn.as_ref().map(crate::types::Plmn::as_str), Some("310260"));

        // Switch to Tel Alaska by numeric sim_type
        let _events = simulator
            .switch_sim_profile_by_type(chip_id, crate::profiles::SIM_TYPE_TEL_ALASKA)
            .unwrap();
        let resp = handler.wait_for_response();
        assert!(String::from_utf8_lossy(&resp).contains("+CPIN: READY"));

        // Advance clock and trigger timer to execute AttachNetwork
        clock.advance(Duration::from_millis(20));
        let _ = simulator.on_timer(chip_id);
        let _attach_urcs = handler.wait_for_response();

        let meta = simulator.get_sim_metadata(chip_id).unwrap();
        assert_eq!(meta.imsi, "311740123456789");
        assert_eq!(meta.home_plmn.as_ref().map(crate::types::Plmn::as_str), Some("311740"));

        // Attempting to insert a SIM when one is already present should fail
        let alaska_prof =
            crate::profiles::get_builtin_profile(crate::profiles::SIM_TYPE_TEL_ALASKA).unwrap();
        let insert_err = simulator.insert_sim(chip_id, alaska_prof).unwrap_err();
        assert!(matches!(insert_err, ModemError::InvalidConfig(_)));

        // Query operator via AT command: AT+COPS?
        let _ = simulator.send_at_command(chip_id, b"AT+COPS?\r\n");
        let mut cops_resp = String::new();
        cops_resp.push_str(&String::from_utf8_lossy(&handler.wait_for_response()));
        while let Some(line) = handler.try_get_response() {
            cops_resp.push_str(&String::from_utf8_lossy(&line));
        }
        assert!(cops_resp.contains("311740"));

        // Test eject_sim and reinsert_sim
        simulator.eject_sim(chip_id).unwrap();
        assert_eq!(simulator.get_sim_metadata(chip_id), None);
        let mut eject_resp = String::new();
        eject_resp.push_str(&String::from_utf8_lossy(&handler.wait_for_response()));
        while let Some(line) = handler.try_get_response() {
            eject_resp.push_str(&String::from_utf8_lossy(&line));
        }
        assert!(eject_resp.contains("+CPIN: ABSENT"));

        // Re-inserting the ejected card brings it back to ready
        simulator.reinsert_sim(chip_id).unwrap();
        let mut reinsert_resp = String::new();
        reinsert_resp.push_str(&String::from_utf8_lossy(&handler.wait_for_response()));
        while let Some(line) = handler.try_get_response() {
            reinsert_resp.push_str(&String::from_utf8_lossy(&line));
        }
        assert!(reinsert_resp.contains("+CPIN: READY"));
        assert_eq!(
            simulator
                .get_sim_metadata(chip_id)
                .unwrap()
                .home_plmn
                .as_ref()
                .map(crate::types::Plmn::as_str),
            Some("311740")
        );

        // Remove SIM (trashes the card)
        simulator.remove_sim(chip_id).unwrap();
        // Metadata must be None when SIM is absent
        assert_eq!(simulator.get_sim_metadata(chip_id), None);

        let mut remove_resp = String::new();
        remove_resp.push_str(&String::from_utf8_lossy(&handler.wait_for_response()));
        while let Some(line) = handler.try_get_response() {
            remove_resp.push_str(&String::from_utf8_lossy(&line));
        }
        assert!(remove_resp.contains("+CPIN: ABSENT"));

        // Re-inserting into an empty slot must fail
        let reinsert_err = simulator.reinsert_sim(chip_id).unwrap_err();
        assert!(matches!(reinsert_err, ModemError::InvalidConfig(_)));

        // Query SIM status: AT+CPIN? (returns error when SIM is absent)
        let _ = simulator.send_at_command(chip_id, b"AT+CPIN?\r\n");
        let mut query_resp = String::new();
        query_resp.push_str(&String::from_utf8_lossy(&handler.wait_for_response()));
        while let Some(line) = handler.try_get_response() {
            query_resp.push_str(&String::from_utf8_lossy(&line));
        }
        assert!(query_resp.contains("+CME ERROR") || query_resp.contains("ERROR"));

        // Insert CTS profile into empty slot
        let cts_prof = crate::profiles::get_builtin_profile(crate::profiles::SIM_TYPE_CTS).unwrap();
        simulator.insert_sim(chip_id, cts_prof).unwrap();
        let mut insert_resp = String::new();
        insert_resp.push_str(&String::from_utf8_lossy(&handler.wait_for_response()));
        while let Some(line) = handler.try_get_response() {
            insert_resp.push_str(&String::from_utf8_lossy(&line));
        }
        assert!(insert_resp.contains("+CPIN: READY"));

        let meta = simulator.get_sim_metadata(chip_id).unwrap();
        assert_eq!(meta.home_plmn.as_ref().map(crate::types::Plmn::as_str), Some("310260"));
    }
}
