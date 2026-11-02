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

use cell_actor::{CellAction, CellClient};
use grpcio::{RpcContext, UnarySink};
use netsim_model::{ChipClient, MODEM_STATE_RINGING};
use netsim_proto::{
    cell::{Cell, ExecuteCellRequest, GetCellRequest, ListCellsRequest, ListCellsResponse},
    cell_grpc::CellService,
};
use protobuf::well_known_types::empty::Empty;
use tracing::error;

#[derive(Clone)]
pub(crate) struct CellServiceImpl {
    client: CellClient,
}

impl CellServiceImpl {
    pub(crate) fn new(client: CellClient) -> Self {
        Self { client }
    }
}

fn map_proto_reg_status(
    proto_status: netsim_proto::cell::RegistrationStatus,
) -> netsim_model::RegistrationStatus {
    match proto_status {
        netsim_proto::cell::RegistrationStatus::NOT_REGISTERED => {
            netsim_model::RegistrationStatus::NotRegistered
        }
        netsim_proto::cell::RegistrationStatus::REGISTERED_HOME => {
            netsim_model::RegistrationStatus::RegisteredHome
        }
        netsim_proto::cell::RegistrationStatus::SEARCHING => {
            netsim_model::RegistrationStatus::Searching
        }
        netsim_proto::cell::RegistrationStatus::DENIED => netsim_model::RegistrationStatus::Denied,
        netsim_proto::cell::RegistrationStatus::ROAMING => {
            netsim_model::RegistrationStatus::Roaming
        }
        netsim_proto::cell::RegistrationStatus::SMS_ONLY_HOME => {
            netsim_model::RegistrationStatus::RegisteredSmsOnlyHome
        }
        netsim_proto::cell::RegistrationStatus::SMS_ONLY_ROAMING => {
            netsim_model::RegistrationStatus::RegisteredSmsOnlyRoaming
        }
        netsim_proto::cell::RegistrationStatus::EMERGENCY => {
            netsim_model::RegistrationStatus::Emergency
        }
        _ => netsim_model::RegistrationStatus::Unknown,
    }
}

fn map_model_reg_status(
    model_status: netsim_model::RegistrationStatus,
) -> protobuf::EnumOrUnknown<netsim_proto::cell::RegistrationStatus> {
    match model_status {
        netsim_model::RegistrationStatus::NotRegistered => {
            protobuf::EnumOrUnknown::new(netsim_proto::cell::RegistrationStatus::NOT_REGISTERED)
        }
        netsim_model::RegistrationStatus::RegisteredHome => {
            protobuf::EnumOrUnknown::new(netsim_proto::cell::RegistrationStatus::REGISTERED_HOME)
        }
        netsim_model::RegistrationStatus::Searching => {
            protobuf::EnumOrUnknown::new(netsim_proto::cell::RegistrationStatus::SEARCHING)
        }
        netsim_model::RegistrationStatus::Denied => {
            protobuf::EnumOrUnknown::new(netsim_proto::cell::RegistrationStatus::DENIED)
        }
        netsim_model::RegistrationStatus::Roaming => {
            protobuf::EnumOrUnknown::new(netsim_proto::cell::RegistrationStatus::ROAMING)
        }
        netsim_model::RegistrationStatus::RegisteredSmsOnlyHome => {
            protobuf::EnumOrUnknown::new(netsim_proto::cell::RegistrationStatus::SMS_ONLY_HOME)
        }
        netsim_model::RegistrationStatus::RegisteredSmsOnlyRoaming => {
            protobuf::EnumOrUnknown::new(netsim_proto::cell::RegistrationStatus::SMS_ONLY_ROAMING)
        }
        netsim_model::RegistrationStatus::Emergency => {
            protobuf::EnumOrUnknown::new(netsim_proto::cell::RegistrationStatus::EMERGENCY)
        }
        netsim_model::RegistrationStatus::Unknown => {
            protobuf::EnumOrUnknown::new(netsim_proto::cell::RegistrationStatus::UNKNOWN)
        }
    }
}

fn map_model_to_proto_cell(chip_id: u32, cell_info: &netsim_model::Cell) -> Cell {
    let mut cell = Cell::new();
    cell.id = chip_id;
    cell.state = cell_info.state.clone();
    cell.ringing = cell.state == MODEM_STATE_RINGING;
    cell.sms_count = cell_info.sms_count;
    cell.rssi = cell_info.rssi;
    cell.ber = cell_info.ber;
    cell.voice_registration = map_model_reg_status(cell_info.voice_registration);
    cell.data_registration = map_model_reg_status(cell_info.data_registration);
    cell.active_calls = cell_info
        .active_calls
        .iter()
        .map(|c| {
            let mut proto_call = netsim_proto::cell::Call::new();
            proto_call.number = c.number.clone();
            proto_call.state = protobuf::EnumOrUnknown::new(match c.state {
                netsim_model::CallState::Unknown => netsim_proto::cell::call::State::UNKNOWN,
                netsim_model::CallState::Active => netsim_proto::cell::call::State::ACTIVE,
                netsim_model::CallState::Holding => netsim_proto::cell::call::State::HOLDING,
                netsim_model::CallState::Dialing => netsim_proto::cell::call::State::DIALING,
                netsim_model::CallState::Alerting => netsim_proto::cell::call::State::ALERTING,
                netsim_model::CallState::Incoming => netsim_proto::cell::call::State::INCOMING,
                netsim_model::CallState::Waiting => netsim_proto::cell::call::State::WAITING,
            });
            proto_call.direction = protobuf::EnumOrUnknown::new(match c.direction {
                netsim_model::CallDirection::Unknown => {
                    netsim_proto::cell::call::Direction::DIR_UNKNOWN
                }
                netsim_model::CallDirection::MobileOriginated => {
                    netsim_proto::cell::call::Direction::MOBILE_ORIGINATED
                }
                netsim_model::CallDirection::MobileTerminated => {
                    netsim_proto::cell::call::Direction::MOBILE_TERMINATED
                }
            });
            proto_call
        })
        .collect();
    cell
}

impl CellService for CellServiceImpl {
    fn get(&mut self, ctx: RpcContext, req: GetCellRequest, sink: UnarySink<Cell>) {
        let client = self.client.clone();
        ctx.spawn(async move {
            let chip_id = netsim_model::ChipId(req.id);
            match client.read(chip_id).await {
                Ok(chip) => {
                    let cell =
                        if let Some(netsim_model::ChipVariant::Cell(cell_info)) = chip.variant {
                            map_model_to_proto_cell(chip.id, &cell_info)
                        } else {
                            Cell::new()
                        };
                    let _ = sink.success(cell).await;
                }
                Err(error) => {
                    error!(%error, "Get cell failed");
                    let status = grpcio::RpcStatus::with_message(
                        grpcio::RpcStatusCode::NOT_FOUND,
                        format!("Failed to get cell: {}", error),
                    );
                    let _ = sink.fail(status).await;
                }
            }
        });
    }

    fn list(
        &mut self,
        ctx: RpcContext,
        _req: ListCellsRequest,
        sink: UnarySink<ListCellsResponse>,
    ) {
        let client = self.client.clone();
        ctx.spawn(async move {
            match client.list().await {
                Ok(chips) => {
                    let mut response = ListCellsResponse::new();
                    for chip in chips {
                        if let Some(netsim_model::ChipVariant::Cell(cell_info)) = chip.variant {
                            response.cells.push(map_model_to_proto_cell(chip.id, &cell_info));
                        }
                    }
                    let _ = sink.success(response).await;
                }
                Err(error) => {
                    error!(%error, "List cells failed");
                    let status = grpcio::RpcStatus::with_message(
                        grpcio::RpcStatusCode::INTERNAL,
                        format!("Failed to list cells: {}", error),
                    );
                    let _ = sink.fail(status).await;
                }
            }
        });
    }

    fn execute(&mut self, ctx: RpcContext, req: ExecuteCellRequest, sink: UnarySink<Empty>) {
        let client = self.client.clone();
        ctx.spawn(async move {
            let chip_id = netsim_model::ChipId(req.id);

            let action = match req.action {
                Some(netsim_proto::cell::execute_cell_request::Action::IncomingCall(call_info)) => {
                    CellAction::IncomingCall { number: call_info.number }
                }
                Some(netsim_proto::cell::execute_cell_request::Action::UpdateCall(_)) => {
                    CellAction::UpdateCall
                }
                Some(netsim_proto::cell::execute_cell_request::Action::EndCall(_)) => {
                    CellAction::EndCall
                }
                Some(netsim_proto::cell::execute_cell_request::Action::ReceiveSms(s)) => {
                    CellAction::ReceiveSms { sender: s.sender, text: s.text }
                }
                Some(netsim_proto::cell::execute_cell_request::Action::ReceivePdu(p)) => {
                    CellAction::ReceivePdu { pdu: p.pdu }
                }
                Some(netsim_proto::cell::execute_cell_request::Action::SetSignalStrength(s)) => {
                    CellAction::SetSignalStrength { rssi: s.rssi, ber: s.ber }
                }
                Some(netsim_proto::cell::execute_cell_request::Action::SetVoiceRegistration(r)) => {
                    let status = map_proto_reg_status(r.status.enum_value_or_default());
                    CellAction::SetVoiceRegistration { status }
                }
                Some(netsim_proto::cell::execute_cell_request::Action::SetDataRegistration(r)) => {
                    let status = map_proto_reg_status(r.status.enum_value_or_default());
                    CellAction::SetDataRegistration { status }
                }
                Some(netsim_proto::cell::execute_cell_request::Action::RemoteAnswer(_)) => {
                    CellAction::RemoteAnswer
                }
                Some(netsim_proto::cell::execute_cell_request::Action::RemoteHold(h)) => {
                    CellAction::RemoteHold { on_hold: h.on_hold }
                }
                Some(netsim_proto::cell::execute_cell_request::Action::SetSimStatus(s)) => {
                    let present = match s.state.enum_value_or_default() {
                        netsim_proto::cell::set_sim_status::SimState::ABSENT => false,
                        netsim_proto::cell::set_sim_status::SimState::PRESENT => true,
                    };
                    CellAction::SetSimStatus { present }
                }
                Some(netsim_proto::cell::execute_cell_request::Action::SetNetworkTechnology(t)) => {
                    let tech = match t.tech.enum_value_or_default() {
                        netsim_proto::cell::set_network_technology::RadioTechnology::GSM => {
                            netsim_model::RadioTechnology::Gsm
                        }
                        netsim_proto::cell::set_network_technology::RadioTechnology::LTE => {
                            netsim_model::RadioTechnology::Lte
                        }
                        netsim_proto::cell::set_network_technology::RadioTechnology::NR => {
                            netsim_model::RadioTechnology::Nr
                        }
                        _ => netsim_model::RadioTechnology::Unknown,
                    };
                    CellAction::SetNetworkTechnology { tech }
                }
                Some(netsim_proto::cell::execute_cell_request::Action::SetOperator(o)) => {
                    CellAction::SetOperator { operator: o.operator.clone() }
                }
                Some(_) => {
                    let _ = sink
                        .fail(grpcio::RpcStatus::with_message(
                            grpcio::RpcStatusCode::UNIMPLEMENTED,
                            "unsupported action".into(),
                        ))
                        .await;
                    return;
                }
                None => {
                    let _ = sink
                        .fail(grpcio::RpcStatus::with_message(
                            grpcio::RpcStatusCode::INVALID_ARGUMENT,
                            "missing action".into(),
                        ))
                        .await;
                    return;
                }
            };

            match client.perform_action(chip_id, action).await {
                Ok(_) => {
                    let _ = sink.success(Empty::new()).await;
                }
                Err(error) => {
                    error!(%error, "Execute cell action failed");
                    let _ = sink
                        .fail(grpcio::RpcStatus::with_message(
                            grpcio::RpcStatusCode::INTERNAL,
                            format!("Failed to execute action: {}", error),
                        ))
                        .await;
                }
            }
        });
    }
}
