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
use std::{io::Error as IoError, pin::Pin};

use actor_framework::{ResourceClient, ResourceRequest};
use bytes::Bytes;
use cell_actor::CellClient;
use device_actor::{DeviceActor, DeviceClient};
use device_api::{DeviceAction, DeviceActionResult};
use futures::{StreamExt, channel::mpsc as fmpsc, future::ready, sink::SinkExt};
use netsim_model::{
    Cell, Chip, ChipClient, ChipCreate, ChipError as NetsimChipError, ChipId, ChipVariant,
    DeviceId, PacketSink, PacketStream,
};
use tokio::sync::mpsc;

// Helper to create a dummy PacketStream and PacketSink
fn create_dummy_stream_sink()
-> (PacketStream, PacketSink, fmpsc::Sender<Bytes>, fmpsc::Receiver<Bytes>) {
    let (stream_tx, stream_rx) = fmpsc::channel::<Bytes>(64);
    let (sink_tx, sink_rx) = fmpsc::channel::<Bytes>(64);

    let stream: PacketStream = Box::new(stream_rx);
    let sink: PacketSink = Pin::from(Box::new(
        sink_tx
            .with(|data: Bytes| ready(Ok(data)))
            .sink_map_err(|e: fmpsc::SendError| IoError::other(e.to_string())),
    ));

    (stream, sink, stream_tx, sink_rx)
}

struct TestHarness {
    client: CellClient,
    slirp_client: slirp_actor::SlirpClient,
    device_server_rx: mpsc::Receiver<ResourceRequest<DeviceActor>>,
    server_handle: tokio::task::JoinHandle<()>,
    _slirp_handle: tokio::task::JoinHandle<()>,
    event_tx: mpsc::UnboundedSender<modem_rs::HostEvent>,
}

async fn setup_test_harness() -> TestHarness {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let (device_server_tx, device_server_rx) = mpsc::channel(100);
    let resource_client = ResourceClient::new(device_server_tx);
    let device_client = DeviceClient::new(Box::new(resource_client));
    let (slirp_runner, slirp_client) = slirp_actor::new();
    let slirp_actor = slirp_actor::SlirpActor::new(Default::default(), None, None).await;
    let _slirp_handle = tokio::spawn(slirp_runner.run(slirp_actor));

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<modem_rs::HostEvent>();
    let controller = Box::new(modem_rs::ModemNetworkSimulator::new(event_tx.clone()));
    let (actor, client) = cell_actor::new();
    let service = cell_actor::CellActor {
        device_client,
        slirp_client: slirp_client.clone(),
        controller,
        active_chips: std::collections::HashMap::new(),
        event_receiver: Some(event_rx),
    };
    let server_handle = tokio::spawn(actor.run(service));

    TestHarness { client, slirp_client, device_server_rx, server_handle, _slirp_handle, event_tx }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        self.server_handle.abort();
        self._slirp_handle.abort();
    }
}

fn create_params(chip_id: ChipId, stream: PacketStream, sink: PacketSink) -> ChipCreate {
    let mut chip = Chip::new_test_cell(format!("cell-{}", chip_id));
    chip.id = chip_id.0;
    chip.device_id = DeviceId(1);
    chip.variant = Some(ChipVariant::Cell(Cell::default()));

    ChipCreate { packet_stream: Some(stream), packet_sink: Some(sink), chip }
}

// T011: Test for CreateChip message
#[tokio::test]
async fn test_create_chip() {
    let harness = setup_test_harness().await;
    let chip_id = ChipId(1);
    let (stream, sink, _, _) = create_dummy_stream_sink();
    let params = create_params(chip_id, stream, sink);
    let response = harness.client.create(chip_id, params).await;
    assert!(response.is_ok(), "CreateChip failed: {:?}", response);
}

// T012: Test for DeleteChip message
#[tokio::test]
async fn test_delete_chip() {
    let mut harness = setup_test_harness().await;
    let chip_id = ChipId(2);
    let (stream, sink, _, _) = create_dummy_stream_sink();
    let params = create_params(chip_id, stream, sink);
    harness.client.create(chip_id, params).await.unwrap();

    let client = harness.client.clone();
    let delete_handle = tokio::spawn(async move { client.delete(chip_id).await });

    let notification_future = async {
        match tokio::time::timeout(
            std::time::Duration::from_secs(1),
            harness.device_server_rx.recv(),
        )
        .await
        {
            Ok(Some(ResourceRequest::Action {
                action: DeviceAction::NotifyChipRemoved(_device_id, id),
                respond_to,
                ..
            })) => {
                assert_eq!(id, chip_id);
                let _ = respond_to.send(Ok(DeviceActionResult::Success));
            }
            Ok(Some(msg)) => panic!("Received unexpected message: {:?}", msg),
            Ok(None) => panic!("Stream closed"),
            Err(_) => panic!("Timed out waiting for NotifyChipRemoved message"),
        }
    };

    tokio::select! {
        res = delete_handle => {
            let response = res.unwrap();
            assert!(response.is_ok(), "DeleteChip failed: {:?}", response);
        }
        _ = notification_future => {}
        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
            panic!("Test timed out");
        }
    }
}

// T013: Test for stream error triggering chip deletion
#[tokio::test]
async fn test_stream_error_triggers_delete() {
    let mut harness = setup_test_harness().await;
    let chip_id = ChipId(3);
    let (stream, sink, mut stream_tx, _) = create_dummy_stream_sink();
    let params = create_params(chip_id, stream, sink);
    harness.client.create(chip_id, params).await.unwrap();

    // Close the stream sender to simulate an error
    stream_tx.close_channel();

    match tokio::time::timeout(std::time::Duration::from_secs(2), harness.device_server_rx.recv())
        .await
    {
        Ok(Some(ResourceRequest::Action {
            action: DeviceAction::NotifyChipRemoved(_device_id, id),
            respond_to,
            ..
        })) => {
            assert_eq!(id, chip_id);
            let _ = respond_to.send(Ok(DeviceActionResult::Success));
        }
        Ok(Some(msg)) => panic!("Received unexpected message: {:?}", msg),
        Ok(None) => panic!("Stream closed"),
        Err(_) => panic!("Timed out waiting for NotifyChipRemoved message on stream error"),
    }
}

// Test for host event SinkError triggering chip deletion
#[tokio::test]
async fn test_sink_error_triggers_delete() {
    let mut harness = setup_test_harness().await;
    let chip_id = ChipId(4);
    let (stream, sink, _stream_tx, _sink_rx) = create_dummy_stream_sink();
    let params = create_params(chip_id, stream, sink);
    harness.client.create(chip_id, params).await.unwrap();

    // Verify chip was created and is active
    let chip = harness.client.read(chip_id).await.unwrap();
    assert_eq!(chip.id, chip_id.0);

    // Send HostEvent::SinkError to simulate a broken client sink
    harness.event_tx.send(modem_rs::HostEvent::SinkError(chip_id.0)).unwrap();

    // Verify NotifyChipRemoved is sent to DeviceActor
    match tokio::time::timeout(std::time::Duration::from_secs(2), harness.device_server_rx.recv())
        .await
    {
        Ok(Some(ResourceRequest::Action {
            action: DeviceAction::NotifyChipRemoved(_device_id, id),
            respond_to,
            ..
        })) => {
            assert_eq!(id, chip_id);
            let _ = respond_to.send(Ok(DeviceActionResult::Success));
        }
        Ok(Some(msg)) => panic!("Received unexpected message: {:?}", msg),
        Ok(None) => panic!("Stream closed"),
        Err(_) => panic!("Timed out waiting for NotifyChipRemoved message on sink error"),
    }

    // Verify chip is deleted from CellActor
    let read_result = harness.client.read(chip_id).await;
    assert!(
        read_result.is_err(),
        "Expected chip to be deleted after SinkError, got: {:?}",
        read_result
    );
}

// T020: Test stream to controller passthrough and ECHO response
#[tokio::test]
async fn test_stream_to_controller_echo() {
    // Requires controller to echo. ModemNetworkSimulator might not by default
    // unless we send AT commands it understands.
}

// T028: Test GetChip message
#[tokio::test]
async fn test_get_chip() {
    let harness = setup_test_harness().await;
    let chip_id = ChipId(1);
    let (stream, sink, _stream_tx, _sink_rx) = create_dummy_stream_sink();
    let params = create_params(chip_id, stream, sink);
    let create_response = harness.client.create(chip_id, params).await;
    assert!(create_response.is_ok(), "CreateChip failed: {:?}", create_response);

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let chip = harness.client.read(chip_id).await.unwrap();
    if let Some(ChipVariant::Cell(cell_chip)) = &chip.variant {
        assert_eq!(cell_chip.state, "idle");
    } else {
        panic!("GetChip failed for existing chip");
    }

    // Test non-existent chip
    let bad_chip_id = ChipId(99);
    match harness.client.read(bad_chip_id).await {
        Err(netsim_model::ClientError::Chip(NetsimChipError::ChipNotFound(id))) => {
            assert_eq!(id, bad_chip_id);
        }
        other => panic!("Expected ChipNotFound error, got {:?}", other),
    }
}

// T014: Test that delete chip does not block the actor loop while waiting for
// DeviceActor
#[tokio::test]
async fn test_delete_chip_non_blocking() {
    let mut harness = setup_test_harness().await;
    let chip_id = ChipId(2);
    let (stream, sink, _, _) = create_dummy_stream_sink();
    let params = create_params(chip_id, stream, sink);
    harness.client.create(chip_id, params).await.unwrap();

    let client = harness.client.clone();
    // Start delete in a spawned task so we don't block the test
    let delete_handle = tokio::spawn(async move { client.delete(chip_id).await });

    // Wait for the NotifyChipRemoved message to arrive at the mock DeviceActor
    let msg =
        tokio::time::timeout(std::time::Duration::from_secs(1), harness.device_server_rx.recv())
            .await
            .unwrap()
            .unwrap();

    let ResourceRequest::Action {
        action: DeviceAction::NotifyChipRemoved(_device_id, id),
        respond_to,
        ..
    } = msg
    else {
        panic!("Unexpected message");
    };
    assert_eq!(id, chip_id);

    // At this point, NotifyChipRemoved has been sent, but we have NOT responded.
    // If CellActor is non-blocking (async), it should be able to process other
    // requests.
    let read_client = harness.client.clone();
    let read_handle = tokio::spawn(async move { read_client.read(chip_id).await });

    // Verify read completes (it might return error/NotFound, but it should NOT
    // hang)
    let read_res = tokio::time::timeout(std::time::Duration::from_millis(200), read_handle).await;
    assert!(read_res.is_ok(), "Read call was blocked by pending delete notification!");

    // Now respond to NotifyChipRemoved to let delete finish
    let _ = respond_to.send(Ok(DeviceActionResult::Success));

    // Verify delete finishes
    delete_handle.await.unwrap().unwrap();
}

// Test status fields are populated and updated
#[tokio::test]
async fn test_get_chip_status_fields() {
    let harness = setup_test_harness().await;
    let chip_id = ChipId(1);
    let (stream, sink, _stream_tx, _sink_rx) = create_dummy_stream_sink();
    let params = create_params(chip_id, stream, sink);
    harness.client.create(chip_id, params).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 1. Initial status check
    let chip = harness.client.read(chip_id).await.unwrap();
    if let Some(ChipVariant::Cell(cell_chip)) = &chip.variant {
        assert_eq!(cell_chip.sms_count, 0);
        assert_eq!(cell_chip.rssi, 20); // Default RSSI in NetworkService::new is 20
        assert_eq!(cell_chip.ber, 99); // Default BER in NetworkService::new is 99
        assert_eq!(cell_chip.voice_registration, netsim_model::RegistrationStatus::NotRegistered);
        assert_eq!(cell_chip.data_registration, netsim_model::RegistrationStatus::NotRegistered);
    } else {
        panic!("GetChip failed for existing chip");
    }

    // 2. Update signal strength
    harness
        .client
        .perform_action(chip_id, cell_actor::CellAction::SetSignalStrength { rssi: 15, ber: 2 })
        .await
        .unwrap();

    // 3. Update registration
    harness
        .client
        .perform_action(
            chip_id,
            cell_actor::CellAction::SetVoiceRegistration {
                status: netsim_model::RegistrationStatus::RegisteredHome,
            },
        )
        .await
        .unwrap();
    harness
        .client
        .perform_action(
            chip_id,
            cell_actor::CellAction::SetDataRegistration {
                status: netsim_model::RegistrationStatus::Roaming,
            },
        )
        .await
        .unwrap();

    // 4. Verify updates
    let chip = harness.client.read(chip_id).await.unwrap();
    if let Some(ChipVariant::Cell(cell_chip)) = &chip.variant {
        assert_eq!(cell_chip.rssi, 15);
        assert_eq!(cell_chip.ber, 2);
        assert_eq!(cell_chip.voice_registration, netsim_model::RegistrationStatus::RegisteredHome);
        assert_eq!(cell_chip.data_registration, netsim_model::RegistrationStatus::Roaming);
    } else {
        panic!("GetChip failed for existing chip");
    }
}

#[tokio::test]
async fn test_receive_pdu_action_forwarding() {
    let harness = setup_test_harness().await;
    let chip_id = ChipId(1);
    let (stream, sink, _stream_tx, mut sink_rx) = create_dummy_stream_sink();
    let params = create_params(chip_id, stream, sink);
    harness.client.create(chip_id, params).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    harness
        .client
        .perform_action(
            chip_id,
            cell_actor::CellAction::SetVoiceRegistration {
                status: netsim_model::RegistrationStatus::RegisteredHome,
            },
        )
        .await
        .unwrap();

    harness
        .client
        .perform_action(
            chip_id,
            cell_actor::CellAction::ReceivePdu {
                pdu: "0011000B915155255155F40000AA01F0".to_string(),
            },
        )
        .await
        .unwrap();

    let packet = tokio::time::timeout(std::time::Duration::from_millis(500), sink_rx.next())
        .await
        .unwrap()
        .unwrap();

    // CMT response: "+CMT: ,15\r\n0011000B915155255155F40000AA01F0\r\n"
    let expected_response = "+CMT: ,15\r\n0011000B915155255155F40000AA01F0\r\n";
    assert_eq!(packet, bytes::Bytes::from(expected_response));
}

#[tokio::test]
async fn test_receive_sms_action_forwarding() {
    let harness = setup_test_harness().await;
    let chip_id = ChipId(1);
    let (stream, sink, _stream_tx, mut sink_rx) = create_dummy_stream_sink();
    let params = create_params(chip_id, stream, sink);
    harness.client.create(chip_id, params).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    harness
        .client
        .perform_action(
            chip_id,
            cell_actor::CellAction::SetVoiceRegistration {
                status: netsim_model::RegistrationStatus::RegisteredHome,
            },
        )
        .await
        .unwrap();

    harness
        .client
        .perform_action(
            chip_id,
            cell_actor::CellAction::ReceiveSms {
                sender: "123456".to_string(),
                text: "hello".to_string(),
            },
        )
        .await
        .unwrap();

    let packet = tokio::time::timeout(std::time::Duration::from_millis(500), sink_rx.next())
        .await
        .unwrap()
        .unwrap();

    let packet_str = std::str::from_utf8(&packet).unwrap().to_lowercase();
    assert!(packet_str.starts_with("+cmt: ,26\r\n002406812143650008"));
    assert!(packet_str.ends_with("0a00680065006c006c006f\r\n"));
}

#[tokio::test]
async fn test_receive_pdu_invalid_hex() {
    let harness = setup_test_harness().await;
    let chip_id = ChipId(1);
    let (stream, sink, _stream_tx, _sink_rx) = create_dummy_stream_sink();
    let params = create_params(chip_id, stream, sink);
    harness.client.create(chip_id, params).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Invalid hex character 'g'
    let result = harness
        .client
        .perform_action(
            chip_id,
            cell_actor::CellAction::ReceivePdu {
                pdu: "0011000B915155255155F40000AA01Fg".to_string(),
            },
        )
        .await;

    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(err_str.contains("Invalid PDU format"), "Unexpected error: {err_str}");
}

#[tokio::test]
async fn test_incoming_call_invalid_number() {
    let harness = setup_test_harness().await;
    let chip_id = ChipId(1);
    let (stream, sink, _stream_tx, _sink_rx) = create_dummy_stream_sink();
    let params = create_params(chip_id, stream, sink);
    harness.client.create(chip_id, params).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Invalid character 'a' in phone number
    let result = harness
        .client
        .perform_action(
            chip_id,
            cell_actor::CellAction::IncomingCall { number: "12345abc".to_string() },
        )
        .await;

    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(err_str.contains("Invalid phone number"), "Unexpected error: {err_str}");
}

#[tokio::test]
async fn test_network_config_lifecycle_and_update() {
    use std::net::{IpAddr, Ipv4Addr};

    let harness = setup_test_harness().await;
    let chip_id = ChipId(1);
    let (stream, sink, _stream_tx, _sink_rx) = create_dummy_stream_sink();
    let mut params = create_params(chip_id, stream, sink);

    let initial_config = netsim_model::CellNetworkConfig {
        ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 97, 2)),
        prefixlen: 30,
        gateway: IpAddr::V4(Ipv4Addr::new(192, 168, 97, 1)),
        dns: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
    };
    if let Some(ChipVariant::Cell(cell)) = &mut params.chip.variant {
        cell.network_configs = vec![initial_config.clone()];
    }

    harness.client.create(chip_id, params).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let chip = harness.client.read(chip_id).await.unwrap();
    if let Some(ChipVariant::Cell(cell_chip)) = &chip.variant {
        assert_eq!(cell_chip.network_configs, vec![initial_config]);
    } else {
        panic!("GetChip failed for existing chip");
    }

    let updated_config = netsim_model::CellNetworkConfig {
        ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 97, 6)),
        prefixlen: 30,
        gateway: IpAddr::V4(Ipv4Addr::new(192, 168, 97, 5)),
        dns: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
    };
    let update = netsim_model::ChipUpdate {
        variant: Some(netsim_model::ChipVariantUpdate::Cell(netsim_model::CellUpdate {
            network_configs: Some(vec![updated_config.clone()]),
            ..Default::default()
        })),
        ..Default::default()
    };
    harness.client.update(chip_id, update).await.unwrap();

    let updated_chip = harness.client.read(chip_id).await.unwrap();
    if let Some(ChipVariant::Cell(cell_chip)) = &updated_chip.variant {
        assert_eq!(cell_chip.network_configs, vec![updated_config]);
    } else {
        panic!("GetChip failed for updated chip");
    }
}

#[tokio::test]
async fn test_goldfish_leases_and_releases_slirp_ip() {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use netsim_model::Quirks;

    let harness = setup_test_harness().await;

    let chip_id = ChipId(1);
    let goldfish_quirks = Quirks { dynamic_slirp_lease: true, ..Default::default() };
    let (stream, sink, _stream_tx, _sink_rx) = create_dummy_stream_sink();
    let mut params = create_params(chip_id, stream, sink);
    if let Some(ChipVariant::Cell(cell)) = &mut params.chip.variant {
        cell.quirks = goldfish_quirks;
        cell.network_configs.clear();
    }

    harness.client.create(chip_id, params).await.expect("Goldfish create should succeed");

    let chip = harness.client.read(chip_id).await.unwrap();
    let cell_chip = match chip.variant {
        Some(ChipVariant::Cell(c)) => c,
        _ => panic!("Expected Cell variant"),
    };
    let (expected_v4, expected_v6) =
        (Ipv4Addr::new(10, 0, 2, 32), Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0x20));

    assert_eq!(cell_chip.network_configs.len(), 2, "must allocate dual-stack v4 and v6 configs");
    assert_eq!(cell_chip.network_configs[0].ip_address, IpAddr::V4(expected_v4));
    assert_eq!(cell_chip.network_configs[1].ip_address, IpAddr::V6(expected_v6));

    harness.client.delete(chip_id).await.expect("delete should succeed");

    let new_lease = harness.slirp_client.allocate_lease(ChipId(2)).await.unwrap().unwrap();
    assert_eq!(new_lease.v4.ip_address, expected_v4);
}

#[tokio::test]
async fn test_cuttlefish_and_provided_configs_do_not_lease_slirp_ip() {
    use std::net::{IpAddr, Ipv4Addr};

    use netsim_model::{CellNetworkConfig, Quirks};

    let harness = setup_test_harness().await;
    let cell_client = &harness.client;
    let slirp_client = &harness.slirp_client;

    let cuttlefish_id = ChipId(10);
    let cuttlefish_quirks = Quirks { is_cuttlefish: true, ..Default::default() };
    let (stream, sink, _stream_tx, _sink_rx) = create_dummy_stream_sink();
    let mut params = create_params(cuttlefish_id, stream, sink);
    if let Some(ChipVariant::Cell(cell)) = &mut params.chip.variant {
        cell.quirks = cuttlefish_quirks;
        cell.network_configs.clear();
    }
    cell_client.create(cuttlefish_id, params).await.expect("Cuttlefish create should succeed");
    let chip = cell_client.read(cuttlefish_id).await.unwrap();
    let cf_cell = match chip.variant {
        Some(ChipVariant::Cell(c)) => c,
        _ => panic!("Expected Cell variant"),
    };
    assert!(cf_cell.network_configs.is_empty(), "Cuttlefish must not receive Slirp IP");

    let default_id = ChipId(15);
    let (stream, sink, _stream_tx, _sink_rx) = create_dummy_stream_sink();
    let mut params = create_params(default_id, stream, sink);
    if let Some(ChipVariant::Cell(cell)) = &mut params.chip.variant {
        cell.quirks = Quirks::default();
        cell.network_configs.clear();
    }
    cell_client.create(default_id, params).await.expect("Default quirks create should succeed");
    let chip = cell_client.read(default_id).await.unwrap();
    let def_cell = match chip.variant {
        Some(ChipVariant::Cell(c)) => c,
        _ => panic!("Expected Cell variant"),
    };
    assert!(def_cell.network_configs.is_empty(), "Non-emulator must not receive Slirp IP");

    let provided_id = ChipId(20);
    let custom_config = vec![CellNetworkConfig {
        ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
        prefixlen: 24,
        gateway: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
        dns: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
    }];
    let (stream, sink, _stream_tx, _sink_rx) = create_dummy_stream_sink();
    let mut params = create_params(provided_id, stream, sink);
    if let Some(ChipVariant::Cell(cell)) = &mut params.chip.variant {
        cell.quirks = Quirks { dynamic_slirp_lease: true, ..Default::default() };
        cell.network_configs = custom_config.clone();
    }
    cell_client.create(provided_id, params).await.expect("Custom configs create should succeed");
    let chip = cell_client.read(provided_id).await.unwrap();
    let prov_cell = match chip.variant {
        Some(ChipVariant::Cell(c)) => c,
        _ => panic!("Expected Cell variant"),
    };
    assert_eq!(prov_cell.network_configs.len(), 1);
    assert_eq!(prov_cell.network_configs[0].ip_address, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)));

    let expected_v4 = Ipv4Addr::new(10, 0, 2, 32);

    let lease = slirp_client.allocate_lease(ChipId(30)).await.unwrap().unwrap();
    assert_eq!(lease.v4.ip_address, expected_v4);
}
