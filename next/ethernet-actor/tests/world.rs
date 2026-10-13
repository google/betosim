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

use std::{collections::HashMap, time::Duration};

use actor_framework::MockActorClient;
use bytes::Bytes;
use device_actor::{DeviceActor, DeviceClient};
use device_api::{DeviceAction, DeviceActionResult};
use ethernet_actor::{EthernetActor, EthernetClient};
use netsim_model::{
    Chip, ChipClient, ChipCreate, ChipError, ChipId, ChipKind, ChipUpdate, ChipVariant,
    ChipVariantUpdate, ClientError, DeviceId, EthernetUpdate, NetsimRadioStats, RadioUpdate,
};
use slirp_actor::{SlirpActor, SlirpBackend, SlirpClient};
use tokio::{sync::mpsc, task::JoinHandle, time::timeout};
use tokio_stream::wrappers::ReceiverStream;

const RECV_TIMEOUT: Duration = Duration::from_secs(5);
const QUIET_PERIOD: Duration = Duration::from_millis(500);

const DEVICE_ID: DeviceId = DeviceId(7);
const UNLEARNED_MAC: [u8; 6] = [0x02, 0x15, 0xb2, 0x00, 0x00, 0x99];
const UNLEARNED_IP: [u8; 4] = [10, 0, 2, 99];
pub(crate) const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];

const ETHERTYPE_ARP: [u8; 2] = [0x08, 0x06];
const ARP_OP_REQUEST: [u8; 2] = [0x00, 0x01];
const ARP_OP_REPLY: [u8; 2] = [0x00, 0x02];

struct Guest {
    mac: [u8; 6],
    ip: [u8; 4],
    uplink: mpsc::Sender<Bytes>,
    downlink: mpsc::Receiver<Bytes>,
}

impl Guest {
    async fn send_arp_request(&self, target_ip: [u8; 4]) {
        let _ = self.uplink.send(arp_request(self.mac, self.ip, target_ip)).await;
    }

    async fn expect_arp_reply_to(&mut self, dst_mac: [u8; 6], sender_ip: [u8; 4]) {
        let mac = self.mac;
        let wait = async {
            while let Some(frame) = self.downlink.recv().await {
                if is_arp_reply_to(&frame, dst_mac, sender_ip) {
                    return true;
                }
            }
            false
        };
        let received = timeout(RECV_TIMEOUT, wait).await.unwrap_or(false);
        assert!(
            received,
            "guest {mac:02x?} received no ARP reply to {dst_mac:02x?} from {sender_ip:?}"
        );
    }

    async fn assert_no_arp_reply_from(&mut self, sender_ip: [u8; 4]) {
        let mac = self.mac;
        let wait = async {
            while let Some(frame) = self.downlink.recv().await {
                if is_arp_reply_from(&frame, sender_ip) {
                    return true;
                }
            }
            false
        };
        let received = timeout(QUIET_PERIOD, wait).await.unwrap_or(false);
        assert!(!received, "guest {mac:02x?} unexpectedly received ARP reply from {sender_ip:?}");
    }
}

pub(crate) struct World {
    client: EthernetClient,
    slirp_client: SlirpClient,
    device_actions: mpsc::UnboundedReceiver<DeviceAction>,
    guests: HashMap<ChipId, Guest>,
    actor_handles: Vec<JoinHandle<()>>,
    last_error: Option<ClientError>,
    last_reset_chip: Option<Chip>,
    last_stats: Option<Box<[NetsimRadioStats]>>,
    last_global_stats: Option<Option<Vec<u8>>>,
}

impl Drop for World {
    fn drop(&mut self) {
        for handle in &self.actor_handles {
            handle.abort();
        }
    }
}

impl World {
    pub(crate) async fn new() -> Self {
        Self::with_slirp_backend(SlirpBackend::default()).await
    }

    pub(crate) async fn with_slirp_backend(backend: SlirpBackend) -> Self {
        let slirp = SlirpActor::new_with_backend(Default::default(), None, None, backend).await;
        let (slirp_runner, slirp_client) = slirp_actor::new();
        let slirp_handle = tokio::spawn(slirp_runner.run(slirp));

        let (action_tx, device_actions) = mpsc::unbounded_channel();
        let device_client = DeviceClient::new(Box::new(recording_device_client(action_tx)));

        let (runner, client) = ethernet_actor::new();
        let eth_handle =
            tokio::spawn(runner.run(EthernetActor::new(slirp_client.clone(), device_client)));

        Self {
            client,
            slirp_client,
            device_actions,
            guests: HashMap::new(),
            actor_handles: vec![slirp_handle, eth_handle],
            last_error: None,
            last_reset_chip: None,
            last_stats: None,
            last_global_stats: None,
        }
    }

    pub(crate) async fn given_chip(&mut self, id: u32, kind: ChipKind) -> &mut Self {
        let params = ChipCreate { packet_stream: None, packet_sink: None, chip: chip(kind) };
        self.client.create(ChipId(id), params).await.expect("given_chip create");
        self
    }

    pub(crate) async fn given_guest_chip(
        &mut self,
        id: u32,
        kind: ChipKind,
        mac: [u8; 6],
        ip: [u8; 4],
    ) -> &mut Self {
        let (uplink, uplink_rx) = mpsc::channel(32);
        let (downlink_tx, downlink) = mpsc::channel(32);
        let packet_sink =
            Box::pin(futures::sink::unfold(downlink_tx, |tx, frame: Bytes| async move {
                let _ = tx.send(frame).await;
                Ok::<_, std::io::Error>(tx)
            }));
        let params = ChipCreate {
            packet_stream: Some(Box::new(ReceiverStream::new(uplink_rx))),
            packet_sink: Some(packet_sink),
            chip: chip(kind),
        };
        self.client.create(ChipId(id), params).await.expect("given_guest_chip create");
        self.guests.insert(ChipId(id), Guest { mac, ip, uplink, downlink });
        self
    }

    pub(crate) async fn when_create_chip(&mut self, id: u32, kind: ChipKind) -> &mut Self {
        let params = ChipCreate { packet_stream: None, packet_sink: None, chip: chip(kind) };
        self.last_error = self.client.create(ChipId(id), params).await.err();
        self
    }

    pub(crate) async fn when_read_chip(&mut self, id: u32) -> &mut Self {
        self.last_error = self.client.read(ChipId(id)).await.err();
        self
    }

    pub(crate) async fn when_update_chip(
        &mut self,
        id: u32,
        name: Option<&str>,
        enabled: Option<bool>,
        radio_state: Option<bool>,
    ) -> &mut Self {
        let patch = ChipUpdate {
            name: name.map(ToString::to_string),
            enabled,
            variant: radio_state.map(|state| {
                ChipVariantUpdate::Ethernet(EthernetUpdate {
                    radio: RadioUpdate { state: Some(state) },
                })
            }),
            ..Default::default()
        };
        self.last_error = self.client.update(ChipId(id), patch).await.err();
        self
    }

    pub(crate) async fn when_reset_chip(&mut self, id: u32) -> &mut Self {
        match self.client.reset(ChipId(id)).await {
            Ok(chip) => {
                self.last_reset_chip = Some(chip);
                self.last_error = None;
            }
            Err(err) => {
                self.last_reset_chip = None;
                self.last_error = Some(err);
            }
        }
        self
    }

    pub(crate) async fn when_delete_chip(&mut self, id: u32) -> &mut Self {
        self.last_error = self.client.delete(ChipId(id)).await.err();
        self
    }

    pub(crate) async fn when_read_statistics(&mut self) -> &mut Self {
        match self.client.read_statistics().await {
            Ok(stats) => {
                self.last_stats = Some(stats);
                self.last_error = None;
            }
            Err(err) => {
                self.last_stats = None;
                self.last_error = Some(err);
            }
        }
        self
    }

    pub(crate) async fn when_get_global_stats(&mut self) -> &mut Self {
        match self.client.get_global_stats().await {
            Ok(stats) => {
                self.last_global_stats = Some(stats);
                self.last_error = None;
            }
            Err(err) => {
                self.last_global_stats = None;
                self.last_error = Some(err);
            }
        }
        self
    }

    pub(crate) async fn when_shutdown(&mut self) -> &mut Self {
        self.last_error = self.client.shutdown().await.err();
        self
    }

    pub(crate) async fn when_guest_sends_arp_request(
        &mut self,
        id: u32,
        target_ip: [u8; 4],
    ) -> &mut Self {
        let guest = self.guests.get(&ChipId(id)).expect("guest chip not found");
        guest.send_arp_request(target_ip).await;
        self
    }

    pub(crate) async fn when_unlearned_host_arps_gateway(&mut self) -> &mut Self {
        self.slirp_client.send_packet(arp_request(UNLEARNED_MAC, UNLEARNED_IP, GATEWAY_IP)).await;
        self
    }

    pub(crate) async fn when_guest_disconnects(&mut self, id: u32) -> &mut Self {
        self.guests.remove(&ChipId(id));
        self
    }

    pub(crate) async fn then_chip_count_is(&self, expected: usize) -> &Self {
        let count = self.client.read_count_for_testing().await.expect("read chip count");
        assert_eq!(count, expected);
        self
    }

    pub(crate) async fn then_chip_matches(
        &self,
        id: u32,
        kind: ChipKind,
        name: &str,
        enabled: bool,
        radio_state: Option<bool>,
    ) -> &Self {
        let chip = self.client.read(ChipId(id)).await.expect("read chip");
        assert_eq!(chip.id, id);
        assert_eq!(chip.kind, kind);
        assert_eq!(chip.name, name);
        assert_eq!(chip.enabled, enabled);
        assert_eq!(chip.device_id, DEVICE_ID);
        match (&chip.variant, kind) {
            (Some(ChipVariant::Ethernet(ethernet)), ChipKind::ETHERNET) => {
                assert_eq!(ethernet.radio.state, radio_state);
            }
            (Some(ChipVariant::CellularData(cellular)), ChipKind::CELLULAR_DATA) => {
                assert_eq!(cellular.radio.state, radio_state);
            }
            (other, _) => panic!("unexpected variant {other:?} for kind {kind:?}"),
        }
        self
    }

    pub(crate) fn then_last_reset_chip_matches(
        &self,
        id: u32,
        kind: ChipKind,
        name: &str,
        enabled: bool,
    ) -> &Self {
        let chip = self.last_reset_chip.as_ref().expect("expected last_reset_chip to be set");
        assert_eq!(chip.id, id);
        assert_eq!(chip.kind, kind);
        assert_eq!(chip.name, name);
        assert_eq!(chip.enabled, enabled);
        self
    }

    pub(crate) async fn then_chip_not_found(&self, id: u32) -> &Self {
        let result = self.client.read(ChipId(id)).await;
        assert!(
            matches!(
                result,
                Err(ClientError::Chip(ChipError::ChipNotFound(ChipId(found_id)))) if found_id == id
            ),
            "expected ChipNotFound({id}), got {result:?}"
        );
        self
    }

    pub(crate) fn then_last_error_is_chip_not_found(&self, id: u32) -> &Self {
        assert!(
            matches!(
                self.last_error,
                Some(ClientError::Chip(ChipError::ChipNotFound(ChipId(found_id)))) if found_id == id
            ),
            "expected ChipNotFound({id}), got {:?}",
            self.last_error
        );
        self
    }

    pub(crate) fn then_last_error_contains(&self, substring: &str) -> &Self {
        let err = self.last_error.as_ref().expect("expected an error, but operation succeeded");
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains(&substring.to_lowercase()),
            "expected error message to contain '{substring}', got '{msg}'"
        );
        self
    }

    pub(crate) fn then_last_operation_succeeded(&self) -> &Self {
        assert!(self.last_error.is_none(), "unexpected last error: {:?}", self.last_error);
        self
    }

    pub(crate) async fn then_device_notified_chip_removed(
        &mut self,
        expected_id: u32,
    ) -> &mut Self {
        let action = timeout(RECV_TIMEOUT, self.device_actions.recv())
            .await
            .expect("timed out waiting for a DeviceAction")
            .expect("device action channel closed");
        match action {
            DeviceAction::NotifyChipRemoved(device_id, chip_id) => {
                assert_eq!(device_id, DEVICE_ID);
                assert_eq!(chip_id, ChipId(expected_id));
            }
            other => panic!("expected NotifyChipRemoved, got {other:?}"),
        }
        self
    }

    pub(crate) async fn then_no_device_notification(&mut self) -> &mut Self {
        if let Ok(Some(action)) = timeout(QUIET_PERIOD, self.device_actions.recv()).await {
            panic!("unexpected DeviceAction: {action:?}");
        }
        self
    }

    pub(crate) async fn then_guest_receives_arp_reply(
        &mut self,
        id: u32,
        sender_ip: [u8; 4],
    ) -> &mut Self {
        let guest = self.guests.get_mut(&ChipId(id)).expect("guest not found");
        let dst_mac = guest.mac;
        guest.expect_arp_reply_to(dst_mac, sender_ip).await;
        self
    }

    pub(crate) async fn then_guest_receives_flooded_arp_reply(
        &mut self,
        id: u32,
        sender_ip: [u8; 4],
    ) -> &mut Self {
        let guest = self.guests.get_mut(&ChipId(id)).expect("guest not found");
        guest.expect_arp_reply_to(UNLEARNED_MAC, sender_ip).await;
        self
    }

    pub(crate) async fn then_guest_receives_no_arp_reply(
        &mut self,
        id: u32,
        sender_ip: [u8; 4],
    ) -> &mut Self {
        let guest = self.guests.get_mut(&ChipId(id)).expect("guest not found");
        guest.assert_no_arp_reply_from(sender_ip).await;
        self
    }

    pub(crate) fn then_statistics_are_empty(&self) -> &Self {
        let stats = self.last_stats.as_ref().expect("statistics not loaded");
        assert!(stats.is_empty(), "expected empty statistics, got {stats:?}");
        self
    }

    pub(crate) fn then_global_stats_reported(&self) -> &Self {
        assert!(self.last_global_stats.is_some(), "expected global stats result");
        self
    }
}

fn recording_device_client(
    actions: mpsc::UnboundedSender<DeviceAction>,
) -> MockActorClient<DeviceActor> {
    let mut mock = MockActorClient::<DeviceActor>::new();
    let action_tx = actions.clone();
    mock.expect_perform_action().returning(move |_id, action| {
        let _ = action_tx.send(action);
        Ok(DeviceActionResult::Success)
    });
    mock.expect_clone_box().returning(move || Box::new(recording_device_client(actions.clone())));
    mock
}

fn chip(kind: ChipKind) -> Chip {
    Chip {
        kind,
        name: "eth0".to_string(),
        device_id: DEVICE_ID,
        variant: match kind {
            ChipKind::ETHERNET => Some(ChipVariant::Ethernet(Default::default())),
            ChipKind::CELLULAR_DATA => Some(ChipVariant::CellularData(Default::default())),
            _ => ChipVariant::try_from(kind).ok(),
        },
        ..Default::default()
    }
}

fn arp_request(sender_mac: [u8; 6], sender_ip: [u8; 4], target_ip: [u8; 4]) -> Bytes {
    let mut frame = Vec::with_capacity(42);
    frame.extend_from_slice(&[0xff; 6]);
    frame.extend_from_slice(&sender_mac);
    frame.extend_from_slice(&ETHERTYPE_ARP);
    frame.extend_from_slice(&[0x00, 0x01, 0x08, 0x00, 6, 4]);
    frame.extend_from_slice(&ARP_OP_REQUEST);
    frame.extend_from_slice(&sender_mac);
    frame.extend_from_slice(&sender_ip);
    frame.extend_from_slice(&[0; 6]);
    frame.extend_from_slice(&target_ip);
    Bytes::from(frame)
}

fn is_arp_reply_from(frame: &[u8], sender_ip: [u8; 4]) -> bool {
    frame.len() >= 42
        && frame[12..14] == ETHERTYPE_ARP
        && frame[20..22] == ARP_OP_REPLY
        && frame[28..32] == sender_ip
}

fn is_arp_reply_to(frame: &[u8], dst_mac: [u8; 6], sender_ip: [u8; 4]) -> bool {
    frame.len() >= 42 && frame[0..6] == dst_mac && is_arp_reply_from(frame, sender_ip)
}
