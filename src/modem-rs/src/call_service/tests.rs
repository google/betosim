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

use super::{CallDirection, CallResponse, CallService, CallState};
use crate::{
    tests::test_utils::{MockCardState, MockRadioAdmission},
    types::{ClirMode, CommandAction, DialArgs, HangupReason, NumberPresentation, PhoneNumber},
};

#[test]
fn test_clcc_not_available() {
    let mut service = CallService::default();
    service.add_call(
        CallState::Incoming,
        CallDirection::Incoming,
        Some(PhoneNumber::new_for_test("123456")),
        NumberPresentation::NotAvailable,
        None,
    );

    let calls_res = service.handle_query_current_calls().unwrap().unwrap();
    let formatted = calls_res.to_string();

    assert_eq!(formatted, "+CLCC: 1,1,4,0,0,,129\r\n");
}

#[test]
fn test_ath_idle_returns_ok() {
    let mut service = CallService::default();
    let id = 1;
    let res = service.handle_hangup(id);
    assert!(res.is_ok());
    assert_eq!(res.unwrap(), None);
}

#[test]
fn test_ath_rejects_waiting_preserves_active() {
    let mut service = CallService::default();
    let id = 1;
    let peer1 = 10;
    let peer2 = 20;

    // Call 1: Active
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("111")),
        NumberPresentation::Allowed,
        Some(peer1),
    );
    // Call 2: Waiting
    service.add_call(
        CallState::Waiting,
        CallDirection::Incoming,
        Some(PhoneNumber::new_for_test("222")),
        NumberPresentation::Allowed,
        Some(peer2),
    );

    let res = service.handle_hangup(id).unwrap().unwrap();
    match res {
        super::CallResponse::WithActions(actions) => {
            assert_eq!(
                actions,
                vec![crate::types::CommandAction::HangupCall {
                    initiator: id,
                    target_peer: peer2,
                    reason: HangupReason::Busy,
                }]
            );
        }
        other => panic!("Unexpected response: {other:?}"),
    }

    // Active call 1 must still be present and Active
    assert_eq!(service.calls.len(), 1);
    assert_eq!(service.calls[0].state, CallState::Active);
    assert_eq!(service.calls[0].peer_id, Some(peer1));
}

#[test]
fn test_ath_drops_active_preserves_held() {
    let mut service = CallService::default();
    let id = 1;
    let peer1 = 10;
    let peer2 = 20;

    // Call 1: Held
    service.add_call(
        CallState::Held,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("111")),
        NumberPresentation::Allowed,
        Some(peer1),
    );
    // Call 2: Active
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("222")),
        NumberPresentation::Allowed,
        Some(peer2),
    );

    let res = service.handle_hangup(id).unwrap().unwrap();
    match res {
        super::CallResponse::WithActions(actions) => {
            assert_eq!(
                actions,
                vec![crate::types::CommandAction::HangupCall {
                    initiator: id,
                    target_peer: peer2,
                    reason: HangupReason::Normal,
                }]
            );
        }
        other => panic!("Unexpected response: {other:?}"),
    }

    // Held call 1 must remain intact
    assert_eq!(service.calls.len(), 1);
    assert_eq!(service.calls[0].state, CallState::Held);
    assert_eq!(service.calls[0].peer_id, Some(peer1));
}

#[test]
fn test_ata_auto_holds_active_call() {
    let mut service = CallService::default();
    let id = 1;
    let peer1 = 10;
    let peer2 = 20;

    // Call 1: Active
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("111")),
        NumberPresentation::Allowed,
        Some(peer1),
    );
    // Call 2: Waiting
    service.add_call(
        CallState::Waiting,
        CallDirection::Incoming,
        Some(PhoneNumber::new_for_test("222")),
        NumberPresentation::Allowed,
        Some(peer2),
    );

    let res = service.handle_answer(id).unwrap().unwrap();
    match res {
        super::CallResponse::WithActions(actions) => {
            assert_eq!(
                actions,
                vec![
                    crate::types::CommandAction::HoldCall { holder: id, target: peer1 },
                    crate::types::CommandAction::AnswerCall(id),
                ]
            );
        }
        other => panic!("Unexpected response: {other:?}"),
    }

    assert_eq!(service.calls.len(), 2);
    // Call 1 transitioned to Held
    assert_eq!(service.calls[0].state, CallState::Held);
    // Call 2 transitioned to Active
    assert_eq!(service.calls[1].state, CallState::Active);
}

#[test]
fn test_chld_0_rejects_waiting_preserves_held() {
    let mut service = CallService::default();
    let id = 1;
    let peer_held = 10;
    let peer_wait = 20;
    let peer_act = 30;

    // Call 1: Held
    service.add_call(
        CallState::Held,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("111")),
        NumberPresentation::Allowed,
        Some(peer_held),
    );
    // Call 2: Waiting
    service.add_call(
        CallState::Waiting,
        CallDirection::Incoming,
        Some(PhoneNumber::new_for_test("222")),
        NumberPresentation::Allowed,
        Some(peer_wait),
    );
    // Call 3: Active
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("333")),
        NumberPresentation::Allowed,
        Some(peer_act),
    );

    let res = service
        .handle_call_hold(
            crate::types::CallHoldParam {
                op: crate::types::CallHoldAction::ReleaseHeld,
                call_id: None,
            },
            id,
        )
        .unwrap()
        .unwrap();

    match res {
        super::CallResponse::WithActions(actions) => {
            // Under 3GPP TS 27.007 § 7.13 / TS 22.030 § 4.5.5.1, CHLD=0
            // sets UDUB on waiting call, rejecting ONLY the waiting call.
            assert_eq!(actions.len(), 1);
            assert_eq!(
                actions[0],
                crate::types::CommandAction::HangupCall {
                    initiator: id,
                    target_peer: peer_wait,
                    reason: HangupReason::Busy,
                }
            );
        }
        other => panic!("Unexpected response: {other:?}"),
    }

    // Active call 3 and Held call 1 should remain intact
    assert_eq!(service.calls.len(), 2);
    assert_eq!(service.calls[0].state, CallState::Held);
    assert_eq!(service.calls[0].peer_id, Some(peer_held));
    assert_eq!(service.calls[1].state, CallState::Active);
    assert_eq!(service.calls[1].peer_id, Some(peer_act));
}

#[test]
fn test_chld_0_releases_held_when_no_waiting() {
    let mut service = CallService::default();
    let id = 1;
    let peer_held1 = 10;
    let peer_held2 = 20;
    let peer_act = 30;

    service.add_call(
        CallState::Held,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("111")),
        NumberPresentation::Allowed,
        Some(peer_held1),
    );
    service.add_call(
        CallState::Held,
        CallDirection::Incoming,
        Some(PhoneNumber::new_for_test("222")),
        NumberPresentation::Allowed,
        Some(peer_held2),
    );
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("333")),
        NumberPresentation::Allowed,
        Some(peer_act),
    );

    let res = service
        .handle_call_hold(
            crate::types::CallHoldParam {
                op: crate::types::CallHoldAction::ReleaseHeld,
                call_id: None,
            },
            id,
        )
        .unwrap()
        .unwrap();

    match res {
        super::CallResponse::WithActions(actions) => {
            assert_eq!(actions.len(), 2);
            assert!(actions.contains(&crate::types::CommandAction::HangupCall {
                initiator: id,
                target_peer: peer_held1,
                reason: HangupReason::Normal,
            }));
            assert!(actions.contains(&crate::types::CommandAction::HangupCall {
                initiator: id,
                target_peer: peer_held2,
                reason: HangupReason::Normal,
            }));
        }
        other => panic!("Unexpected response: {other:?}"),
    }

    assert_eq!(service.calls.len(), 1);
    assert_eq!(service.calls[0].state, CallState::Active);
    assert_eq!(service.calls[0].peer_id, Some(peer_act));
}

#[test]
fn test_ath_teardown_conference_drops_all_active_legs() {
    let mut service = CallService::default();
    let id = 1;
    let peer1 = 10;
    let peer2 = 20;
    let peer_held = 30;

    service.add_call(
        CallState::Held,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("111")),
        NumberPresentation::Allowed,
        Some(peer_held),
    );
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("222")),
        NumberPresentation::Allowed,
        Some(peer1),
    );
    service.calls[1].is_multi_party = true;
    service.add_call(
        CallState::Active,
        CallDirection::Incoming,
        Some(PhoneNumber::new_for_test("333")),
        NumberPresentation::Allowed,
        Some(peer2),
    );
    service.calls[2].is_multi_party = true;

    let res = service.handle_hangup(id).unwrap().unwrap();
    match res {
        super::CallResponse::WithActions(actions) => {
            assert_eq!(actions.len(), 2);
            assert!(actions.contains(&crate::types::CommandAction::HangupCall {
                initiator: id,
                target_peer: peer1,
                reason: HangupReason::Normal,
            }));
            assert!(actions.contains(&crate::types::CommandAction::HangupCall {
                initiator: id,
                target_peer: peer2,
                reason: HangupReason::Normal,
            }));
        }
        other => panic!("Unexpected response: {other:?}"),
    }

    // Both active conference legs dropped, held call remains
    assert_eq!(service.calls.len(), 1);
    assert_eq!(service.calls[0].state, CallState::Held);
    assert_eq!(service.calls[0].peer_id, Some(peer_held));
}

#[test]
fn test_receive_hangup_drops_foreground_preserves_held() {
    let mut service = CallService::default();
    let peer1 = 10;
    let peer2 = 20;

    // Call 1: Held
    service.add_call(
        CallState::Held,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("111")),
        NumberPresentation::Allowed,
        Some(peer1),
    );
    // Call 2: Active
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("222")),
        NumberPresentation::Allowed,
        Some(peer2),
    );

    service.receive_hangup();

    // Active call was dropped; Held call remains intact
    assert_eq!(service.calls.len(), 1);
    assert_eq!(service.calls[0].state, CallState::Held);
    assert_eq!(service.calls[0].peer_id, Some(peer1));
}

#[test]
fn test_receive_hangup_drops_all_conference_legs() {
    let mut service = CallService::default();
    let peer_held = 10;
    let peer1 = 20;
    let peer2 = 30;

    service.add_call(
        CallState::Held,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("111")),
        NumberPresentation::Allowed,
        Some(peer_held),
    );
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        Some(PhoneNumber::new_for_test("222")),
        NumberPresentation::Allowed,
        Some(peer1),
    );
    service.calls[1].is_multi_party = true;
    service.add_call(
        CallState::Active,
        CallDirection::Incoming,
        Some(PhoneNumber::new_for_test("333")),
        NumberPresentation::Allowed,
        Some(peer2),
    );
    service.calls[2].is_multi_party = true;

    service.receive_hangup();

    // Both active conference legs dropped; Held call remains intact
    assert_eq!(service.calls.len(), 1);
    assert_eq!(service.calls[0].state, CallState::Held);
    assert_eq!(service.calls[0].peer_id, Some(peer_held));
}

#[test]
fn test_handle_voice_dial_resolves_clir() {
    let mut service = CallService::default();
    let sim = MockCardState {
        is_present: true,
        is_ready: true,
        is_fdn_allowed: true,
        gating_error: None,
    };
    let radio = MockRadioAdmission::default();
    let dial_args = DialArgs {
        number: PhoneNumber::new_for_test("12345"),
        clir: ClirMode::Suppression,
        is_emergency: false,
    };
    let res = service.handle_voice_dial(1, dial_args, ClirMode::Invocation, &sim, &radio);
    match res {
        Ok(Some(CallResponse::WithActions(actions))) => {
            let init = actions
                .into_iter()
                .find_map(|a| match a {
                    CommandAction::InitiateCall(args) => Some(args),
                    _ => None,
                })
                .expect("Expected InitiateCall action");
            assert_eq!(init.clir, ClirMode::Suppression);
        }
        other => panic!("Expected Ok(Some(CallResponse::WithActions)), got {other:?}"),
    }

    let mut service = CallService::default();
    let dial_args_default = DialArgs {
        number: PhoneNumber::new_for_test("12345"),
        clir: ClirMode::SubscriptionDefault,
        is_emergency: false,
    };
    let res = service.handle_voice_dial(1, dial_args_default, ClirMode::Invocation, &sim, &radio);
    match res {
        Ok(Some(CallResponse::WithActions(actions))) => {
            let init = actions
                .into_iter()
                .find_map(|a| match a {
                    CommandAction::InitiateCall(args) => Some(args),
                    _ => None,
                })
                .expect("Expected InitiateCall action");
            assert_eq!(init.clir, ClirMode::Invocation);
        }
        other => panic!("Expected Ok(Some(CallResponse::WithActions)), got {other:?}"),
    }
}

#[test]
fn test_call_status_hangup_action() {
    let mut service = CallService::default();
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        None,
        NumberPresentation::Allowed,
        Some(2),
    );
    let call = &mut service.calls[0];
    assert_eq!(
        call.hangup_action(1),
        Some(CommandAction::HangupCall {
            initiator: 1,
            target_peer: 2,
            reason: HangupReason::Normal,
        })
    );

    call.state = CallState::Waiting;
    assert_eq!(
        call.hangup_action(1),
        Some(CommandAction::HangupCall {
            initiator: 1,
            target_peer: 2,
            reason: HangupReason::Busy,
        })
    );

    call.peer_id = None;
    assert_eq!(call.hangup_action(1), None);
}

#[test]
fn test_call_status_promote_to_active() {
    let mut service = CallService::default();
    service.add_call(
        CallState::Waiting,
        CallDirection::Incoming,
        None,
        NumberPresentation::Allowed,
        Some(2),
    );
    let call = &mut service.calls[0];

    assert_eq!(call.promote_to_active(true, 1), Some(CommandAction::AnswerCall(1)));
    assert_eq!(call.state, CallState::Active);

    call.state = CallState::Held;
    call.peer_id = Some(2);
    assert_eq!(
        call.promote_to_active(false, 1),
        Some(CommandAction::ResumeCall { resumer: 1, target: 2 })
    );
    assert_eq!(call.state, CallState::Active);

    call.state = CallState::Held;
    call.peer_id = None;
    assert_eq!(call.promote_to_active(false, 1), None);
    assert_eq!(call.state, CallState::Active);

    call.state = CallState::Held;
    assert_eq!(call.promote_to_active(true, 1), None);
    assert_eq!(call.state, CallState::Held);

    call.state = CallState::Active;
    assert_eq!(call.promote_to_active(false, 1), None);
    assert_eq!(call.state, CallState::Active);

    call.state = CallState::Dialing;
    assert_eq!(call.promote_to_active(false, 1), None);
    assert_eq!(call.state, CallState::Dialing);
}

#[test]
fn test_call_status_hold_if_active() {
    let mut service = CallService::default();
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        None,
        NumberPresentation::Allowed,
        Some(2),
    );
    let call = &mut service.calls[0];
    assert_eq!(call.hold_if_active(1), Some(CommandAction::HoldCall { holder: 1, target: 2 }));
    assert_eq!(call.state, CallState::Held);

    call.state = CallState::Active;
    call.peer_id = None;
    assert_eq!(call.hold_if_active(1), None);
    assert_eq!(call.state, CallState::Held);

    call.state = CallState::Held;
    assert_eq!(call.hold_if_active(1), None);
    assert_eq!(call.state, CallState::Held);

    call.state = CallState::Waiting;
    assert_eq!(call.hold_if_active(1), None);
    assert_eq!(call.state, CallState::Waiting);

    call.state = CallState::Dialing;
    assert_eq!(call.hold_if_active(1), None);
    assert_eq!(call.state, CallState::Dialing);
}

#[test]
fn test_call_service_hangup_matching() {
    let mut service = CallService::default();
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        None,
        NumberPresentation::Allowed,
        Some(2),
    );
    service.add_call(
        CallState::Waiting,
        CallDirection::Incoming,
        None,
        NumberPresentation::Allowed,
        None,
    );
    service.add_call(
        CallState::Held,
        CallDirection::Outgoing,
        None,
        NumberPresentation::Allowed,
        Some(3),
    );

    let actions = service.hangup_matching(1, |c| c.state != CallState::Held);
    assert_eq!(
        actions,
        vec![CommandAction::HangupCall {
            initiator: 1,
            target_peer: 2,
            reason: HangupReason::Normal,
        }]
    );
    assert_eq!(service.calls.len(), 1);
    assert_eq!(service.calls[0].state, CallState::Held);
    assert_eq!(service.calls[0].peer_id, Some(3));
}

#[test]
fn test_handle_call_hold_swap_peerless_active_and_held() {
    let mut service = CallService::default();
    service.add_call(
        CallState::Active,
        CallDirection::Outgoing,
        None,
        NumberPresentation::Allowed,
        None,
    );
    service.add_call(
        CallState::Held,
        CallDirection::Incoming,
        None,
        NumberPresentation::Allowed,
        None,
    );

    let res = service
        .handle_call_hold(
            crate::types::CallHoldParam {
                op: crate::types::CallHoldAction::HoldAndAccept,
                call_id: None,
            },
            1,
        )
        .unwrap()
        .unwrap();

    assert_eq!(res, CallResponse::WithActions(Vec::new()));
    assert_eq!(service.calls.len(), 2);
    assert_eq!(service.calls[0].state, CallState::Held);
    assert_eq!(service.calls[1].state, CallState::Active);
}
