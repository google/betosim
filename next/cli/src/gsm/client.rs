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

use netsim_proto::cell::{
    EndCall, ExecuteCellRequest, GetCellRequest, IncomingCall, ListCellsRequest,
    RegistrationStatus, RemoteAnswer, RemoteHold, SetDataRegistration, SetNetworkTechnology,
    SetSignalStrength, SetSimStatus, SetVoiceRegistration,
};

use super::args::{
    GsmCommand, HoldStateOption, RadioTechnologyOption, RegistrationStatusOption, SimStateOption,
};
use crate::{
    cell_helper::{CellClient, resolve_cell_id},
    display::DisplayExt,
    error::{Error, Result},
};

fn map_registration_status(opt: RegistrationStatusOption) -> RegistrationStatus {
    match opt {
        RegistrationStatusOption::Unregistered => RegistrationStatus::NOT_REGISTERED,
        RegistrationStatusOption::Home => RegistrationStatus::REGISTERED_HOME,
        RegistrationStatusOption::Searching => RegistrationStatus::SEARCHING,
        RegistrationStatusOption::Denied => RegistrationStatus::DENIED,
        RegistrationStatusOption::Unknown => RegistrationStatus::UNKNOWN,
        RegistrationStatusOption::Roaming => RegistrationStatus::ROAMING,
        RegistrationStatusOption::SmsOnlyHome => RegistrationStatus::SMS_ONLY_HOME,
        RegistrationStatusOption::SmsOnlyRoaming => RegistrationStatus::SMS_ONLY_ROAMING,
        RegistrationStatusOption::Emergency => RegistrationStatus::EMERGENCY,
    }
}

/// Executes a GSM subcommand against the simulated cellular client.
pub(crate) fn execute(cmd: GsmCommand, client: &impl CellClient, verbose: bool) -> Result<()> {
    match cmd {
        GsmCommand::Status(args) => {
            let id = resolve_cell_id(args.id, client)?;
            let mut req = GetCellRequest::new();
            req.id = id;
            let response = client.get(&req)?;
            println!("{}", response.display(verbose));
        }
        GsmCommand::List => {
            let req = ListCellsRequest::new();
            let response = client.list(&req)?;
            println!("{}", response.display(verbose));
        }
        GsmCommand::Call(args) => {
            let id = resolve_cell_id(args.id, client)?;
            if verbose {
                println!("Incoming call from {} triggered for cell {}.", &args.number, id);
            }
            let mut req = ExecuteCellRequest::new();
            req.id = id;
            let mut action = IncomingCall::new();
            action.number = args.number;
            req.set_incoming_call(action);
            client.execute(&req)?;
        }
        GsmCommand::Accept(args) => {
            let id = resolve_cell_id(args.id, client)?;
            let mut req = ExecuteCellRequest::new();
            req.id = id;
            // TODO(b/514348948): Pass args.number to target a specific call when cell.proto
            // and backend support multi-calling targeting.
            let action = RemoteAnswer::new();
            req.set_remote_answer(action);
            client.execute(&req)?;
            if verbose {
                println!("Accepted call from {} on cell {}.", args.number, id);
            }
        }
        GsmCommand::Cancel(args) => {
            let id = resolve_cell_id(args.id, client)?;
            let mut req = ExecuteCellRequest::new();
            req.id = id;
            // TODO(b/514348948): Pass args.number to target a specific call when cell.proto
            // and backend support multi-calling targeting.
            let action = EndCall::new();
            req.set_end_call(action);
            client.execute(&req)?;
            if verbose {
                println!("Cancelled call from {} on cell {}.", args.number, id);
            }
        }
        GsmCommand::Hold(args) => {
            let id = resolve_cell_id(args.id, client)?;
            let on_hold = match args.state {
                Some(HoldStateOption::On) => true,
                Some(HoldStateOption::Off) => false,
                None => {
                    // Toggle hold state: first query cell status
                    let mut get_req = GetCellRequest::new();
                    get_req.id = id;
                    let cell = client.get(&get_req)?;

                    let call_opt = cell.active_calls.iter().find(|c| c.number == args.number);
                    if let Some(call) = call_opt {
                        call.state.enum_value() != Ok(netsim_proto::cell::call::State::HOLDING)
                    } else {
                        return Err(Error::Message(format!(
                            "No active call found with number {}.",
                            args.number
                        )));
                    }
                }
            };

            let mut req = ExecuteCellRequest::new();
            req.id = id;
            // TODO(b/514348948): Pass args.number to target a specific call when cell.proto
            // and backend support multi-calling targeting.
            let mut action = RemoteHold::new();
            action.on_hold = on_hold;
            req.set_remote_hold(action);
            client.execute(&req)?;
            if verbose {
                println!("Remote hold set to {} for cell {}.", on_hold, id);
            }
        }
        GsmCommand::Signal(args) => {
            let id = resolve_cell_id(args.id, client)?;
            let mut req = ExecuteCellRequest::new();
            req.id = id;
            let mut action = SetSignalStrength::new();
            action.rssi = args.rssi;
            action.ber = args.ber;
            req.set_set_signal_strength(action);
            client.execute(&req)?;
            if verbose {
                println!(
                    "Signal strength set (RSSI: {}, BER: {}) for cell {}.",
                    args.rssi, args.ber, id
                );
            }
        }
        GsmCommand::Voice(args) => {
            let id = resolve_cell_id(args.id, client)?;
            let mut req = ExecuteCellRequest::new();
            req.id = id;
            let mut action = SetVoiceRegistration::new();
            action.status = protobuf::EnumOrUnknown::new(map_registration_status(args.status));
            req.set_set_voice_registration(action);
            client.execute(&req)?;
            if verbose {
                println!("Voice registration status set to {:?} for cell {}.", args.status, id);
            }
        }
        GsmCommand::Data(args) => {
            let id = resolve_cell_id(args.id, client)?;
            let mut req = ExecuteCellRequest::new();
            req.id = id;
            let mut action = SetDataRegistration::new();
            action.status = protobuf::EnumOrUnknown::new(map_registration_status(args.status));
            req.set_set_data_registration(action);
            client.execute(&req)?;
            if verbose {
                println!("Data registration status set to {:?} for cell {}.", args.status, id);
            }
        }
        GsmCommand::Sim(args) => {
            let id = resolve_cell_id(args.id, client)?;
            let mut req = ExecuteCellRequest::new();
            req.id = id;
            let mut action = SetSimStatus::new();
            action.state = protobuf::EnumOrUnknown::new(match args.state {
                SimStateOption::Present => netsim_proto::cell::set_sim_status::SimState::PRESENT,
                SimStateOption::Absent => netsim_proto::cell::set_sim_status::SimState::ABSENT,
            });
            req.set_set_sim_status(action);
            client.execute(&req)?;
            if verbose {
                println!("SIM status set to {:?} for cell {}.", args.state, id);
            }
        }
        GsmCommand::Tech(args) => {
            let id = resolve_cell_id(args.id, client)?;
            let mut req = ExecuteCellRequest::new();
            req.id = id;
            let mut action = SetNetworkTechnology::new();
            action.tech = protobuf::EnumOrUnknown::new(match args.tech {
                RadioTechnologyOption::Gsm => {
                    netsim_proto::cell::set_network_technology::RadioTechnology::GSM
                }
                RadioTechnologyOption::Lte => {
                    netsim_proto::cell::set_network_technology::RadioTechnology::LTE
                }
                RadioTechnologyOption::Nr => {
                    netsim_proto::cell::set_network_technology::RadioTechnology::NR
                }
            });
            req.set_set_network_technology(action);
            client.execute(&req)?;
            if verbose {
                println!("Network technology set to {:?} for cell {}.", args.tech, id);
            }
        }
        GsmCommand::Operator(args) => {
            let id = resolve_cell_id(args.id, client)?;
            let mut req = ExecuteCellRequest::new();
            req.id = id;
            let mut action = netsim_proto::cell::SetOperator::new();
            action.operator = args.operator.clone();
            req.set_set_operator(action);
            client.execute(&req)?;
            if verbose {
                println!("Network operator set to '{}' for cell {}.", args.operator, id);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use netsim_proto::cell::{Cell, ListCellsResponse};
    use protobuf::well_known_types::empty::Empty;

    use super::{
        super::args::{
            GsmAccept, GsmCall, GsmCancel, GsmData, GsmHold, GsmSignal, GsmSim, GsmStatus, GsmTech,
            GsmVoice, RadioTechnologyOption, SimStateOption,
        },
        *,
    };
    use crate::cell_helper::test_utils::MockCellClient;

    #[test]
    fn test_map_registration_status() {
        assert_eq!(
            map_registration_status(RegistrationStatusOption::Unregistered),
            RegistrationStatus::NOT_REGISTERED
        );
        assert_eq!(
            map_registration_status(RegistrationStatusOption::Home),
            RegistrationStatus::REGISTERED_HOME
        );
        assert_eq!(
            map_registration_status(RegistrationStatusOption::Searching),
            RegistrationStatus::SEARCHING
        );
        assert_eq!(
            map_registration_status(RegistrationStatusOption::Denied),
            RegistrationStatus::DENIED
        );
        assert_eq!(
            map_registration_status(RegistrationStatusOption::Unknown),
            RegistrationStatus::UNKNOWN
        );
        assert_eq!(
            map_registration_status(RegistrationStatusOption::Roaming),
            RegistrationStatus::ROAMING
        );
        assert_eq!(
            map_registration_status(RegistrationStatusOption::SmsOnlyHome),
            RegistrationStatus::SMS_ONLY_HOME
        );
        assert_eq!(
            map_registration_status(RegistrationStatusOption::SmsOnlyRoaming),
            RegistrationStatus::SMS_ONLY_ROAMING
        );
        assert_eq!(
            map_registration_status(RegistrationStatusOption::Emergency),
            RegistrationStatus::EMERGENCY
        );
    }

    #[test]
    fn test_gsm_status() {
        let client = MockCellClient::default();
        let cmd = GsmCommand::Status(GsmStatus { id: Some(1) });
        let mut cell = Cell::new();
        cell.id = 1;
        cell.state = "idle".to_string();
        client.get_responses.lock().unwrap().push_back(Ok(cell));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.get_calls.lock().unwrap().len(), 1);
        assert_eq!(client.get_calls.lock().unwrap()[0].id, 1);
    }

    #[test]
    fn test_gsm_list() {
        let client = MockCellClient::default();
        let cmd = GsmCommand::List;
        let mut response = ListCellsResponse::new();
        let mut cell = Cell::new();
        cell.id = 1;
        response.cells.push(cell);
        client.list_responses.lock().unwrap().push_back(Ok(response));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.list_calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_gsm_call() {
        let client = MockCellClient::default();
        let cmd = GsmCommand::Call(GsmCall { number: "1234".to_string(), id: Some(1) });
        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert_eq!(req.id, 1);
        assert!(req.has_incoming_call());
        assert_eq!(req.incoming_call().number, "1234");
    }

    #[test]
    fn test_gsm_accept() {
        let client = MockCellClient::default();
        let cmd = GsmCommand::Accept(GsmAccept { number: "1234".to_string(), id: Some(1) });
        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert_eq!(req.id, 1);
        assert!(req.has_remote_answer());
    }

    #[test]
    fn test_gsm_cancel() {
        let client = MockCellClient::default();
        let cmd = GsmCommand::Cancel(GsmCancel { number: "1234".to_string(), id: Some(1) });
        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert_eq!(req.id, 1);
        assert!(req.has_end_call());
    }

    #[test]
    fn test_gsm_hold_on() {
        let client = MockCellClient::default();
        let cmd = GsmCommand::Hold(GsmHold {
            number: "1234".to_string(),
            state: Some(HoldStateOption::On),
            id: Some(1),
        });
        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert_eq!(req.id, 1);
        assert!(req.has_remote_hold());
        assert!(req.remote_hold().on_hold);
    }

    #[test]
    fn test_gsm_hold_toggle_to_on() {
        let client = MockCellClient::default();
        let cmd =
            GsmCommand::Hold(GsmHold { number: "1234".to_string(), state: None, id: Some(1) });

        // Mock get cell status to return an active call that is NOT holding
        let mut cell = Cell::new();
        cell.id = 1;
        let mut call = netsim_proto::cell::Call::new();
        call.number = "1234".to_string();
        call.state = netsim_proto::cell::call::State::ACTIVE.into();
        cell.active_calls.push(call);
        client.get_responses.lock().unwrap().push_back(Ok(cell));

        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.get_calls.lock().unwrap().len(), 1);
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert!(req.remote_hold().on_hold); // Toggled to true
    }

    #[test]
    fn test_gsm_signal() {
        let client = MockCellClient::default();
        let cmd = GsmCommand::Signal(GsmSignal { rssi: 10, ber: 2, id: Some(1) });
        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert_eq!(req.id, 1);
        assert!(req.has_set_signal_strength());
        assert_eq!(req.set_signal_strength().rssi, 10);
        assert_eq!(req.set_signal_strength().ber, 2);
    }

    #[test]
    fn test_gsm_voice() {
        let client = MockCellClient::default();
        let cmd =
            GsmCommand::Voice(GsmVoice { status: RegistrationStatusOption::Roaming, id: Some(1) });
        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert_eq!(req.id, 1);
        assert!(req.has_set_voice_registration());
        assert_eq!(
            req.set_voice_registration().status.enum_value(),
            Ok(RegistrationStatus::ROAMING)
        );
    }

    #[test]
    fn test_gsm_data() {
        let client = MockCellClient::default();
        let cmd =
            GsmCommand::Data(GsmData { status: RegistrationStatusOption::Roaming, id: Some(1) });
        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert_eq!(req.id, 1);
        assert!(req.has_set_data_registration());
        assert_eq!(
            req.set_data_registration().status.enum_value(),
            Ok(RegistrationStatus::ROAMING)
        );
    }

    #[test]
    fn test_gsm_sim() {
        let client = MockCellClient::default();
        let cmd = GsmCommand::Sim(GsmSim { state: SimStateOption::Present, id: Some(1) });
        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert_eq!(req.id, 1);
        assert!(req.has_set_sim_status());
        assert_eq!(
            req.set_sim_status().state.enum_value(),
            Ok(netsim_proto::cell::set_sim_status::SimState::PRESENT)
        );
    }

    #[test]
    fn test_gsm_tech() {
        let client = MockCellClient::default();
        let cmd = GsmCommand::Tech(GsmTech { tech: RadioTechnologyOption::Lte, id: Some(1) });
        client.execute_responses.lock().unwrap().push_back(Ok(Empty::new()));

        let result = execute(cmd, &client, false);
        assert!(result.is_ok());
        assert_eq!(client.execute_calls.lock().unwrap().len(), 1);
        let req = &client.execute_calls.lock().unwrap()[0];
        assert_eq!(req.id, 1);
        assert!(req.has_set_network_technology());
        assert_eq!(
            req.set_network_technology().tech.enum_value(),
            Ok(netsim_proto::cell::set_network_technology::RadioTechnology::LTE)
        );
    }
}
