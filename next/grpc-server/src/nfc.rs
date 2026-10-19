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

use futures::SinkExt;
use grpcio::{RpcContext, ServerStreamingSink, UnarySink, WriteFlags};
use netsim_model::{ChipClient, ChipId, ChipUpdate};
use netsim_proto::{
    nfc_service::{
        GetStatusRequest, GetStatusResponse, PollRequest, PollResponse, SendApduRequest,
        SendApduResponse, SetPowerRequest, SetPowerResponse,
    },
    nfc_service_grpc::NfcService,
};
use tracing::error;

use crate::frontend_converter::to_proto_chip;

#[derive(Clone)]
pub struct NfcServiceImpl {
    client: nfc_actor::NfcClient,
}

impl NfcServiceImpl {
    pub fn new(client: nfc_actor::NfcClient) -> Self {
        Self { client }
    }
}

impl NfcService for NfcServiceImpl {
    fn get_status(
        &mut self,
        ctx: RpcContext<'_>,
        req: GetStatusRequest,
        sink: UnarySink<GetStatusResponse>,
    ) {
        self.client.incr_get_status_count();
        let client = self.client.clone();
        ctx.spawn(async move {
            let mut resp = GetStatusResponse::new();
            if req.chip_id == 0 {
                match client.list().await {
                    Ok(chips) => {
                        for c in chips {
                            resp.chips.push(to_proto_chip(c));
                        }
                        let _ = sink.success(resp).await;
                    }
                    Err(error) => {
                        error!(%error, "GetStatus list failed");
                        let _ = sink
                            .fail(grpcio::RpcStatus::with_message(
                                grpcio::RpcStatusCode::INTERNAL,
                                format!("Failed to list NFC chips: {}", error),
                            ))
                            .await;
                    }
                }
            } else {
                match client.read(ChipId(req.chip_id)).await {
                    Ok(chip) => {
                        resp.chips.push(to_proto_chip(chip));
                        let _ = sink.success(resp).await;
                    }
                    Err(error) => {
                        error!(%error, "GetStatus read failed");
                        let _ = sink
                            .fail(grpcio::RpcStatus::with_message(
                                grpcio::RpcStatusCode::NOT_FOUND,
                                format!("Failed to get NFC chip {}: {}", req.chip_id, error),
                            ))
                            .await;
                    }
                }
            }
        });
    }

    fn set_power(
        &mut self,
        ctx: RpcContext<'_>,
        req: SetPowerRequest,
        sink: UnarySink<SetPowerResponse>,
    ) {
        self.client.incr_set_power_count();
        let client = self.client.clone();
        ctx.spawn(async move {
            let patch = ChipUpdate { enabled: Some(req.power_on), ..Default::default() };
            match client.update(ChipId(req.chip_id), patch).await {
                Ok(chip) => {
                    let mut resp = SetPowerResponse::new();
                    resp.chip = protobuf::MessageField::some(to_proto_chip(chip));
                    let _ = sink.success(resp).await;
                }
                Err(error) => {
                    if req.chip_id == 0 {
                        // Return simulated chip 0 for default/mock CLI operations when no guest is
                        // attached
                        let mut resp = SetPowerResponse::new();
                        let mock_chip = netsim_model::Chip {
                            kind: netsim_model::ChipKind::NFC,
                            id: 0,
                            name: "nfc-default".to_string(),
                            enabled: req.power_on,
                            variant: Some(netsim_model::ChipVariant::Nfc(netsim_model::Nfc {
                                radio: netsim_model::Radio {
                                    state: Some(req.power_on),
                                    ..Default::default()
                                },
                            })),
                            ..Default::default()
                        };
                        resp.chip = protobuf::MessageField::some(to_proto_chip(mock_chip));
                        let _ = sink.success(resp).await;
                    } else {
                        error!(%error, "SetPower failed");
                        let _ = sink
                            .fail(grpcio::RpcStatus::with_message(
                                grpcio::RpcStatusCode::INTERNAL,
                                format!(
                                    "Failed to set power for NFC chip {}: {}",
                                    req.chip_id, error
                                ),
                            ))
                            .await;
                    }
                }
            }
        });
    }

    fn poll(
        &mut self,
        ctx: RpcContext<'_>,
        req: PollRequest,
        mut sink: ServerStreamingSink<PollResponse>,
    ) {
        self.client.incr_poll_count();
        let client = self.client.clone();
        ctx.spawn(async move {
            let chip_exists = client.read(ChipId(req.chip_id)).await.is_ok();
            if chip_exists || req.chip_id == 0 {
                let mut resp = PollResponse::new();
                resp.target_id = 1;
                resp.tag_data = vec![0x04, 0x00]; // Simulated tag discovery ATQA / UID
                let _ = sink.send((resp, WriteFlags::default())).await;
                let _ = sink.close().await;
            } else {
                let _ = sink
                    .fail(grpcio::RpcStatus::with_message(
                        grpcio::RpcStatusCode::NOT_FOUND,
                        format!("NFC chip {} not found", req.chip_id),
                    ))
                    .await;
            }
        });
    }

    fn send_apdu(
        &mut self,
        ctx: RpcContext<'_>,
        req: SendApduRequest,
        sink: UnarySink<SendApduResponse>,
    ) {
        self.client.incr_send_apdu_count();
        let client = self.client.clone();
        ctx.spawn(async move {
            let chip_exists = client.read(ChipId(req.chip_id)).await.is_ok();
            if chip_exists || req.chip_id == 0 {
                let mut resp = SendApduResponse::new();
                resp.response = vec![0x90, 0x00]; // Return status word 90 00 (Success)
                let _ = sink.success(resp).await;
            } else {
                let _ = sink
                    .fail(grpcio::RpcStatus::with_message(
                        grpcio::RpcStatusCode::NOT_FOUND,
                        format!("NFC chip {} not found", req.chip_id),
                    ))
                    .await;
            }
        });
    }
}
