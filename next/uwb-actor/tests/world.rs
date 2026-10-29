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

use std::collections::HashMap;

use bytes::Bytes;
use device_actor::DeviceClient;
use netsim_model::{
    Chip, ChipClient, ChipCreate, ChipError, ChipId, ChipUpdate, ClientError, DeviceId, Position,
};
use netsim_testing::{mock_sink, mock_stream};
use pdl_runtime::Packet;
use pica::packets::uci::{self, ShortAddressTwoWayRangingMeasurement};
use tokio::sync::mpsc::{Receiver, Sender};
use uwb_actor::{UwbActor, UwbClient};

/// The BDD World for UWB Actor tests.
pub struct World {
    pub client: UwbClient,
    pub _device_client: DeviceClient, /* Keep reference if we need to check notifications, or
                                       * use a Mock */
    pub packet_txs: HashMap<ChipId, Sender<Bytes>>,
    pub packet_rxs: HashMap<ChipId, Receiver<Vec<u8>>>,
    _actor_task: tokio::task::JoinHandle<()>,
}

impl Drop for World {
    fn drop(&mut self) {
        self._actor_task.abort();
    }
}

impl World {
    pub async fn new() -> Self {
        let (runner, client) = uwb_actor::new();

        // Use MockActorClient for the DeviceClient
        let mut mock_device_client = actor_framework::MockActorClient::new();
        mock_device_client
            .expect_perform_action()
            .returning(|_, _| Ok(device_api::DeviceActionResult::Success));
        mock_device_client.expect_clone_box().returning(|| {
            let mut new_mock = actor_framework::MockActorClient::new();
            new_mock
                .expect_perform_action()
                .returning(|_, _| Ok(device_api::DeviceActionResult::Success));
            // Expect clone_box recursively if needed, but UwbActor probably doesn't clone
            // it again? Better to be safe:
            new_mock.expect_clone_box().returning(|| {
                let mut inner_mock = actor_framework::MockActorClient::new();
                inner_mock
                    .expect_perform_action()
                    .returning(|_, _| Ok(device_api::DeviceActionResult::Success));
                inner_mock.expect_clone_box().returning(|| {
                    let mut deep_mock = actor_framework::MockActorClient::new();
                    deep_mock
                        .expect_perform_action()
                        .returning(|_, _| Ok(device_api::DeviceActionResult::Success));
                    // Hope we don't need deeper than this (Mock4)
                    Box::new(deep_mock)
                });
                Box::new(inner_mock)
            });
            Box::new(new_mock)
        });

        let device_client = DeviceClient::new(Box::new(mock_device_client));

        // Spawn actor
        let actor = UwbActor::new(device_client.clone());
        let actor_task = tokio::spawn(runner.run(actor));

        World {
            client,
            _device_client: device_client,
            packet_txs: HashMap::new(),
            packet_rxs: HashMap::new(),
            _actor_task: actor_task,
        }
    }

    pub async fn when_create_chip(&mut self, chip_id: u32) -> Result<(), ClientError> {
        let id = ChipId(chip_id);
        let (stream, packet_tx) = mock_stream();
        let (sink, packet_rx) = mock_sink();
        let mut chip = Chip::new_test_uwb(format!("uwb_chip_{id}"));
        chip.id = id.0;
        chip.device_id = DeviceId(1);

        let params = ChipCreate { packet_stream: Some(stream), packet_sink: Some(sink), chip };

        self.client.create(id, params).await?;

        self.packet_txs.insert(id, packet_tx);
        self.packet_rxs.insert(id, packet_rx);

        Ok(())
    }

    pub async fn when_delete_chip(&self, chip_id: u32) -> Result<(), ClientError> {
        self.client.delete(ChipId(chip_id)).await
    }

    pub async fn when_get_chip(&self, chip_id: u32) -> Result<Chip, ClientError> {
        self.client.read(ChipId(chip_id)).await
    }

    pub fn and_packet_stream_is_closed(&mut self, chip_id: u32) {
        let chip_id = ChipId(chip_id);
        self.packet_txs.remove(&chip_id);
    }

    pub fn and_packet_sink_is_closed(&mut self, chip_id: u32) {
        let chip_id = ChipId(chip_id);
        self.packet_rxs.remove(&chip_id);
    }

    pub async fn and_tick_occurs(&mut self) {
        tokio::time::sleep(2 * UwbActor::TICK_INTERVAL).await;
    }

    pub async fn then_chip_does_not_exist(&self, chip_id: u32) {
        // Yield to allow the actor to process the stream/sink closure.
        tokio::task::yield_now().await;
        let err = self.when_get_chip(chip_id).await.unwrap_err();
        let Some(ChipError::ChipNotFound(ChipId(actual_chip_id))) = err.as_chip_error() else {
            panic!("Unexpected error: {err}");
        };
        assert_eq!(*actual_chip_id, chip_id);
    }

    pub async fn when_packet_is_sent(&mut self, chip_id: u32, packet: &[u8]) {
        let chip_id = ChipId(chip_id);
        let tx = self.packet_txs.get_mut(&chip_id).expect("Chip not found or already closed");
        tx.send(Bytes::copy_from_slice(packet)).await.expect("Failed to send packet");
    }

    pub async fn then_packet_is_received(&mut self, chip_id: u32) -> Vec<u8> {
        let chip_id = ChipId(chip_id);
        let rx = self.packet_rxs.get_mut(&chip_id).expect("Chip not found or already closed");
        tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("Timed out waiting for packet")
            .expect("Packet stream closed unexpectedly")
    }

    pub async fn then_chip_exists(&self, chip_id: u32) {
        self.when_get_chip(chip_id).await.expect("Chip should exist");
    }

    pub async fn given_a_chip(&mut self, chip_id: u32) {
        self.when_create_chip(chip_id).await.expect("GIVEN: Failed to create chip");
    }

    pub async fn when_chip_is_reset(&self, chip_id: u32) {
        self.client.reset(ChipId(chip_id)).await.expect("WHEN: Failed to reset chip");
    }

    pub async fn then_reset_response_is_received(&mut self, chip_id: u32, status: uci::Status) {
        let packet = self.then_packet_is_received(chip_id).await;
        let (rsp, _) = uci::CoreDeviceResetRsp::decode(&packet).expect("reset response");
        assert_eq!(rsp.status, status);
    }

    pub async fn then_status_notification_is_received(
        &mut self,
        chip_id: u32,
        state: uci::DeviceState,
    ) {
        let packet = self.then_packet_is_received(chip_id).await;
        let (ntf, _) = uci::CoreDeviceStatusNtf::decode(&packet).expect("status notification");
        assert_eq!(ntf.device_state, state);
    }

    pub async fn when_uci_is_reset(&mut self, chip_id: u32) {
        let reset_cmd = uci::CoreDeviceResetCmd { reset_config: uci::ResetConfig::UwbsReset };
        self.when_packet_is_sent(chip_id, &reset_cmd.encode_to_vec().unwrap()).await;

        let p1 = self.then_packet_is_received(chip_id).await;
        let (rsp, _) = uci::CoreDeviceResetRsp::decode(&p1).expect("reset response");
        assert_eq!(rsp.status, uci::Status::Ok);

        let p2 = self.then_packet_is_received(chip_id).await;
        let (ntf, _) = uci::CoreDeviceStatusNtf::decode(&p2).expect("status notification (reset)");
        assert_eq!(ntf.device_state, uci::DeviceState::DeviceStateReady);
    }

    pub async fn given_a_chip_at(&mut self, chip_id: u32, pos: netsim_model::Position) {
        self.given_a_chip(chip_id).await;
        self.client
            .update(
                ChipId(chip_id),
                ChipUpdate {
                    pose: netsim_model::PoseUpdate { position: Some(pos), orientation: None },
                    ..Default::default()
                },
            )
            .await
            .expect("Failed to set chip position");
    }

    pub async fn when_uci_session_is_established(
        &mut self,
        chip_id: u32,
        session_id: u32,
        device_type: uci::DeviceType,
        device_role: uci::DeviceRole,
        device_mac: [u8; 2],
        peer_mac: [u8; 2],
    ) {
        // 1. Session Init
        let init_cmd =
            uci::SessionInitCmd { session_id, session_type: uci::SessionType::FiraRangingSession };
        self.when_packet_is_sent(chip_id, &init_cmd.encode_to_vec().unwrap()).await;

        let p1 = self.then_packet_is_received(chip_id).await;
        let (init_rsp, _) = uci::SessionInitRsp::decode(&p1).expect("session init response");
        assert_eq!(init_rsp.status, uci::Status::Ok);

        let p2 = self.then_packet_is_received(chip_id).await;
        let (status_ntf, _) =
            uci::SessionStatusNtf::decode(&p2).expect("session status notification (init)");
        assert_eq!(status_ntf.session_state, uci::SessionState::SessionStateInit);

        // 2. Set App Config
        let tlvs = vec![
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::DeviceType,
                v: vec![device_type as u8],
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::DeviceRole,
                v: vec![device_role as u8],
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::DeviceMacAddress,
                v: device_mac.to_vec(),
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::DstMacAddress,
                v: peer_mac.to_vec(),
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::MultiNodeMode,
                v: vec![uci::MultiNodeMode::OneToOne as u8],
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::RangingRoundUsage,
                v: vec![uci::RangingRoundUsage::DsTwrDeferredMode as u8],
            },
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::ScheduleMode,
                v: vec![uci::ScheduleMode::TimeScheduled as u8],
            },
            // Prevents additional measurements from being triggered
            uci::AppConfigTlv {
                cfg_id: uci::AppConfigTlvType::RangingDuration,
                v: u32::MAX.to_le_bytes().to_vec(),
            },
        ];
        let config_cmd = uci::SessionSetAppConfigCmd { session_token: session_id, tlvs };
        self.when_packet_is_sent(chip_id, &config_cmd.encode_to_vec().unwrap()).await;

        let p3 = self.then_packet_is_received(chip_id).await;
        let (config_rsp, _) =
            uci::SessionSetAppConfigRsp::decode(&p3).expect("session set app config response");
        assert_eq!(config_rsp.status, uci::Status::Ok);

        let p4 = self.then_packet_is_received(chip_id).await;
        let (status_ntf2, _) =
            uci::SessionStatusNtf::decode(&p4).expect("session status notification (config)");
        assert_eq!(status_ntf2.session_state, uci::SessionState::SessionStateIdle);
    }

    pub async fn when_ranging_is_started(&mut self, chip_id: u32, session_id: u32) {
        let start_cmd = uci::SessionStartCmd { session_id };
        self.when_packet_is_sent(chip_id, &start_cmd.encode_to_vec().unwrap()).await;

        let p1 = self.then_packet_is_received(chip_id).await;
        let (start_rsp, _) = uci::SessionStartRsp::decode(&p1).expect("session start response");
        assert_eq!(start_rsp.status, uci::Status::Ok);

        let p2 = self.then_packet_is_received(chip_id).await;
        let (status_ntf, _) =
            uci::SessionStatusNtf::decode(&p2).expect("session status notification (start)");
        assert_eq!(status_ntf.session_state, uci::SessionState::SessionStateActive);

        let p3 = self.then_packet_is_received(chip_id).await;
        let (device_status_ntf, _) =
            uci::CoreDeviceStatusNtf::decode(&p3).expect("device status notification (active)");
        assert_eq!(device_status_ntf.device_state, uci::DeviceState::DeviceStateActive);
    }

    pub async fn when_ranging_is_triggered(&mut self, chip_id: u32, session_id: u32) {
        self.client
            .start_ranging(ChipId(chip_id), session_id)
            .await
            .expect("WHEN: Failed to trigger ranging round");
    }

    pub async fn when_ranging_is_stopped(&mut self, chip_id: u32, session_id: u32) {
        self.client
            .stop_ranging(ChipId(chip_id), session_id)
            .await
            .expect("WHEN: Failed to stop ranging session");
    }

    pub async fn then_ranging_measurement_is_received(
        &mut self,
        chip_id: u32,
        expected_range: u16,
    ) {
        let packet = self.then_packet_is_received(chip_id).await;
        let (ntf, _) =
            uci::ShortMacTwoWaySessionInfoNtf::decode(&packet).expect("ranging info notification");

        assert_eq!(ntf.two_way_ranging_measurements.len(), 1, "Expected exactly one measurement");
        let tolerance = (6.0 * (expected_range as f32 / 500.0).max(1.0)).ceil() as u16;
        assert!(
            (expected_range.saturating_sub(tolerance)..=expected_range.saturating_add(tolerance))
                .contains(&ntf.two_way_ranging_measurements[0].distance)
        );
    }

    pub async fn when_create_anchor(
        &mut self,
        chip_id: u32,
        address: Option<&str>,
        pos: Position,
    ) -> Result<(), ClientError> {
        let id = ChipId(chip_id);
        let mut chip =
            Chip::new_test_uwb_anchor(format!("uwb_anchor_{id}"), address.map(str::to_owned));
        chip.id = id.0;
        chip.device_id = DeviceId(1);
        chip.pose.position = pos;

        let params = ChipCreate { packet_stream: None, packet_sink: None, chip };
        self.client.create(id, params).await
    }

    pub async fn given_an_anchor_at(&mut self, chip_id: u32, address: Option<&str>, pos: Position) {
        self.when_create_anchor(chip_id, address, pos)
            .await
            .expect("GIVEN: Failed to create anchor");
    }

    pub async fn then_single_ranging_measurement_is_received(
        &mut self,
        chip_id: u32,
    ) -> ShortAddressTwoWayRangingMeasurement {
        let packet = self.then_packet_is_received(chip_id).await;
        let (ntf, _) =
            uci::ShortMacTwoWaySessionInfoNtf::decode(&packet).expect("ranging info notification");

        assert_eq!(ntf.two_way_ranging_measurements.len(), 1, "Expected exactly one measurement");
        ntf.two_way_ranging_measurements[0].clone()
    }
}
