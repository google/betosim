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

use super::*;

#[test]
fn test_cops_query_registered_numeric() {
    let response = NetworkResponse::OperatorQuery {
        is_registered: true,
        mode: CopsMode::Automatic,
        format: CopsFormat::Numeric,
        plmn: "310260".to_string(),
        quirks: Quirks::default(),
    };
    assert_eq!(format!("{response}"), "+COPS: 0,2,\"310260\"\r\n");
}

#[test]
fn test_cops_query_unregistered_standard() {
    let quirks = Quirks { goldfish_ril_37_or_earlier: false, ..Default::default() };
    let response = NetworkResponse::OperatorQuery {
        is_registered: false,
        mode: CopsMode::Automatic,
        format: CopsFormat::Numeric,
        plmn: "310260".to_string(),
        quirks,
    };
    assert_eq!(format!("{response}"), "+COPS: 0\r\n");
}

#[test]
fn test_cops_query_unregistered_goldfish_37_compound_queries() {
    let quirks = Quirks { goldfish_ril_37_or_earlier: true, ..Default::default() };
    for (fmt, expected_fmt_code) in [
        (CopsFormat::LongAlphanumeric, "0"),
        (CopsFormat::ShortAlphanumeric, "1"),
        (CopsFormat::Numeric, "2"),
    ] {
        let response = NetworkResponse::OperatorQuery {
            is_registered: false,
            mode: CopsMode::Automatic,
            format: fmt,
            plmn: "310260".to_string(),
            quirks,
        };
        assert_eq!(format!("{response}"), format!("+COPS: 0,{expected_fmt_code},\"\"\r\n"));
    }
}

#[test]
fn test_radio_admission_truth_table() {
    let mut net = NetworkService::new(Quirks::default(), None);
    // Default: radio_power is Full, voice/data are NotRegistered.
    assert!(net.is_radio_on());

    assert_eq!(net.can_originate_voice_call(false), Err(AdmissionDenial::NoNetworkService));
    assert_eq!(net.can_originate_voice_call(true), Ok(()));
    assert_eq!(net.can_originate_data(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_voice_call());
    assert_eq!(net.can_originate_sms(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_sms());

    net.set_registration(RegistrationType::Voice, RegistrationStatus::Searching);
    net.set_registration(RegistrationType::Data, RegistrationStatus::Searching);
    assert_eq!(net.can_originate_voice_call(false), Err(AdmissionDenial::NoNetworkService));
    assert_eq!(net.can_originate_voice_call(true), Ok(()));
    assert_eq!(net.can_originate_data(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_voice_call());
    assert_eq!(net.can_originate_sms(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_sms());

    net.set_registration(RegistrationType::Voice, RegistrationStatus::Denied);
    net.set_registration(RegistrationType::Data, RegistrationStatus::Denied);
    assert_eq!(net.can_originate_voice_call(false), Err(AdmissionDenial::NoNetworkService));
    assert_eq!(net.can_originate_voice_call(true), Ok(()));
    assert_eq!(net.can_originate_data(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_voice_call());
    assert_eq!(net.can_originate_sms(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_sms());

    net.set_registration(RegistrationType::Voice, RegistrationStatus::Unknown);
    net.set_registration(RegistrationType::Data, RegistrationStatus::Unknown);
    assert_eq!(net.can_originate_voice_call(false), Err(AdmissionDenial::NoNetworkService));
    assert_eq!(net.can_originate_voice_call(true), Ok(()));
    assert_eq!(net.can_originate_data(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_voice_call());
    assert_eq!(net.can_originate_sms(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_sms());

    net.set_registration(RegistrationType::Voice, RegistrationStatus::Emergency);
    net.set_registration(RegistrationType::Data, RegistrationStatus::Emergency);
    assert_eq!(net.can_originate_voice_call(false), Err(AdmissionDenial::EmergencyCallsOnly));
    assert_eq!(net.can_originate_voice_call(true), Ok(()));
    assert_eq!(net.can_originate_data(), Err(AdmissionDenial::EmergencyCallsOnly));
    assert!(!net.can_terminate_voice_call());
    assert_eq!(net.can_originate_sms(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_sms());

    net.set_registration(RegistrationType::Voice, RegistrationStatus::RegisteredHome);
    net.set_registration(RegistrationType::Data, RegistrationStatus::RegisteredHome);
    assert_eq!(net.can_originate_voice_call(false), Ok(()));
    assert_eq!(net.can_originate_voice_call(true), Ok(()));
    assert_eq!(net.can_originate_data(), Ok(()));
    assert!(net.can_terminate_voice_call());
    assert_eq!(net.can_originate_sms(), Ok(()));
    assert!(net.can_terminate_sms());

    net.set_registration(RegistrationType::Voice, RegistrationStatus::Roaming);
    net.set_registration(RegistrationType::Data, RegistrationStatus::Roaming);
    assert_eq!(net.can_originate_voice_call(false), Ok(()));
    assert_eq!(net.can_originate_voice_call(true), Ok(()));
    assert_eq!(net.can_originate_data(), Ok(()));
    assert!(net.can_terminate_voice_call());
    assert_eq!(net.can_originate_sms(), Ok(()));
    assert!(net.can_terminate_sms());

    net.radio_power = RadioPowerLevel::Minimum;
    assert!(!net.is_radio_on());
    assert_eq!(net.can_originate_voice_call(false), Err(AdmissionDenial::NoNetworkService));
    assert_eq!(net.can_originate_voice_call(true), Err(AdmissionDenial::NoNetworkService));
    assert_eq!(net.can_originate_data(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_voice_call());
    assert_eq!(net.can_originate_sms(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_sms());

    net.radio_power = RadioPowerLevel::DisableRf;
    assert!(!net.is_radio_on());
    assert_eq!(net.can_originate_voice_call(false), Err(AdmissionDenial::NoNetworkService));
    assert_eq!(net.can_originate_voice_call(true), Err(AdmissionDenial::NoNetworkService));
    assert_eq!(net.can_originate_data(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_voice_call());
    assert_eq!(net.can_originate_sms(), Err(AdmissionDenial::NoNetworkService));
    assert!(!net.can_terminate_sms());
}

#[test]
fn test_emergency_registration_wire_status_quirk_gating() {
    let mut legacy_service = NetworkService::new(
        Quirks { goldfish_ril_37_or_earlier: true, ..Default::default() },
        None,
    );
    let mut modern_service = NetworkService::new(
        Quirks { goldfish_ril_37_or_earlier: false, ..Default::default() },
        None,
    );

    assert_eq!(
        legacy_service.wire_registration_status(RegistrationStatus::Emergency),
        RegistrationStatus::NotRegistered
    );
    assert_eq!(
        modern_service.wire_registration_status(RegistrationStatus::Emergency),
        RegistrationStatus::Emergency
    );

    legacy_service.voice_registration = RegistrationStatus::Emergency;
    let query_legacy =
        legacy_service.handle_query_registration(RegistrationType::Voice).unwrap().unwrap();
    assert_eq!(format!("{query_legacy}"), "+CREG: 0,0\r\n");

    modern_service.voice_registration = RegistrationStatus::Emergency;
    let query_modern =
        modern_service.handle_query_registration(RegistrationType::Voice).unwrap().unwrap();
    assert_eq!(format!("{query_modern}"), "+CREG: 0,8\r\n");

    legacy_service.voice_unsol_mode = RegistrationUnsolicitedMode::Enable;
    modern_service.voice_unsol_mode = RegistrationUnsolicitedMode::Enable;
    legacy_service.voice_registration = RegistrationStatus::NotRegistered;
    modern_service.voice_registration = RegistrationStatus::NotRegistered;

    let legacy_urc =
        legacy_service.set_registration(RegistrationType::Voice, RegistrationStatus::Emergency);
    assert_eq!(legacy_urc, None);

    let modern_urc =
        modern_service.set_registration(RegistrationType::Voice, RegistrationStatus::Emergency);
    assert_eq!(modern_urc, Some("+CREG: 8\r\n".to_string()));
}

#[test]
fn test_operator_validation_and_selection() {
    use crate::tests::test_utils::MockCardState;

    let mut service = NetworkService::new(
        Quirks::default(),
        Some(Plmn::parse("310260").expect("valid test PLMN")),
    );
    let sim = MockCardState::default();
    let data = MockDataBearerState::default();

    // Verify is_valid_operator helper matching PLMN and standard long/short operator names
    assert!(service.is_valid_operator("310260"));
    assert!(service.is_valid_operator(crate::constants::DEFAULT_OPERATOR_NAME_LONG));
    assert!(service.is_valid_operator(crate::constants::DEFAULT_OPERATOR_NAME_SHORT));
    assert!(!service.is_valid_operator("999999"));

    // Test CopsMode::Automatic attaches and returns URCs
    let auto_res = service
        .handle_set_operator(
            CopsMode::Automatic,
            Some(CopsFormat::Numeric),
            None,
            None,
            &sim,
            &data,
        )
        .unwrap();
    assert!(auto_res.is_some());
    assert!(service.is_attached);
    assert_eq!(service.cops_mode, CopsMode::Automatic);
    assert_eq!(service.cops_format, CopsFormat::Numeric);

    // Test CopsMode::Manual with valid operator
    let manual_valid_res = service
        .handle_set_operator(
            CopsMode::Manual,
            Some(CopsFormat::LongAlphanumeric),
            Some("310260"),
            None,
            &sim,
            &data,
        )
        .unwrap();
    assert_eq!(service.cops_mode, CopsMode::Manual);
    assert_eq!(service.cops_format, CopsFormat::LongAlphanumeric);
    // Already attached without tech change, so no new URCs emitted
    assert_eq!(manual_valid_res, None);

    // Test CopsMode::Manual with invalid operator denies registration
    let manual_invalid_res =
        service.handle_set_operator(CopsMode::Manual, None, Some("InvalidOp"), None, &sim, &data);
    assert!(manual_invalid_res.is_err());
    assert!(!service.is_attached);
    assert_eq!(service.voice_registration, RegistrationStatus::Denied);
    assert_eq!(service.data_registration, RegistrationStatus::Denied);

    // Test CopsMode::ManualAutomatic with invalid operator falls back to Automatic
    let manual_auto_res = service
        .handle_set_operator(
            CopsMode::ManualAutomatic,
            Some(CopsFormat::Numeric),
            Some("NonExistentOp"),
            None,
            &sim,
            &data,
        )
        .unwrap();
    assert_eq!(service.cops_mode, CopsMode::Automatic);
    assert!(service.is_attached);
    assert!(manual_auto_res.is_some());
}

#[derive(Default)]
struct MockDataBearerState<'a>(&'a [u8]);

impl DataBearerState for MockDataBearerState<'_> {
    fn active_context_ids(&self) -> impl Iterator<Item = u8> {
        self.0.iter().copied()
    }
}

#[test]
fn test_physical_channel_config_formatting_lte() {
    let mut service = NetworkService::new(Quirks::default(), None);
    service.voice_registration = RegistrationStatus::RegisteredHome;
    service.data_registration = RegistrationStatus::RegisteredHome;
    service.current_network_mode = CtecTechnology::Lte;

    let formatted: Vec<String> = service
        .physical_channel_config_urcs(&MockDataBearerState(&[1, 2]))
        .map(|u| u.to_string())
        .collect();
    assert_eq!(
        formatted,
        &[
            "%CGFPCCFG: 1,5000,5,0,1\r\n",
            "%CGFPCCFG: 2,5000,5,0,1\r\n",
            "%CGFPCCFG: 1,5000,5,0,2\r\n",
            "%CGFPCCFG: 2,5000,5,0,2\r\n",
        ]
    );
}

#[test]
fn test_physical_channel_config_formatting_nr() {
    let mut service = NetworkService::new(Quirks::default(), None);
    service.voice_registration = RegistrationStatus::RegisteredHome;
    service.data_registration = RegistrationStatus::RegisteredHome;
    service.current_network_mode = CtecTechnology::Nr;

    let formatted: Vec<String> = service
        .physical_channel_config_urcs(&MockDataBearerState(&[1]))
        .map(|u| u.to_string())
        .collect();
    assert_eq!(formatted, &["%CGFPCCFG: 1,50000,6,4,1\r\n", "%CGFPCCFG: 2,50000,6,4,1\r\n"]);
}

#[test]
fn test_physical_channel_config_formatting_gsm_and_wcdma() {
    let mut service = NetworkService::new(Quirks::default(), None);
    service.voice_registration = RegistrationStatus::RegisteredHome;
    service.data_registration = RegistrationStatus::RegisteredHome;

    service.current_network_mode = CtecTechnology::Gsm;
    let gsm_formatted: Vec<String> = service
        .physical_channel_config_urcs(&MockDataBearerState(&[1]))
        .map(|u| u.to_string())
        .collect();
    assert_eq!(gsm_formatted, &["%CGFPCCFG: 1,5000,0,0,1\r\n", "%CGFPCCFG: 2,5000,0,0,1\r\n"]);

    service.current_network_mode = CtecTechnology::Wcdma;
    let wcdma_formatted: Vec<String> = service
        .physical_channel_config_urcs(&MockDataBearerState(&[1]))
        .map(|u| u.to_string())
        .collect();
    assert_eq!(wcdma_formatted, &["%CGFPCCFG: 1,5000,1,0,1\r\n", "%CGFPCCFG: 2,5000,1,0,1\r\n"]);
}

#[test]
fn test_physical_channel_config_suppression_when_radio_off() {
    let mut service = NetworkService::new(Quirks::default(), None);
    let data = MockDataBearerState(&[1]);
    service.radio_power = RadioPowerLevel::Minimum;
    service.voice_registration = RegistrationStatus::RegisteredHome;
    assert_eq!(service.physical_channel_config_urcs(&data).next(), None);
}

#[test]
fn test_physical_channel_config_suppression_when_unregistered() {
    let mut service = NetworkService::new(Quirks::default(), None);
    let data = MockDataBearerState(&[1]);
    service.radio_power = RadioPowerLevel::Full;
    service.voice_registration = RegistrationStatus::NotRegistered;
    service.data_registration = RegistrationStatus::NotRegistered;
    assert_eq!(service.physical_channel_config_urcs(&data).next(), None);
}

#[test]
fn test_physical_channel_config_suppression_when_emergency_only() {
    let mut service = NetworkService::new(Quirks::default(), None);
    let data = MockDataBearerState(&[1]);
    service.radio_power = RadioPowerLevel::Full;
    service.voice_registration = RegistrationStatus::Emergency;
    service.data_registration = RegistrationStatus::Emergency;
    assert_eq!(service.physical_channel_config_urcs(&data).next(), None);
}

#[test]
fn test_physical_channel_config_suppression_when_no_active_cids() {
    let mut service = NetworkService::new(Quirks::default(), None);
    service.voice_registration = RegistrationStatus::RegisteredHome;
    assert_eq!(service.physical_channel_config_urcs(&MockDataBearerState(&[])).next(), None);
}

#[test]
fn test_handle_set_ctec_nr_unclamped_with_physical_channel_configs() {
    let mut service = NetworkService::new(Quirks::default(), None);
    service.is_attached = true;
    service.voice_registration = RegistrationStatus::RegisteredHome;
    service.data_registration = RegistrationStatus::RegisteredHome;
    service.voice_unsol_mode = RegistrationUnsolicitedMode::Enable;

    let data = MockDataBearerState(&[1]);
    let res = service.handle_set_ctec(CtecTechnology::Nr, None, &data).unwrap();
    assert_eq!(service.act, AccessTechnology::Nr);
    assert_eq!(service.current_network_mode, CtecTechnology::Nr);

    let Some(NetworkResponse::CtecSetResult { urcs }) = res else {
        panic!("Expected CtecSetResult with URCs");
    };
    let formatted: Vec<String> = urcs
        .into_iter()
        .filter(|u| matches!(u, NetworkUrc::PhysicalChannelConfig(_)))
        .map(|u| u.to_string())
        .collect();
    assert_eq!(formatted, &["%CGFPCCFG: 1,50000,6,4,1\r\n", "%CGFPCCFG: 2,50000,6,4,1\r\n"]);
}

fn registered_network_service() -> NetworkService {
    let mut service = NetworkService::new(
        Quirks::default(),
        Some(Plmn::parse("310260").expect("valid test PLMN")),
    );
    service.voice_registration = RegistrationStatus::RegisteredHome;
    service.data_registration = RegistrationStatus::RegisteredHome;
    service.is_attached = true;
    service
}

#[test]
fn test_physical_channel_timer_coalesces_while_pending() {
    let mut service = registered_network_service();
    let data = MockDataBearerState(&[1]);

    let token1 = service
        .schedule_physical_channel_update(&data)
        .expect("scheduling should succeed when registered with active CIDs");
    assert_eq!(service.schedule_physical_channel_update(&data), None);

    let formatted: Vec<String> =
        service.handle_physical_channel_timeout(token1, &data).map(|u| u.to_string()).collect();
    assert_eq!(formatted, &["%CGFPCCFG: 1,5000,5,0,1\r\n", "%CGFPCCFG: 2,5000,5,0,1\r\n"]);
}

#[test]
fn test_physical_channel_timer_canceled_on_detach_and_ignores_stale_token() {
    let mut service = registered_network_service();
    let data = MockDataBearerState(&[1]);

    let token1 = service
        .schedule_physical_channel_update(&data)
        .expect("scheduling should succeed when registered with active CIDs");
    service.detach_network();
    assert_eq!(service.handle_physical_channel_timeout(token1, &data).next(), None);

    service.voice_registration = RegistrationStatus::RegisteredHome;
    service.data_registration = RegistrationStatus::RegisteredHome;
    service.is_attached = true;
    let token2 = service
        .schedule_physical_channel_update(&data)
        .expect("scheduling should succeed after re-registration");
    assert_ne!(token1, token2);

    assert_eq!(service.handle_physical_channel_timeout(token1, &data).next(), None);

    let formatted: Vec<String> =
        service.handle_physical_channel_timeout(token2, &data).map(|u| u.to_string()).collect();
    assert_eq!(formatted, &["%CGFPCCFG: 1,5000,5,0,1\r\n", "%CGFPCCFG: 2,5000,5,0,1\r\n"]);
}

#[test]
fn test_physical_channel_timer_canceled_on_operator_denial() {
    use crate::tests::test_utils::MockCardState;

    let mut service = registered_network_service();
    let sim = MockCardState::default();
    let data = MockDataBearerState(&[1]);

    let token = service
        .schedule_physical_channel_update(&data)
        .expect("scheduling should succeed when registered with active CIDs");
    let res =
        service.handle_set_operator(CopsMode::Manual, None, Some("InvalidOp"), None, &sim, &data);
    assert!(res.is_err());
    assert_eq!(service.voice_registration, RegistrationStatus::Denied);

    service.voice_registration = RegistrationStatus::RegisteredHome;
    service.data_registration = RegistrationStatus::RegisteredHome;
    service.is_attached = true;
    assert_eq!(service.handle_physical_channel_timeout(token, &data).next(), None);
}
