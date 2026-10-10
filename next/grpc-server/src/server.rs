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

use std::sync::{Arc, OnceLock};

use device_actor::DeviceClient;
use grpcio::{
    ChannelBuilder, Environment, ResourceQuota, Server, ServerBuilder, ServerCredentials,
};
use netsim_proto::{
    access_point_grpc::create_access_point_service, ble_service_grpc::create_ble_service,
    casimir_control_grpc::create_casimir_control_service, cell_grpc::create_cell_service,
    frontend_grpc::create_frontend_service, nfc_service_grpc::create_nfc_service,
    packet_streamer_grpc::create_packet_streamer, wifi_service_grpc::create_wifi_service,
};
#[cfg(unix)]
use tracing::warn;
use tracing::{error, info};

use crate::{
    access_point::AccessPointServiceImpl, ble_service::BleServiceImpl, cell::CellServiceImpl,
    frontend::FrontendClient, nfc::NfcServiceImpl, packet_streamer::PacketStreamerService,
    wifi::WifiServiceImpl,
};

// Share a single gRPC Environment across all server instances within the same
// process (namely during parallel test execution) to prevent Abseil lock
// contention.
static SHARED_ENV: OnceLock<Arc<Environment>> = OnceLock::new();

#[allow(clippy::too_many_arguments)]
pub fn start(
    host: &str,
    port: u32,
    #[cfg(unix)] grpc_uds_path: Option<String>,
    enable_cli_ui: bool,
    device_client: DeviceClient,
    link_client: link_actor::LinkClient,
    ap_client: ap_actor::ApClient,
    cell_client: cell_actor::CellClient,
    nfc_client: nfc_actor::NfcClient,
    wifi_client: wifi_actor::WifiClient,
    capture_client: capture_actor::CaptureClient,

    packet_streamer_service: PacketStreamerService,

    version: String,
    frontend_stats: Arc<netsim_model::FrontendStats>,
) -> Result<(Server, u16), grpcio::Error> {
    let env = SHARED_ENV.get_or_init(|| Arc::new(Environment::new(1))).clone();
    let backend_service = create_packet_streamer(packet_streamer_service);
    let access_point_service =
        create_access_point_service(AccessPointServiceImpl::new(ap_client.clone()));
    let cell_service = create_cell_service(CellServiceImpl::new(cell_client));
    let casimir_service = create_casimir_control_service(
        crate::casimir::CasimirControlServiceImpl::new(nfc_client.clone()),
    );
    let nfc_service = create_nfc_service(NfcServiceImpl::new(nfc_client));
    let wifi_service = create_wifi_service(WifiServiceImpl::new(wifi_client));

    let ble_service = create_ble_service(BleServiceImpl::new(device_client.clone()));
    let reflection_service = netsim_proto::reflection_grpc::create_server_reflection(
        crate::reflection::ServerReflectionImpl::from_descriptors(&[
            netsim_proto::casimir_control::file_descriptor(),
            netsim_proto::nfc_service::file_descriptor(),
            netsim_proto::ble_service::file_descriptor(),
            netsim_proto::access_point::file_descriptor(),
            netsim_proto::cell::file_descriptor(),
            netsim_proto::wifi_service::file_descriptor(),
            netsim_proto::frontend::file_descriptor(),
            netsim_proto::packet_streamer::file_descriptor(),
            netsim_proto::reflection::file_descriptor(),
        ]),
    );
    let quota = ResourceQuota::new(Some("NetsimGrpcServerQuota")).resize_memory(1024 * 1024);
    let ch_builder = ChannelBuilder::new(env.clone()).set_resource_quota(quota).reuse_port(false);
    let mut server_builder = ServerBuilder::new(env)
        .register_service(backend_service)
        .register_service(ble_service)
        .register_service(cell_service)
        .register_service(casimir_service)
        .register_service(nfc_service)
        .register_service(wifi_service)
        .register_service(reflection_service);

    server_builder = server_builder.register_service(access_point_service);

    if enable_cli_ui {
        let frontend_service = create_frontend_service(FrontendClient::new(
            device_client.clone(),
            Arc::new(link_client),
            ap_client,
            capture_client,
            version,
            frontend_stats,
        ));
        server_builder = server_builder.register_service(frontend_service);
    }

    let mut server = server_builder.channel_args(ch_builder.build_args()).build()?;

    #[cfg(unix)]
    if let Some(ref uds_path) = grpc_uds_path {
        let unix_addr = format!("unix:{uds_path}");
        match server.add_listening_port(&unix_addr, ServerCredentials::insecure()) {
            Ok(0) | Err(_) => warn!("Failed to bind Rust gRPC on UDS {unix_addr}"),
            Ok(_) => {
                info!("Rust gRPC listening on UDS {unix_addr}");
                if let Err(e) = std::fs::set_permissions(
                    uds_path,
                    std::os::unix::fs::PermissionsExt::from_mode(0o777),
                ) {
                    warn!("Failed to set permissions on UDS {unix_addr}: {e}");
                }
            }
        }
    }

    let addr = format!("{host}:{port}");
    let port = match server.add_listening_port(&addr, ServerCredentials::insecure()) {
        Ok(p) if p > 0 => {
            info!("Rust gRPC listening on {addr}");
            Ok(p)
        }
        err => {
            error!("Failed to bind Rust gRPC on {addr}. Status: {:?}", err);
            Err(grpcio::Error::BindFail(
                std::ffi::CString::new("Failed to bind Rust gRPC server to any interface.")
                    .unwrap_or_default(),
            ))
        }
    }?;

    server.start();
    Ok((server, port))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cell_actor::{CellAction, CellActionResult};
    use futures::StreamExt;
    use grpcio::{ChannelBuilder, Environment, RpcStatusCode};
    use netsim_proto::{
        cell::{ExecuteCellRequest, GetCellRequest, ListCellsRequest},
        cell_grpc::CellServiceClient,
        nfc_service::{GetStatusRequest, PollRequest, SendApduRequest, SetPowerRequest},
        nfc_service_grpc::NfcServiceClient,
    };
    use tokio::sync::mpsc;

    async fn setup_test_server() -> (
        grpcio::Server,
        u16,
        mpsc::Receiver<actor_framework::ResourceRequest<cell_actor::CellActor>>,
        mpsc::Receiver<actor_framework::ResourceRequest<nfc_actor::NfcActor>>,
    ) {
        let (ap_tx, _ap_rx) = mpsc::channel(10);
        let ap_client = ap_actor::ApClient::new(actor_framework::ResourceClient::new(ap_tx));
        let (device_tx, _device_rx) = mpsc::channel(10);
        let device_client = device_actor::DeviceClient::new(Box::new(
            actor_framework::ResourceClient::new(device_tx),
        ));
        let link_client =
            link_actor::LinkClient::new(actor_framework::ResourceClient::new(mpsc::channel(10).0));

        let (cell_tx, cell_rx) = mpsc::channel(100);
        let cell_client = cell_actor::CellClient(actor_framework::ResourceClient::new(cell_tx));

        let (nfc_tx, nfc_rx) = mpsc::channel(100);
        let nfc_client = nfc_actor::NfcClient::new(actor_framework::ResourceClient::new(nfc_tx));

        let (wifi_tx, _wifi_rx) = mpsc::channel(10);
        let wifi_client =
            wifi_actor::WifiClient::new(actor_framework::ResourceClient::new(wifi_tx));

        let (capture_tx, _capture_rx) = mpsc::channel(10);
        let capture_client =
            capture_actor::CaptureClient::new(actor_framework::ResourceClient::new(capture_tx));

        let (new_connection_tx, _new_connection_rx) = mpsc::channel(10);
        let packet_streamer_service =
            crate::packet_streamer::PacketStreamerService::new(new_connection_tx);

        let (server, addr) = super::start(
            "localhost",
            0, // ephemeral port
            #[cfg(unix)]
            None, // UDS
            false, // cli_ui
            device_client,
            link_client,
            ap_client,
            cell_client,
            nfc_client,
            wifi_client,
            capture_client,
            packet_streamer_service,
            "test_version".to_string(),
            Arc::new(netsim_model::FrontendStats::default()),
        )
        .unwrap();

        (server, addr, cell_rx, nfc_rx)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_server_start_stop() {
        let (server, addr, _, _) = setup_test_server().await;
        assert!(addr > 0);
        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cell_service_get_success() {
        let (server, addr, mut cell_rx, _) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = CellServiceClient::new(ch);

        // Spawn mock actor response
        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Get { id, respond_to }) =
                cell_rx.recv().await
            {
                assert_eq!(id.0, 1);
                let chip = netsim_model::Chip {
                    id: 1,
                    name: "cell_chip".to_string(),
                    kind: netsim_model::ChipKind::CELLULAR,
                    variant: Some(netsim_model::ChipVariant::Cell(netsim_model::Cell {
                        state: netsim_model::MODEM_STATE_IDLE.to_string(),
                        ..Default::default()
                    })),
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(Some(chip)));
            }
        });

        let mut req = GetCellRequest::new();
        req.id = 1;
        let response = client.get(&req);
        assert!(response.is_ok());
        let cell = response.unwrap();
        assert_eq!(cell.id, 1);
        assert_eq!(cell.state, netsim_model::MODEM_STATE_IDLE);

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cell_service_get_fail() {
        let (server, addr, mut cell_rx, _) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = CellServiceClient::new(ch);

        // Spawn mock actor response returning error
        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Get { id, respond_to }) =
                cell_rx.recv().await
            {
                assert_eq!(id.0, 99);
                let _ = respond_to.send(Err(cell_actor::CellError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                ))));
            }
        });

        let mut req = GetCellRequest::new();
        req.id = 99;
        let response = client.get(&req);
        assert!(response.is_err());
        match response.unwrap_err() {
            grpcio::Error::RpcFailure(status) => {
                assert_eq!(status.code(), RpcStatusCode::NOT_FOUND);
            }
            err => panic!("Expected RpcFailure, got {:?}", err),
        }

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cell_service_list_success() {
        let (server, addr, mut cell_rx, _) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = CellServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::List { respond_to }) =
                cell_rx.recv().await
            {
                let chip = netsim_model::Chip {
                    id: 2,
                    name: "cell_chip_2".to_string(),
                    kind: netsim_model::ChipKind::CELLULAR,
                    variant: Some(netsim_model::ChipVariant::Cell(netsim_model::Cell {
                        state: netsim_model::MODEM_STATE_IDLE.to_string(),
                        ..Default::default()
                    })),
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(vec![chip]));
            }
        });

        let req = ListCellsRequest::new();
        let response = client.list(&req);
        assert!(response.is_ok());
        let list_res = response.unwrap();
        assert_eq!(list_res.cells.len(), 1);
        assert_eq!(list_res.cells[0].id, 2);

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_get_status_success() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Get { id, respond_to }) =
                nfc_rx.recv().await
            {
                assert_eq!(id.0, 1);
                let chip = netsim_model::Chip {
                    id: 1,
                    name: "nfc_chip".to_string(),
                    kind: netsim_model::ChipKind::NFC,
                    variant: Some(netsim_model::ChipVariant::Nfc(netsim_model::Nfc {
                        radio: netsim_model::Radio { state: Some(true), ..Default::default() },
                    })),
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(Some(chip)));
            }
        });

        let mut req = GetStatusRequest::new();
        req.chip_id = 1;
        let response = client.get_status(&req);
        assert!(response.is_ok());
        let status = response.unwrap();
        assert_eq!(status.chips.len(), 1);
        assert_eq!(status.chips[0].id, 1);

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cell_service_execute_success() {
        let (server, addr, mut cell_rx, _) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = CellServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Action { id, action, respond_to }) =
                cell_rx.recv().await
            {
                assert_eq!(id.unwrap().0, 1);
                match action {
                    CellAction::EndCall => {
                        let _ = respond_to.send(Ok(CellActionResult::Success));
                    }
                    _ => panic!("Unexpected action"),
                }
            }
        });

        let mut req = ExecuteCellRequest::new();
        req.id = 1;
        req.set_end_call(netsim_proto::cell::EndCall::new());
        let response = client.execute(&req);
        assert!(response.is_ok());

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cell_service_execute_pdu_success() {
        let (server, addr, mut cell_rx, _) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = CellServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Action { id, action, respond_to }) =
                cell_rx.recv().await
            {
                assert_eq!(id.unwrap().0, 1);
                match action {
                    CellAction::ReceivePdu { pdu } => {
                        assert_eq!(pdu, "00010203");
                        let _ = respond_to.send(Ok(CellActionResult::Success));
                    }
                    _ => panic!("Unexpected action: {:?}", action),
                }
            }
        });

        let mut req = ExecuteCellRequest::new();
        req.id = 1;
        let mut pdu_action = netsim_proto::cell::ReceivePdu::new();
        pdu_action.pdu = "00010203".to_string();
        req.set_receive_pdu(pdu_action);
        let response = client.execute(&req);
        assert!(response.is_ok());

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cell_service_execute_fail() {
        let (server, addr, mut cell_rx, _) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = CellServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Action { id, action, respond_to }) =
                cell_rx.recv().await
            {
                assert_eq!(id.unwrap().0, 99);
                match action {
                    CellAction::EndCall => {
                        let _ = respond_to.send(Err(cell_actor::CellError::IoError(
                            std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
                        )));
                    }
                    _ => panic!("Unexpected action"),
                }
            }
        });

        let mut req = ExecuteCellRequest::new();
        req.id = 99;
        req.set_end_call(netsim_proto::cell::EndCall::new());
        let response = client.execute(&req);
        assert!(response.is_err());
        match response.unwrap_err() {
            grpcio::Error::RpcFailure(status) => {
                assert_eq!(status.code(), RpcStatusCode::INTERNAL);
            }
            err => panic!("Expected RpcFailure, got {:?}", err),
        }

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_get_status_list_success() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::List { respond_to }) = nfc_rx.recv().await
            {
                let chip = netsim_model::Chip {
                    id: 2,
                    name: "nfc_chip_2".to_string(),
                    kind: netsim_model::ChipKind::NFC,
                    variant: Some(netsim_model::ChipVariant::Nfc(netsim_model::Nfc {
                        radio: netsim_model::Radio { state: Some(true), ..Default::default() },
                    })),
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(vec![chip]));
            }
        });

        let mut req = GetStatusRequest::new();
        req.chip_id = 0; // list
        let response = client.get_status(&req);
        assert!(response.is_ok());
        let status = response.unwrap();
        assert_eq!(status.chips.len(), 1);
        assert_eq!(status.chips[0].id, 2);

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_get_status_list_fail() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::List { respond_to }) = nfc_rx.recv().await
            {
                let _ = respond_to.send(Err(nfc_actor::NfcError::IoError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "other error",
                ))));
            }
        });

        let mut req = GetStatusRequest::new();
        req.chip_id = 0; // list
        let response = client.get_status(&req);
        assert!(response.is_err());
        match response.unwrap_err() {
            grpcio::Error::RpcFailure(status) => {
                assert_eq!(status.code(), RpcStatusCode::INTERNAL);
            }
            err => panic!("Expected RpcFailure, got {:?}", err),
        }

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_get_status_read_fail() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Get { id, respond_to }) =
                nfc_rx.recv().await
            {
                assert_eq!(id.0, 99);
                let _ = respond_to.send(Err(nfc_actor::NfcError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                ))));
            }
        });

        let mut req = GetStatusRequest::new();
        req.chip_id = 99;
        let response = client.get_status(&req);
        assert!(response.is_err());
        match response.unwrap_err() {
            grpcio::Error::RpcFailure(status) => {
                assert_eq!(status.code(), RpcStatusCode::NOT_FOUND);
            }
            err => panic!("Expected RpcFailure, got {:?}", err),
        }

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_set_power_success() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Update { id, update, respond_to }) =
                nfc_rx.recv().await
            {
                assert_eq!(id.0, 1);
                assert_eq!(update.enabled, Some(true));
                let chip = netsim_model::Chip {
                    id: 1,
                    name: "nfc_chip".to_string(),
                    kind: netsim_model::ChipKind::NFC,
                    variant: Some(netsim_model::ChipVariant::Nfc(netsim_model::Nfc {
                        radio: netsim_model::Radio { state: Some(true), ..Default::default() },
                    })),
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(chip));
            }
        });

        let mut req = SetPowerRequest::new();
        req.chip_id = 1;
        req.power_on = true;
        let response = client.set_power(&req);
        assert!(response.is_ok());
        let power_res = response.unwrap();
        assert!(power_res.chip.is_some());
        assert_eq!(power_res.chip.unwrap().id, 1);

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_set_power_mock_success() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Update { id, respond_to, .. }) =
                nfc_rx.recv().await
            {
                assert_eq!(id.0, 0);
                // Fail to trigger the fallback logic in set_power
                let _ = respond_to.send(Err(nfc_actor::NfcError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                ))));
            }
        });

        let mut req = SetPowerRequest::new();
        req.chip_id = 0;
        req.power_on = true;
        let response = client.set_power(&req);
        assert!(response.is_ok());
        let power_res = response.unwrap();
        assert!(power_res.chip.is_some());
        let chip = power_res.chip.unwrap();
        assert_eq!(chip.id, 0);
        assert_eq!(chip.name, "nfc-default");

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_set_power_fail() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Update { id, respond_to, .. }) =
                nfc_rx.recv().await
            {
                assert_eq!(id.0, 99);
                let _ = respond_to.send(Err(nfc_actor::NfcError::IoError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "other error",
                ))));
            }
        });

        let mut req = SetPowerRequest::new();
        req.chip_id = 99;
        req.power_on = true;
        let response = client.set_power(&req);
        assert!(response.is_err());
        match response.unwrap_err() {
            grpcio::Error::RpcFailure(status) => {
                assert_eq!(status.code(), RpcStatusCode::INTERNAL);
            }
            err => panic!("Expected RpcFailure, got {:?}", err),
        }

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_poll_success() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Get { id, respond_to }) =
                nfc_rx.recv().await
            {
                assert_eq!(id.0, 1);
                let chip = netsim_model::Chip {
                    id: 1,
                    name: "nfc_chip".to_string(),
                    kind: netsim_model::ChipKind::NFC,
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(Some(chip)));
            }
        });

        let mut req = PollRequest::new();
        req.chip_id = 1;
        let mut stream = client.poll(&req).unwrap();
        let first_resp = stream.next().await;
        assert!(first_resp.is_some());
        let resp = first_resp.unwrap().unwrap();
        assert_eq!(resp.target_id, 1);
        assert_eq!(resp.tag_data, vec![0x04, 0x00]);

        // Stream should close
        assert!(stream.next().await.is_none());

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_poll_mock_success() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Get { id, respond_to }) =
                nfc_rx.recv().await
            {
                assert_eq!(id.0, 0);
                // Return not found to simulate fallback
                let _ = respond_to.send(Err(nfc_actor::NfcError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                ))));
            }
        });

        let mut req = PollRequest::new();
        req.chip_id = 0;
        let mut stream = client.poll(&req).unwrap();
        let first_resp = stream.next().await;
        assert!(first_resp.is_some());
        let resp = first_resp.unwrap().unwrap();
        assert_eq!(resp.target_id, 1);

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_poll_fail() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Get { id, respond_to }) =
                nfc_rx.recv().await
            {
                assert_eq!(id.0, 99);
                let _ = respond_to.send(Err(nfc_actor::NfcError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                ))));
            }
        });

        let mut req = PollRequest::new();
        req.chip_id = 99;
        let mut stream = client.poll(&req).unwrap();
        let first_resp = stream.next().await;
        assert!(first_resp.is_some());
        let err = first_resp.unwrap().unwrap_err();
        match err {
            grpcio::Error::RpcFailure(status) => {
                assert_eq!(status.code(), RpcStatusCode::NOT_FOUND);
            }
            err => panic!("Expected RpcFailure, got {:?}", err),
        }

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_send_apdu_success() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Get { id, respond_to }) =
                nfc_rx.recv().await
            {
                assert_eq!(id.0, 1);
                let chip = netsim_model::Chip {
                    id: 1,
                    name: "nfc_chip".to_string(),
                    kind: netsim_model::ChipKind::NFC,
                    ..Default::default()
                };
                let _ = respond_to.send(Ok(Some(chip)));
            }
        });

        let mut req = SendApduRequest::new();
        req.chip_id = 1;
        req.apdu = vec![0x00, 0xA4, 0x04, 0x00, 0x00];
        let response = client.send_apdu(&req);
        assert!(response.is_ok());
        let apdu_res = response.unwrap();
        assert_eq!(apdu_res.response, vec![0x90, 0x00]);

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_send_apdu_mock_success() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Get { id, respond_to }) =
                nfc_rx.recv().await
            {
                assert_eq!(id.0, 0);
                let _ = respond_to.send(Err(nfc_actor::NfcError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                ))));
            }
        });

        let mut req = SendApduRequest::new();
        req.chip_id = 0;
        let response = client.send_apdu(&req);
        assert!(response.is_ok());

        drop(server);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_nfc_service_send_apdu_fail() {
        let (server, addr, _, mut nfc_rx) = setup_test_server().await;

        let env = Arc::new(Environment::new(1));
        let ch = ChannelBuilder::new(env).connect(&format!("localhost:{}", addr));
        let client = NfcServiceClient::new(ch);

        tokio::spawn(async move {
            if let Some(actor_framework::ResourceRequest::Get { id, respond_to }) =
                nfc_rx.recv().await
            {
                assert_eq!(id.0, 99);
                let _ = respond_to.send(Err(nfc_actor::NfcError::IoError(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "not found",
                ))));
            }
        });

        let mut req = SendApduRequest::new();
        req.chip_id = 99;
        let response = client.send_apdu(&req);
        assert!(response.is_err());
        match response.unwrap_err() {
            grpcio::Error::RpcFailure(status) => {
                assert_eq!(status.code(), RpcStatusCode::NOT_FOUND);
            }
            err => panic!("Expected RpcFailure, got {:?}", err),
        }

        drop(server);
    }
}
