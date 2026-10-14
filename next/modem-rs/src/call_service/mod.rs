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
use tracing::debug;

use crate::{
    CardState, RadioAdmission,
    types::{
        CallHoldAction, CallHoldParam, ClirMode, CmeError, CommandAction, DialArgs, DtmfArgs,
        ExecutionResult, HangupReason, ModemId, NumberPresentation, Parsable, PhoneNumber,
    },
};

/// Call service AT commands.
#[derive(Debug, PartialEq, Clone, CommandParser)]
pub enum CallCommand {
    #[command(tag = "ATD")]
    Dial(DialArgs),
    #[command(tag = "ATA")]
    Answer,
    #[command(tag = "ATH")]
    Hangup,
    #[command(tag = "AT+CHLD=")]
    CallHold(CallHoldParam),
    #[command(tag = "AT+CLCC")]
    QueryCurrentCalls,
    #[command(tag = "AT+CMUT=")]
    SetMute(bool),
    #[command(tag = "AT+CMUT?")]
    QueryMute,
    #[command(tag = "AT+VTS=")]
    SendDtmf(DtmfArgs),
    #[command(tag = "AT+WSOS=")]
    SetEmergencyMode(bool),
    #[command(tag = "AT+WSOS?")]
    QueryEmergencyMode,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CallDirection {
    Outgoing = 0,
    Incoming = 1,
}

/// Standard 3GPP TS 27.007 §7.18 (+CLCC) voice call states.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CallState {
    /// Connected voice call.
    Active = 0,
    /// Call on hold.
    Held = 1,
    /// Outgoing call dialing.
    Dialing = 2,
    /// Outgoing call ringing remote peer.
    Alerting = 3,
    /// Inbound call ringing.
    Incoming = 4,
    /// Inbound call waiting while another call is active.
    Waiting = 5,
}

impl CallState {
    pub fn is_outbound(self) -> bool {
        matches!(self, CallState::Dialing | CallState::Alerting)
    }

    fn is_inbound(self) -> bool {
        matches!(self, CallState::Incoming | CallState::Waiting)
    }

    pub fn release_reason(self) -> HangupReason {
        if self.is_inbound() { HangupReason::Busy } else { HangupReason::Normal }
    }

    fn is_foreground(self) -> bool {
        matches!(self, CallState::Active | CallState::Dialing | CallState::Alerting)
    }

    fn is_answerable(self) -> bool {
        matches!(self, CallState::Alerting | CallState::Incoming | CallState::Waiting)
    }

    /// Prioritizes answering inbound calls over resuming held calls during
    /// accept/swap operations.
    fn should_promote_to_active(self, has_inbound: bool) -> bool {
        self.is_inbound() || (self == CallState::Held && !has_inbound)
    }
}

/// Monotonic token pairing a scheduled `ModemEvent::CallRingTimeout` with the
/// specific incoming call attempt that armed it.
///
/// Because scheduled timer events are not cancelled when a call is answered or
/// hung up (and `CallId` slots are immediately reused), `handle_ring_timeout`
/// checks this token so a stale timer from an earlier call does not drop a
/// newer incoming call.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RingToken(u32);

impl RingToken {
    pub(crate) fn next(&mut self) -> Self {
        self.0 = self.0.wrapping_add(1);
        *self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallStatus {
    pub id: u8,
    pub state: CallState,
    pub direction: CallDirection,
    pub is_voice_mode: bool,
    pub is_multi_party: bool,
    pub number: Option<PhoneNumber>,
    pub peer_id: Option<ModemId>,
    pub number_presentation: NumberPresentation,
    pub(crate) ring_token: Option<RingToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallResponse {
    Ring,
    CurrentCalls(Vec<CallStatus>),
    Mute(bool),
    EmergencyMode(bool),
    WithActions(Vec<CommandAction>),
    Empty,
}

impl std::fmt::Display for CallResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallResponse::Ring => write!(f, "RING\r\n"),
            CallResponse::CurrentCalls(calls) => {
                for call in calls {
                    let presentation = match call.direction {
                        CallDirection::Outgoing => NumberPresentation::Allowed,
                        CallDirection::Incoming => call.number_presentation,
                    };
                    let val = presentation.format_number(call.number.as_ref());
                    write!(
                        f,
                        "+CLCC: {},{},{},{},{},{},{}\r\n",
                        call.id,
                        call.direction as u8,
                        call.state as u8,
                        if call.is_voice_mode { 0 } else { 1 },
                        call.is_multi_party as u8,
                        val.number,
                        val.toa,
                    )?;
                }
                Ok(())
            }
            CallResponse::Mute(mute) => {
                write!(f, "+CMUT: {}\r\n", if *mute { 1 } else { 0 })
            }
            CallResponse::EmergencyMode(mode) => {
                write!(f, "+WSOS: {}\r\n", if *mode { 1 } else { 0 })
            }
            CallResponse::WithActions(_) => Ok(()),
            CallResponse::Empty => Ok(()),
        }
    }
}

type CallResult = Result<Option<CallResponse>, ExecutionResult>;

// Holds all state related to the call service.
#[derive(Default)]
pub struct CallService {
    calls: Vec<CallStatus>,
    mute: bool,
    emergency_mode: bool,
    next_ring_token: RingToken,
}

impl CallService {
    pub(crate) fn calls(&self) -> &[CallStatus] {
        &self.calls
    }

    pub(crate) fn has_calls(&self) -> bool {
        !self.calls.is_empty()
    }

    pub(crate) fn clear_calls(&mut self) {
        self.calls.clear();
    }

    pub(crate) fn has_outbound_to_peer(&self, peer_id: ModemId) -> bool {
        self.calls.iter().any(|c| c.state.is_outbound() && c.peer_id == Some(peer_id))
    }

    pub(crate) fn set_outbound_peer_id(&mut self, peer_id: Option<ModemId>) {
        if let Some(call) = self.calls.iter_mut().find(|c| c.state.is_outbound()) {
            call.peer_id = peer_id;
        }
    }

    pub(crate) fn fail_outbound_call(&mut self) -> bool {
        if let Some(pos) = self.calls.iter().position(|c| c.state.is_outbound()) {
            self.calls.remove(pos);
            true
        } else {
            false
        }
    }

    fn has_call_in_state(&self, state: CallState) -> bool {
        self.calls.iter().any(|c| c.state == state)
    }

    fn has_call(&self, id: u8) -> bool {
        self.calls.iter().any(|c| c.id == id)
    }

    // Allocates a stable 1-indexed ID for AT+CLCC/CHLD; N is small (typically <= 7)
    // due to network limits.
    fn allocate_id(&self) -> Option<u8> {
        (1..=u8::MAX).find(|&id| !self.has_call(id))
    }

    fn add_call(
        &mut self,
        state: CallState,
        direction: CallDirection,
        number: Option<PhoneNumber>,
        number_presentation: NumberPresentation,
        peer_id: Option<ModemId>,
    ) -> Option<u8> {
        self.add_call_with_token(state, direction, number, number_presentation, peer_id, None)
    }

    fn add_call_with_token(
        &mut self,
        state: CallState,
        direction: CallDirection,
        number: Option<PhoneNumber>,
        number_presentation: NumberPresentation,
        peer_id: Option<ModemId>,
        ring_token: Option<RingToken>,
    ) -> Option<u8> {
        let id = self.allocate_id()?;
        self.calls.push(CallStatus {
            id,
            state,
            direction,
            is_voice_mode: true,
            is_multi_party: false,
            number,
            peer_id,
            number_presentation,
            ring_token,
        });
        Some(id)
    }

    fn remove_call(&mut self, id: u8) {
        if let Some(pos) = self.calls.iter().position(|c| c.id == id) {
            self.calls.remove(pos);
        }
    }

    /// Returns true if this call is currently part of an active or held
    /// multi-party conference with >= 2 remote participants in the same
    /// state, per 3GPP TS 22.084.
    fn is_multiparty_call(&self, call: &CallStatus) -> bool {
        call.is_multi_party
            && self.calls.iter().filter(|c| c.state == call.state && c.is_multi_party).count() >= 2
    }

    // --- Helper methods for external services ---

    pub fn receive_hangup(&mut self) {
        let has_foreground = self.has_foreground();
        let has_inbound = self.has_inbound();

        self.calls.retain(|call| {
            let should_drop = if has_foreground {
                call.state.is_foreground()
            } else if has_inbound {
                call.state.is_inbound()
            } else {
                call.state == CallState::Held
            };
            !should_drop
        });
    }

    pub fn receive_hangup_from_peer_id(&mut self, peer_id: ModemId) -> bool {
        let prev_len = self.calls.len();
        self.calls.retain(|c| c.peer_id != Some(peer_id));
        self.calls.len() < prev_len
    }

    pub fn hangup_all(&mut self, id: ModemId) -> Vec<CommandAction> {
        let actions: Vec<CommandAction> = self
            .calls
            .iter()
            .filter_map(|call| {
                call.peer_id.map(|peer_id| CommandAction::HangupCall {
                    initiator: id,
                    target_peer: peer_id,
                    reason: call.state.release_reason(),
                })
            })
            .collect();
        self.calls.clear();
        actions
    }

    pub fn handle_ring_timeout(&mut self, id: ModemId, ring_token: RingToken) -> CallResult {
        let mut actions = Vec::new();
        self.calls.retain(|call| {
            if call.state.is_inbound() && call.ring_token == Some(ring_token) {
                if let Some(peer_id) = call.peer_id {
                    actions.push(CommandAction::HangupCall {
                        initiator: id,
                        target_peer: peer_id,
                        reason: HangupReason::NoAnswer,
                    });
                }
                false
            } else {
                true
            }
        });
        Ok(Some(CallResponse::WithActions(actions)))
    }

    pub fn ring(
        &mut self,
        number: Option<PhoneNumber>,
        number_presentation: NumberPresentation,
        peer_id: Option<ModemId>,
    ) -> Result<RingToken, ExecutionResult> {
        let initial_state =
            if self.has_active() { CallState::Waiting } else { CallState::Incoming };
        let ring_token = self.next_ring_token.next();
        if self
            .add_call_with_token(
                initial_state,
                CallDirection::Incoming,
                number,
                number_presentation,
                peer_id,
                Some(ring_token),
            )
            .is_none()
        {
            return Err(ExecutionResult::error());
        }
        Ok(ring_token)
    }

    pub fn receive_peer_hold(&mut self, peer_id: ModemId, on_hold: bool) {
        debug!(
            "[CallService] Receiving {} from {:?}",
            if on_hold { "hold" } else { "resume" },
            peer_id
        );
        if let Some(call) = self.calls.iter_mut().find(|c| c.peer_id == Some(peer_id)) {
            call.state = if on_hold { CallState::Held } else { CallState::Active };
        }
    }

    pub fn trigger_remote_hold(&mut self, on_hold: bool) {
        let target_state = if on_hold { CallState::Active } else { CallState::Held };
        let new_state = if on_hold { CallState::Held } else { CallState::Active };
        // Prioritize independent calls created directly via CLI/RPC (peer_id is None)
        if let Some(call) =
            self.calls.iter_mut().find(|c| c.state == target_state && c.peer_id.is_none())
        {
            call.state = new_state;
        } else if let Some(call) = self.calls.iter_mut().find(|c| c.state == target_state) {
            call.state = new_state;
        }
    }

    pub(crate) fn remote_answer(&mut self) -> bool {
        if let Some(call) = self.calls.iter_mut().find(|c| c.state.is_outbound()) {
            call.state = CallState::Active;
            return true;
        }
        false
    }

    fn is_idle(&self) -> bool {
        self.calls.is_empty()
    }

    pub fn has_alerting(&self) -> bool {
        self.has_call_in_state(CallState::Alerting)
    }

    pub(crate) fn has_outbound(&self) -> bool {
        self.calls.iter().any(|c| c.state.is_outbound())
    }

    pub(crate) fn has_inbound(&self) -> bool {
        self.calls.iter().any(|c| c.state.is_inbound())
    }

    fn has_foreground(&self) -> bool {
        self.calls.iter().any(|c| c.state.is_foreground())
    }

    pub(crate) fn has_active(&self) -> bool {
        self.has_call_in_state(CallState::Active)
    }

    fn has_held(&self) -> bool {
        self.has_call_in_state(CallState::Held)
    }

    pub fn has_incoming(&self) -> bool {
        self.has_call_in_state(CallState::Incoming)
    }

    pub fn connect(&mut self, _modem_id: ModemId, peer_id: ModemId) -> Option<Vec<u8>> {
        debug!("[CallService] Connecting to peer {peer_id}");
        debug!("[CallService] Calls before connect: {:?}", self.calls);

        if let Some(call) = self.calls.iter_mut().find(|c| c.state.is_outbound()) {
            call.state = CallState::Active;
            call.peer_id = Some(peer_id);
            debug!("[CallService] Connected as caller.");
            return Some(b"RING\r\n".to_vec());
        }

        if let Some(call) = self.calls.iter_mut().find(|c| {
            c.state == CallState::Active && (c.peer_id.is_none() || c.peer_id == Some(peer_id))
        }) {
            call.peer_id = Some(peer_id);
            debug!("[CallService] Connected as callee (peer_id set). Calls: {:?}", self.calls);
            return None;
        }

        debug!("[CallService] Connect failed. Calls: {:?}", self.calls);
        None
    }

    // --- Command handlers ---

    fn handle_dial(
        &mut self,
        args: DialArgs,
        id: ModemId,
        clir_default: ClirMode,
        sim_service: &impl CardState,
        radio_service: &impl RadioAdmission,
    ) -> ExecutionResult {
        debug!("[CallService] Dialing number: {}", args.number.as_str());
        let result = self.handle_voice_dial(id, args, clir_default, sim_service, radio_service);
        result.into()
    }

    fn handle_voice_dial(
        &mut self,
        id: ModemId,
        args: DialArgs,
        clir_default: ClirMode,
        sim_service: &impl CardState,
        radio_service: &impl RadioAdmission,
    ) -> CallResult {
        let is_emergency = args.is_emergency;

        if self.emergency_mode && !is_emergency {
            return Err(ExecutionResult::cme_error(CmeError::NetworkNotAllowedEmergencyCallsOnly));
        }

        // Locked SIMs attach in Emergency-only registration (CME 32), so SIM gating runs
        // first to report CME 10/11/12 (SIM absent/PIN/PUK) on non-emergency dials.
        sim_service.validate_call(&args.number, is_emergency)?;
        radio_service.can_originate_voice_call(is_emergency)?;

        debug!("[CallService] Calls before dial: {:?}", self.calls);
        if self.has_outbound() {
            return Err(ExecutionResult::cme_error(CmeError::OperationNotAllowed));
        }

        let mut actions = Vec::new();
        for call in self.calls.iter_mut() {
            if call.state == CallState::Active {
                call.state = CallState::Held;
                if let Some(peer_id) = call.peer_id {
                    actions.push(CommandAction::HoldCall { holder: id, target: peer_id });
                }
            }
        }

        let call_clir =
            if args.clir == ClirMode::SubscriptionDefault { clir_default } else { args.clir };

        let number_presentation = match call_clir {
            ClirMode::Invocation => NumberPresentation::Restricted,
            _ => NumberPresentation::Allowed,
        };

        if self
            .add_call(
                CallState::Dialing,
                CallDirection::Outgoing,
                Some(args.number.clone()),
                number_presentation,
                None,
            )
            .is_none()
        {
            return Err(ExecutionResult::error());
        }
        debug!("[CallService] Calls after dial: {:?}", self.calls);

        actions.push(CommandAction::InitiateCall(DialArgs {
            number: args.number,
            clir: call_clir,
            is_emergency,
        }));

        Ok(Some(CallResponse::WithActions(actions)))
    }

    fn handle_answer(&mut self, id: ModemId) -> CallResult {
        debug!("[CallService modem={id}] Answering call");
        debug!("[CallService modem={id}] Calls before answer: {:?}", self.calls);
        if let Some(answerable_id) =
            self.calls.iter().find(|c| c.state.is_answerable()).map(|c| c.id)
        {
            let mut actions = Vec::new();
            // Under 3GPP TS 22.030 / 24.083, answering an incoming/waiting call
            // automatically places any existing active call on hold.
            for call in self.calls.iter_mut() {
                if call.id == answerable_id {
                    call.state = CallState::Active;
                } else if call.state == CallState::Active {
                    call.state = CallState::Held;
                    if let Some(peer_id) = call.peer_id {
                        actions.push(CommandAction::HoldCall { holder: id, target: peer_id });
                    }
                }
            }

            actions.push(CommandAction::AnswerCall(id));
            debug!("[CallService modem={id}] Calls after answer: {:?}", self.calls);
            return Ok(Some(CallResponse::WithActions(actions)));
        }
        Err(ExecutionResult::error())
    }

    fn handle_hangup(&mut self, id: ModemId) -> CallResult {
        if self.is_idle() {
            return Ok(None);
        }

        // 3GPP TS 22.030 priority: inbound/waiting (UDUB) -> foreground -> held
        let has_inbound = self.has_inbound();
        let has_foreground = self.has_foreground();

        let mut actions = Vec::new();
        self.calls.retain(|call| {
            let should_drop = if has_inbound {
                call.state.is_inbound()
            } else if has_foreground {
                call.state.is_foreground()
            } else {
                call.state == CallState::Held
            };

            if should_drop {
                if let Some(peer_id) = call.peer_id {
                    actions.push(CommandAction::HangupCall {
                        initiator: id,
                        target_peer: peer_id,
                        reason: call.state.release_reason(),
                    });
                }
                false
            } else {
                true
            }
        });

        Ok(Some(CallResponse::WithActions(actions)))
    }

    fn handle_call_hold(&mut self, chld: CallHoldParam, id: ModemId) -> CallResult {
        debug!("[CallService] Call hold operation: {chld:?}");
        debug!("[CallService] Calls before hold op: {:?}", self.calls);

        let (op, index) = (chld.op, chld.call_id);

        // Releasing when no calls exist is a no-op that succeeds with OK.
        if self.calls.is_empty()
            && (op == CallHoldAction::ReleaseHeld || op == CallHoldAction::ReleaseAndAccept)
        {
            return Ok(None);
        }

        // Validate index if specified for operations targeting a specific call
        if let Some(idx) = index
            && !self.has_call(idx)
        {
            return Err(ExecutionResult::error());
        }

        let actions = match op {
            CallHoldAction::ReleaseHeld => {
                let mut actions = Vec::new();
                let has_inbound = self.has_inbound();
                self.calls.retain(|call| {
                    let should_drop = if has_inbound {
                        call.state.is_inbound()
                    } else {
                        call.state == CallState::Held
                    };
                    if should_drop {
                        if let Some(peer_id) = call.peer_id {
                            actions.push(CommandAction::HangupCall {
                                initiator: id,
                                target_peer: peer_id,
                                reason: call.state.release_reason(),
                            });
                        }
                        false
                    } else {
                        true
                    }
                });
                actions
            }
            CallHoldAction::ReleaseAndAccept => {
                let mut actions = Vec::new();
                if let Some(idx) = index {
                    if let Some(call) = self.calls.iter().find(|c| c.id == idx)
                        && let Some(peer_id) = call.peer_id
                    {
                        actions.push(CommandAction::HangupCall {
                            initiator: id,
                            target_peer: peer_id,
                            reason: call.state.release_reason(),
                        });
                    }
                    self.remove_call(idx);
                } else {
                    let has_inbound = self.has_inbound();
                    self.calls.retain(|c| {
                        if c.state.is_foreground() {
                            if let Some(peer) = c.peer_id {
                                actions.push(CommandAction::HangupCall {
                                    initiator: id,
                                    target_peer: peer,
                                    reason: c.state.release_reason(),
                                });
                            }
                            false
                        } else {
                            true
                        }
                    });
                    for call in self.calls.iter_mut() {
                        let was_inbound = call.state.is_inbound();
                        if call.state.should_promote_to_active(has_inbound) {
                            if was_inbound {
                                actions.push(CommandAction::AnswerCall(id));
                            } else if let Some(peer_id) = call.peer_id {
                                actions.push(CommandAction::ResumeCall {
                                    resumer: id,
                                    target: peer_id,
                                });
                            }
                            call.state = CallState::Active;
                        }
                    }
                }
                actions
            }
            CallHoldAction::HoldAndAccept => {
                let mut actions = Vec::new();
                if let Some(idx) = index {
                    for call in self.calls.iter_mut() {
                        if call.id == idx {
                            if call.state == CallState::Held
                                && let Some(peer_id) = call.peer_id
                            {
                                actions.push(CommandAction::ResumeCall {
                                    resumer: id,
                                    target: peer_id,
                                });
                            } else if call.state.is_inbound() {
                                actions.push(CommandAction::AnswerCall(id));
                            }
                            call.state = CallState::Active;
                            call.is_multi_party = false;
                        } else if call.state == CallState::Active {
                            if let Some(peer_id) = call.peer_id {
                                actions
                                    .push(CommandAction::HoldCall { holder: id, target: peer_id });
                            }
                            call.state = CallState::Held;
                        }
                    }
                } else {
                    let has_inbound = self.has_inbound();
                    for call in self.calls.iter_mut() {
                        if call.state == CallState::Active {
                            if let Some(peer_id) = call.peer_id {
                                actions
                                    .push(CommandAction::HoldCall { holder: id, target: peer_id });
                            }
                            call.state = CallState::Held;
                        } else {
                            let was_inbound = call.state.is_inbound();
                            if call.state.should_promote_to_active(has_inbound) {
                                if was_inbound {
                                    actions.push(CommandAction::AnswerCall(id));
                                } else if let Some(peer_id) = call.peer_id {
                                    actions.push(CommandAction::ResumeCall {
                                        resumer: id,
                                        target: peer_id,
                                    });
                                }
                                call.state = CallState::Active;
                            }
                        }
                    }
                }
                actions
            }
            CallHoldAction::Conference => {
                if !self.has_active() || !self.has_held() {
                    return Err(ExecutionResult::error());
                }
                let mut actions = Vec::new();
                for call in self.calls.iter_mut() {
                    match call.state {
                        CallState::Held => {
                            call.state = CallState::Active;
                            call.is_multi_party = true;
                            if let Some(peer_id) = call.peer_id {
                                actions.push(CommandAction::ResumeCall {
                                    resumer: id,
                                    target: peer_id,
                                });
                            }
                        }
                        CallState::Active => {
                            call.is_multi_party = true;
                        }
                        _ => {}
                    }
                }
                actions
            }
            CallHoldAction::Transfer => {
                // ECT: Connect remote parties and disconnect us.
                // TODO: Support true ECT by bridging peers in the simulator, but it is
                // currently unsupported in the Android Emulator RIL.
                // For now, we hang up to avoid leaking state.
                return self.handle_hangup(id);
            }
            CallHoldAction::UserToUserSignaling => {
                return Err(ExecutionResult::error());
            }
        };
        debug!("[CallService] Calls after hold op: {:?}", self.calls);
        Ok(Some(CallResponse::WithActions(actions)))
    }

    fn handle_query_current_calls(&self) -> CallResult {
        if self.calls.is_empty() {
            Ok(None)
        } else {
            let calls = self
                .calls
                .iter()
                .cloned()
                .map(|mut c| {
                    c.is_multi_party = self.is_multiparty_call(&c);
                    c
                })
                .collect();
            Ok(Some(CallResponse::CurrentCalls(calls)))
        }
    }

    fn handle_set_mute(&mut self, mute: bool) -> CallResult {
        self.mute = mute;
        Ok(None)
    }

    fn handle_query_mute(&self) -> CallResult {
        Ok(Some(CallResponse::Mute(self.mute)))
    }

    fn handle_send_dtmf(&self, dtmf: &DtmfArgs) -> CallResult {
        debug!("[CallService] Send DTMF: {}", dtmf.tone);
        Ok(None)
    }

    pub fn set_emergency_mode(&mut self, mode: bool) -> CallResponse {
        self.emergency_mode = mode;
        CallResponse::EmergencyMode(mode)
    }

    fn handle_query_emergency_mode(&self) -> CallResult {
        Ok(Some(CallResponse::EmergencyMode(self.emergency_mode)))
    }

    pub(crate) fn execute(
        &mut self,
        command: &CallCommand,
        id: ModemId,
        clir_default: ClirMode,
        sim_service: &impl CardState,
        radio_service: &impl RadioAdmission,
    ) -> ExecutionResult {
        let res = match command {
            CallCommand::Dial(args) => {
                return self.handle_dial(
                    args.clone(),
                    id,
                    clir_default,
                    sim_service,
                    radio_service,
                );
            }
            CallCommand::Answer => self.handle_answer(id),
            CallCommand::Hangup => self.handle_hangup(id),
            CallCommand::CallHold(op) => self.handle_call_hold(*op, id),
            CallCommand::QueryCurrentCalls => self.handle_query_current_calls(),
            CallCommand::SetMute(mute) => self.handle_set_mute(*mute),
            CallCommand::QueryMute => self.handle_query_mute(),
            CallCommand::SendDtmf(dtmf) => self.handle_send_dtmf(dtmf),
            CallCommand::SetEmergencyMode(mode) => {
                self.set_emergency_mode(*mode);
                Ok(None)
            }
            CallCommand::QueryEmergencyMode => self.handle_query_emergency_mode(),
        };
        res.into()
    }
}

#[cfg(test)]
mod tests;
