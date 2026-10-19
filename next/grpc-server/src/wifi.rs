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

use grpcio::{RpcContext, UnarySink};
use netsim_model::{ChipClient, ChipId, ChipUpdate};
use netsim_proto::{
    wifi_service::{GetStatusRequest, GetStatusResponse, SetPowerRequest, SetPowerResponse},
    wifi_service_grpc::WifiService,
};
use tracing::error;
use wifi_actor::WifiClient;

use crate::frontend_converter::to_proto_chip;

#[derive(Clone)]
pub(crate) struct WifiServiceImpl {
    client: WifiClient,
}

impl WifiServiceImpl {
    pub(crate) fn new(client: WifiClient) -> Self {
        Self { client }
    }
}

impl WifiService for WifiServiceImpl {
    fn get_status(
        &mut self,
        ctx: RpcContext<'_>,
        req: GetStatusRequest,
        sink: UnarySink<GetStatusResponse>,
    ) {
        let client = self.client.clone();
        ctx.spawn(async move {
            let mut resp = GetStatusResponse::new();
            if req.chip_id == 0 {
                match client.list().await {
                    Ok(chips) => {
                        for chip in chips {
                            resp.chips.push(to_proto_chip(chip));
                        }
                        let _ = sink.success(resp).await;
                    }
                    Err(error) => {
                        error!(%error, "GetStatus list failed");
                        let _ = sink
                            .fail(grpcio::RpcStatus::with_message(
                                grpcio::RpcStatusCode::INTERNAL,
                                format!("Failed to list Wifi chips: {}", error),
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
                                format!("Failed to get Wifi chip {}: {}", req.chip_id, error),
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
        let client = self.client.clone();
        ctx.spawn(async move {
            let patch = ChipUpdate { enabled: Some(req.power_on), ..Default::default() };
            if req.chip_id == 0 {
                match client.list().await {
                    Ok(chips) => {
                        let mut resp = SetPowerResponse::new();
                        let mut success = true;
                        let mut last_chip = None;
                        for chip in chips {
                            if let Ok(c) = client.update(ChipId(chip.id), patch.clone()).await {
                                last_chip = Some(c);
                            } else {
                                success = false;
                            }
                        }
                        if success {
                            if let Some(c) = last_chip {
                                resp.chip = protobuf::MessageField::some(to_proto_chip(c));
                            }
                            let _ = sink.success(resp).await;
                        } else {
                            let _ = sink
                                .fail(grpcio::RpcStatus::with_message(
                                    grpcio::RpcStatusCode::INTERNAL,
                                    "Failed to set power for some chips".to_string(),
                                ))
                                .await;
                        }
                    }
                    Err(e) => {
                        let _ = sink
                            .fail(grpcio::RpcStatus::with_message(
                                grpcio::RpcStatusCode::INTERNAL,
                                format!("Failed to list chips for power operation: {}", e),
                            ))
                            .await;
                    }
                }
            } else {
                match client.update(ChipId(req.chip_id), patch).await {
                    Ok(chip) => {
                        let mut resp = SetPowerResponse::new();
                        resp.chip = protobuf::MessageField::some(to_proto_chip(chip));
                        let _ = sink.success(resp).await;
                    }
                    Err(error) => {
                        error!(%error, "SetPower failed");
                        let _ = sink
                            .fail(grpcio::RpcStatus::with_message(
                                grpcio::RpcStatusCode::NOT_FOUND,
                                format!("Failed to power Wifi chip {}: {}", req.chip_id, error),
                            ))
                            .await;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use actor_framework::{ResourceClient, ResourceRequest};
    use grpcio::{ChannelBuilder, Environment, ServerBuilder, ServerCredentials};
    use netsim_model::Chip;
    use netsim_proto::{
        wifi_service::{GetStatusRequest, SetPowerRequest},
        wifi_service_grpc::{WifiServiceClient, create_wifi_service},
    };
    use tokio::sync::mpsc;
    use wifi_actor::WifiClient;

    use super::*;

    async fn setup_test_server()
    -> (grpcio::Server, String, mpsc::Receiver<ResourceRequest<wifi_actor::WifiActor>>) {
        let (wifi_tx, wifi_rx) = mpsc::channel(100);
        let wifi_client = WifiClient::new(ResourceClient::new(wifi_tx));
        let service = WifiServiceImpl::new(wifi_client);
        let env = Arc::new(Environment::new(1));
        let wifi_service = create_wifi_service(service);

        let mut server =
            ServerBuilder::new(env.clone()).register_service(wifi_service).build().unwrap();

        let port = server.add_listening_port("localhost:0", ServerCredentials::insecure()).unwrap();
        server.start();
        let addr = format!("localhost:{port}");

        (server, addr, wifi_rx)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_status_all_chips_success() {
        let (server, addr, mut wifi_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&addr);
        let client = WifiServiceClient::new(ch);

        // Spawn mock actor response
        tokio::spawn(async move {
            if let Some(ResourceRequest::List { respond_to }) = wifi_rx.recv().await {
                let chip1 = Chip {
                    id: 1,
                    name: "wifi_chip_1".to_string(),
                    kind: netsim_model::ChipKind::WIFI,
                    variant: Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi::default())),
                    ..Default::default()
                };
                let chip2 = Chip {
                    id: 2,
                    name: "wifi_chip_2".to_string(),
                    kind: netsim_model::ChipKind::WIFI,
                    variant: Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi::default())),
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(vec![chip1, chip2]));
            }
        });

        let mut req = GetStatusRequest::new();
        req.chip_id = 0; // all
        let response = client.get_status(&req);
        assert!(response.is_ok(), "{:?}", response);
        let status = response.unwrap();
        assert_eq!(status.chips.len(), 2);
        assert_eq!(status.chips[0].id, 1);
        assert_eq!(status.chips[1].id, 2);

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_status_single_chip_success() {
        let (server, addr, mut wifi_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&addr);
        let client = WifiServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(ResourceRequest::Get { id, respond_to }) = wifi_rx.recv().await {
                assert_eq!(id.0, 1);
                let chip = Chip {
                    id: 1,
                    name: "wifi_chip_1".to_string(),
                    kind: netsim_model::ChipKind::WIFI,
                    variant: Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi::default())),
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(Some(chip)));
            }
        });

        let mut req = GetStatusRequest::new();
        req.chip_id = 1;
        let response = client.get_status(&req);
        assert!(response.is_ok(), "{:?}", response);
        let status = response.unwrap();
        assert_eq!(status.chips.len(), 1);
        assert_eq!(status.chips[0].id, 1);

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_power_single_chip_success() {
        let (server, addr, mut wifi_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&addr);
        let client = WifiServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(ResourceRequest::Update { id, update, respond_to }) = wifi_rx.recv().await {
                assert_eq!(id.0, 1);
                assert_eq!(update.enabled, Some(true));
                // Ensure variant is set so to_proto_chip works correctly
                let chip = Chip {
                    id: 1,
                    name: "wifi_chip_1".to_string(),
                    enabled: true,
                    kind: netsim_model::ChipKind::WIFI,
                    variant: Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi::default())),
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(chip));
            }
        });

        let mut req = SetPowerRequest::new();
        req.chip_id = 1;
        req.power_on = true;
        let response = client.set_power(&req);
        assert!(response.is_ok(), "{:?}", response);
        let power_res = response.unwrap();
        assert!(power_res.chip.is_some());
        assert_eq!(power_res.chip.unwrap().id, 1);

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_power_all_chips_success() {
        let (server, addr, mut wifi_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&addr);
        let client = WifiServiceClient::new(ch);

        tokio::spawn(async move {
            // First expect list
            if let Some(ResourceRequest::List { respond_to }) = wifi_rx.recv().await {
                let chip1 = Chip { id: 1, name: "wifi_chip_1".to_string(), ..Default::default() };
                let chip2 = Chip { id: 2, name: "wifi_chip_2".to_string(), ..Default::default() };
                let _ = respond_to.send(Ok(vec![chip1, chip2]));
            }
            // Then expect updates
            if let Some(ResourceRequest::Update { id, respond_to, .. }) = wifi_rx.recv().await {
                assert_eq!(id.0, 1);
                let chip = Chip {
                    id: 1,
                    name: "wifi_chip_1".to_string(),
                    kind: netsim_model::ChipKind::WIFI,
                    variant: Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi::default())),
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(chip));
            }
            if let Some(ResourceRequest::Update { id, respond_to, .. }) = wifi_rx.recv().await {
                assert_eq!(id.0, 2);
                let chip = Chip {
                    id: 2,
                    name: "wifi_chip_2".to_string(),
                    kind: netsim_model::ChipKind::WIFI,
                    variant: Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi::default())),
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(chip));
            }
        });

        let mut req = SetPowerRequest::new();
        req.chip_id = 0; // all
        req.power_on = true;
        let response = client.set_power(&req);
        assert!(response.is_ok(), "{:?}", response);
        let power_res = response.unwrap();
        assert!(power_res.chip.is_some());
        // Should return the last chip updated
        assert_eq!(power_res.chip.unwrap().id, 2);

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_status_all_chips_fail() {
        let (server, addr, mut wifi_rx) = setup_test_server().await;
        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&addr);
        let client = WifiServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(ResourceRequest::List { respond_to }) = wifi_rx.recv().await {
                let _ = respond_to.send(Err(wifi_actor::WifiError::Internal("test error".into())));
            }
        });

        let mut req = GetStatusRequest::new();
        req.chip_id = 0;
        let response = client.get_status(&req);
        assert!(response.is_err());
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_status_single_chip_fail() {
        let (server, addr, mut wifi_rx) = setup_test_server().await;
        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&addr);
        let client = WifiServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(ResourceRequest::Get { id: _, respond_to }) = wifi_rx.recv().await {
                let _ = respond_to.send(Err(wifi_actor::WifiError::Internal("test error".into())));
            }
        });

        let mut req = GetStatusRequest::new();
        req.chip_id = 1;
        let response = client.get_status(&req);
        assert!(response.is_err());
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_power_single_chip_fail() {
        let (server, addr, mut wifi_rx) = setup_test_server().await;
        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&addr);
        let client = WifiServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(ResourceRequest::Update { id: _, update: _, respond_to }) =
                wifi_rx.recv().await
            {
                let _ = respond_to.send(Err(wifi_actor::WifiError::Internal("test error".into())));
            }
        });

        let mut req = SetPowerRequest::new();
        req.chip_id = 1;
        req.power_on = true;
        let response = client.set_power(&req);
        assert!(response.is_err());
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_power_all_chips_list_fail() {
        let (server, addr, mut wifi_rx) = setup_test_server().await;
        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&addr);
        let client = WifiServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(ResourceRequest::List { respond_to }) = wifi_rx.recv().await {
                let _ = respond_to.send(Err(wifi_actor::WifiError::Internal("test error".into())));
            }
        });

        let mut req = SetPowerRequest::new();
        req.chip_id = 0;
        req.power_on = true;
        let response = client.set_power(&req);
        assert!(response.is_err());
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_power_all_chips_update_fail() {
        let (server, addr, mut wifi_rx) = setup_test_server().await;
        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&addr);
        let client = WifiServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(ResourceRequest::List { respond_to }) = wifi_rx.recv().await {
                let chip1 = Chip { id: 1, name: "wifi_chip_1".to_string(), ..Default::default() };
                let _ = respond_to.send(Ok(vec![chip1]));
            }
            if let Some(ResourceRequest::Update { id: _, update: _, respond_to }) =
                wifi_rx.recv().await
            {
                let _ = respond_to.send(Err(wifi_actor::WifiError::Internal("test error".into())));
            }
        });

        let mut req = SetPowerRequest::new();
        req.chip_id = 0;
        req.power_on = true;
        let response = client.set_power(&req);
        assert!(response.is_err());
        drop(server);
    }
}
