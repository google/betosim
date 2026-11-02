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

use ap_actor::{ApClient, ApConfig};
use grpcio::{RpcContext, UnarySink};
use netsim_model::WifiMode;
use netsim_proto::{
    access_point::{
        AccessPoint, CreateAccessPointRequest, DeleteAccessPointRequest, ExecuteAccessPointRequest,
        GetAccessPointRequest, ListAccessPointsRequest, ListAccessPointsResponse,
        UpdateAccessPointRequest,
    },
    access_point_grpc::AccessPointService,
};
use protobuf::well_known_types::empty::Empty;

#[derive(Clone)]
pub(crate) struct AccessPointServiceImpl {
    client: ApClient,
}

impl AccessPointServiceImpl {
    pub(crate) fn new(client: ApClient) -> Self {
        Self { client }
    }
}

fn ap_config_to_proto(id: u32, config: ApConfig, connected_devices: Vec<String>) -> AccessPoint {
    let mut ap = AccessPoint::new();
    ap.id = id;
    ap.ssid = config.ssid;
    ap.bssid = config.bssid.to_string();
    ap.channel = config.channel as u32;
    ap.hw_mode = config.hw_mode.to_string();
    if let Some(pass) = config.wpa_passphrase {
        ap.wpa_passphrase = pass;
    }
    ap.beacon_interval = config.beacon_interval as u32;
    if let Some(cc) = config.country_code {
        ap.country_code = cc;
    }
    ap.dtim_period = config.dtim_period as u32;
    ap.hidden_ssid = config.hidden_ssid;
    ap.sae = config.sae;
    ap.wmm_enabled = config.wmm_enabled;
    ap.enterprise_enabled = config.enterprise_enabled;
    ap.mac_acl_mode = config.mac_acl_mode as u32;
    ap.ftm_responder_enabled = config.ftm_responder_enabled;
    ap.connected_devices = connected_devices;
    ap
}

fn proto_to_ap_config(proto: AccessPoint) -> Result<ApConfig, grpcio::RpcStatus> {
    let mut config = ApConfig::default();
    if !proto.ssid.is_empty() {
        config.ssid = proto.ssid;
    }
    if !proto.bssid.is_empty() {
        if let Ok(bssid) = proto.bssid.parse::<netsim_packets::MacAddr>() {
            if bssid.bytes == [0; 6] {
                return Err(grpcio::RpcStatus::with_message(
                    grpcio::RpcStatusCode::INVALID_ARGUMENT,
                    "Invalid BSSID: all zeros is not permitted".to_string(),
                ));
            }
            config.bssid = bssid;
        } else {
            return Err(grpcio::RpcStatus::with_message(
                grpcio::RpcStatusCode::INVALID_ARGUMENT,
                format!("Invalid BSSID: {}", proto.bssid),
            ));
        }
    }
    if proto.channel != 0 {
        config.channel = proto.channel as u8;
    }
    if !proto.wpa_passphrase.is_empty() {
        config.wpa_passphrase = Some(proto.wpa_passphrase);
    }
    if proto.beacon_interval != 0 {
        config.beacon_interval = proto.beacon_interval as u16;
    }
    if !proto.country_code.is_empty() {
        config.country_code = Some(proto.country_code);
    }
    if proto.dtim_period != 0 {
        config.dtim_period = proto.dtim_period as u8;
    }
    config.hidden_ssid = proto.hidden_ssid;
    config.sae = proto.sae;
    config.wmm_enabled = proto.wmm_enabled;
    config.enterprise_enabled = proto.enterprise_enabled;
    // mac_acl_mode: 0=Disable, 1=Deny, 2=Allow
    config.mac_acl_mode = proto.mac_acl_mode as u8;
    for mac_str in &proto.mac_acl_list {
        if let Ok(mac) = mac_str.parse::<netsim_packets::MacAddr>() {
            config.mac_acl_list.push(mac);
        } else {
            return Err(grpcio::RpcStatus::with_message(
                grpcio::RpcStatusCode::INVALID_ARGUMENT,
                format!("Invalid MAC in ACL: {}", mac_str),
            ));
        }
    }
    config.ftm_responder_enabled = proto.ftm_responder_enabled;

    if !proto.hw_mode.is_empty() {
        config.hw_mode = match proto.hw_mode.as_str() {
            "a" => WifiMode::A,
            "b" => WifiMode::B,
            "g" => WifiMode::G,
            "n" => WifiMode::N,
            "ac" => WifiMode::Ac,
            "ax" => WifiMode::Ax,
            _ => WifiMode::G, // Default fallback
        };
    }
    Ok(config)
}

impl AccessPointService for AccessPointServiceImpl {
    fn create(
        &mut self,
        ctx: RpcContext,
        req: CreateAccessPointRequest,
        sink: UnarySink<AccessPoint>,
    ) {
        let client = self.client.clone();
        let f = async move {
            let id = if req.access_point.id == 0 { None } else { Some(req.access_point.id) };
            // Basic conversion - most fields might be defaulted if not present in request
            // since CreateAccessPointRequest wraps AccessPoint which has scalar fields.
            // Scalar fields default to 0/empty if not set in proto3.
            let config = match proto_to_ap_config(req.access_point.unwrap_or_default()) {
                Ok(c) => c,
                Err(status) => {
                    let _ = sink.fail(status).await;
                    return;
                }
            };

            match client.create_ap(id, config.clone()).await {
                Ok(assigned_id) => {
                    let resp = ap_config_to_proto(assigned_id, config, vec![]);
                    let _ = sink.success(resp).await;
                }
                Err(e) => {
                    let _ = sink
                        .fail(grpcio::RpcStatus::with_message(
                            grpcio::RpcStatusCode::INTERNAL,
                            format!("Failed to create AP: {}", e),
                        ))
                        .await;
                }
            }
        };
        ctx.spawn(f);
    }

    fn get(&mut self, ctx: RpcContext, req: GetAccessPointRequest, sink: UnarySink<AccessPoint>) {
        let client = self.client.clone();
        let f = async move {
            match client.get_ap(req.id).await {
                Ok(Some(state)) => {
                    let connected_devices =
                        state.associations.iter().map(|m| m.to_string()).collect();
                    let _ = sink
                        .success(ap_config_to_proto(req.id, state.config, connected_devices))
                        .await;
                }
                Ok(None) => {
                    let _ = sink
                        .fail(grpcio::RpcStatus::with_message(
                            grpcio::RpcStatusCode::NOT_FOUND,
                            format!("Access Point {} not found", req.id),
                        ))
                        .await;
                }
                Err(e) => {
                    let _ = sink
                        .fail(grpcio::RpcStatus::with_message(
                            grpcio::RpcStatusCode::INTERNAL,
                            format!("Failed to get AP: {}", e),
                        ))
                        .await;
                }
            }
        };
        ctx.spawn(f);
    }

    fn update(
        &mut self,
        ctx: RpcContext,
        req: UpdateAccessPointRequest,
        sink: UnarySink<AccessPoint>,
    ) {
        let client = self.client.clone();
        let f = async move {
            let ssid = req.ssid;
            let channel = req.channel.map(|c| c as u8);
            let force_disconnect = None;

            match client.update_ap_config(req.id, ssid, channel, force_disconnect, None).await {
                Ok(state) => {
                    let connected_devices =
                        state.associations.iter().map(|m| m.to_string()).collect();
                    let _ = sink
                        .success(ap_config_to_proto(req.id, state.config, connected_devices))
                        .await;
                }
                Err(e) => {
                    let _ = sink
                        .fail(grpcio::RpcStatus::with_message(
                            grpcio::RpcStatusCode::INTERNAL,
                            format!("Failed to update AP: {}", e),
                        ))
                        .await;
                }
            }
        };
        ctx.spawn(f);
    }

    fn delete(&mut self, ctx: RpcContext, req: DeleteAccessPointRequest, sink: UnarySink<Empty>) {
        let client = self.client.clone();
        let f = async move {
            match client.destroy_ap(req.id).await {
                Ok(_) => {
                    let _ = sink.success(Empty::new()).await;
                }
                Err(e) => {
                    let _ = sink
                        .fail(grpcio::RpcStatus::with_message(
                            grpcio::RpcStatusCode::INTERNAL,
                            format!("Failed to delete AP: {}", e),
                        ))
                        .await;
                }
            }
        };
        ctx.spawn(f);
    }

    fn list(
        &mut self,
        ctx: RpcContext,
        _req: ListAccessPointsRequest,
        sink: UnarySink<ListAccessPointsResponse>,
    ) {
        let client = self.client.clone();
        let f = async move {
            match client.list_aps().await {
                Ok(aps) => {
                    let mut resp = ListAccessPointsResponse::new();
                    for (id, ap) in aps {
                        let connected_devices =
                            ap.associations.iter().map(|m| m.to_string()).collect();
                        resp.access_points.push(ap_config_to_proto(
                            id,
                            ap.config,
                            connected_devices,
                        ));
                    }
                    let _ = sink.success(resp).await;
                }
                Err(e) => {
                    let _ = sink
                        .fail(grpcio::RpcStatus::with_message(
                            grpcio::RpcStatusCode::INTERNAL,
                            format!("Failed to list APs: {}", e),
                        ))
                        .await;
                }
            }
        };
        ctx.spawn(f);
    }

    fn execute(
        &mut self,
        ctx: RpcContext<'_>,
        req: ExecuteAccessPointRequest,
        sink: UnarySink<Empty>,
    ) {
        let client = self.client.clone();
        let f = async move {
            let result = match req.action {
                Some(
                    netsim_proto::access_point::execute_access_point_request::Action::Disconnect(d),
                ) => client.disconnect(req.id, d.mac_address).await,
                Some(_) => {
                    Err(netsim_model::ClientError::Send("Unknown action specified".to_string()))
                }
                None => Err(netsim_model::ClientError::Send("No action specified".to_string())),
            };

            match result {
                Ok(_) => {
                    let _ = sink.success(Empty::new()).await;
                }
                Err(e) => {
                    let _ = sink
                        .fail(grpcio::RpcStatus::with_message(
                            grpcio::RpcStatusCode::INTERNAL,
                            format!("Failed to execute action: {}", e),
                        ))
                        .await;
                }
            }
        };
        ctx.spawn(f);
    }
}
