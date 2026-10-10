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

use std::{collections::HashMap, sync::Arc};

use actor_framework::DynContext;
use ap_actor::{ApClient, SharedKeyStore};
use netsim_model::{Chip, ChipId, NetsimRadioStats};
use netsim_packets::Ieee80211;
use netsim_proto::stats::WifiIpcStats as ProtoWifiIpcStats;
use slirp_actor::SlirpClient;
use tokio::sync::mpsc::UnboundedSender;
#[cfg(not(target_os = "linux"))]
use tracing::warn;
use tracing::{debug, trace};

#[cfg(target_os = "linux")]
use crate::tap_gateway::TapGateway;
use crate::{
    error::WifiError,
    gateway::GatewayTrait,
    medium::{Medium, tx_packet_state::InfraTarget},
    slirp_gateway::SlirpGateway,
};

#[derive(Debug)]
pub enum WifiReq {
    GetStatistics,
    GetGlobalStats,
    Reset { id: ChipId },
}

/// Responses from the WifiActor (Output).
#[derive(Debug, Clone)]
pub enum WifiResponse {
    Ok,
    Statistics(Box<[NetsimRadioStats]>),
    GlobalStats(Box<ProtoWifiIpcStats>),
    Chip(Box<netsim_model::Chip>),
}

pub(crate) type SlirpPendingRequest = (
    tokio::sync::mpsc::UnboundedReceiver<bytes::Bytes>,
    tokio::sync::mpsc::UnboundedSender<bytes::Bytes>,
    tokio::sync::mpsc::UnboundedReceiver<bytes::Bytes>,
);

#[derive(Debug)]
pub struct WifiActor {
    pub(crate) ap_client: Option<Arc<ApClient>>,
    pub(crate) medium: Medium,
    pub(crate) active_chips: HashMap<ChipId, Chip>,
    pub(crate) initial_chips: HashMap<ChipId, Chip>,
    pub(crate) senders: HashMap<ChipId, UnboundedSender<bytes::Bytes>>,
    pub(crate) shared_keys: Arc<SharedKeyStore>,
    // Output buffer for Medium to avoid allocations
    pub(crate) out_queue: Vec<(u32, bytes::Bytes)>,
    pub(crate) device_client: device_actor::DeviceClient,
    // Channel to send frames TO the AP Actor (registered via ApClient)
    pub(crate) to_ap: Option<tokio::sync::mpsc::UnboundedSender<bytes::Bytes>>,
    // Gateway for Infra packets (Tap or Slirp)
    pub(crate) gateway: Box<dyn GatewayTrait>,
    pub(crate) forward_host_mdns: bool,
}

impl WifiActor {
    pub fn new(
        ap_client: Option<Arc<ApClient>>,
        slirp_client: Option<SlirpClient>,
        device_client: device_actor::DeviceClient,
        wifi_tap: Option<String>,
        shared_keys: Arc<SharedKeyStore>,
        clock: Arc<dyn crate::stats::Clock>,
        forward_host_mdns: bool,
    ) -> Self {
        // Fixup pending channels if we just created a SlirpGateway
        let gateway = if let Some(if_name) = wifi_tap {
            #[cfg(target_os = "linux")]
            {
                Box::new(TapGateway::new(if_name)) as Box<dyn GatewayTrait>
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = if_name;
                warn!("TAP Configured but not supported on this OS. Falling back to Slirp.");
                Box::new(SlirpGateway::new(slirp_client)) as Box<dyn GatewayTrait>
            }
        } else {
            // Default to SlirpGateway
            Box::new(SlirpGateway::new(slirp_client)) as Box<dyn GatewayTrait>
        };
        Self::new_with_gateway(
            ap_client,
            gateway,
            device_client,
            shared_keys,
            clock,
            forward_host_mdns,
        )
    }

    pub fn new_with_gateway(
        ap_client: Option<Arc<ApClient>>,
        gateway: Box<dyn GatewayTrait>,
        device_client: device_actor::DeviceClient,
        shared_keys: Arc<SharedKeyStore>,
        clock: Arc<dyn crate::stats::Clock>,
        forward_host_mdns: bool,
    ) -> Self {
        let medium = Medium::new(
            shared_keys.clone(),
            crate::stats::WifiStats::new(clock),
            Arc::new(crate::DebugArgs::default()),
        );

        Self {
            ap_client,
            medium,
            active_chips: HashMap::new(),
            initial_chips: HashMap::new(),
            senders: HashMap::new(),
            shared_keys,
            out_queue: Vec::new(),
            device_client,
            to_ap: None,
            gateway,
            forward_host_mdns,
        }
    }

    pub(crate) async fn handle_delete_impl(
        &mut self,
        id: ChipId,
        ctx: &mut DynContext<WifiActor>,
    ) -> Result<(), WifiError> {
        if let Some(chip) = self.active_chips.remove(&id) {
            self.senders.remove(&id);
            self.medium.remove(id.0);

            // Notify Gateway
            self.gateway.on_chip_remove(id, ctx).await;

            // Notify DeviceService
            let dc = self.device_client.clone();
            let device_id = chip.device_id;
            // Notify DeviceService asynchronously
            tokio::spawn(async move {
                let _ = dc.notify_chip_removed(device_id, id).await;
            });

            Ok(())
        } else {
            Err(WifiError::Internal(Box::from(format!("Chip {} not found", id))))
        }
    }

    pub(crate) fn flush_out_queue(&mut self) {
        for (chip_id, packet) in self.out_queue.drain(..) {
            if let Some(sender) = self.senders.get(&ChipId(chip_id)) {
                let _ = sender.send(packet);
            }
        }
    }

    #[allow(clippy::collapsible_if)]
    pub(crate) async fn process_guest_packet(&mut self, chip_id: u32, packet: bytes::Bytes) {
        trace!("WifiActor: Packet from Guest (Chip {}) len {}", chip_id, packet.len());

        // Fast path: Avoid full PDL decode overhead for standard data packets.
        // HwsimMsg format: NlMsgHdr (16 bytes) + HwsimMsgHdr (hwsim_cmd at offset 16).
        // HwsimCmd::StartPmsr relies on netsim_packets::HwsimCmd::StartPmsr as u8.
        if packet.len() >= 20
            && packet.get(16).copied() == Some(netsim_packets::HwsimCmd::StartPmsr as u8)
        {
            if let Ok(hwsim_msg) = netsim_packets::HwsimMsg::decode_full(&packet) {
                if hwsim_msg.hwsim_hdr.hwsim_cmd == netsim_packets::HwsimCmd::StartPmsr {
                    if let Some(resp) =
                        crate::pmsr::handle_start_pmsr(&hwsim_msg, chip_id, &self.active_chips)
                    {
                        self.out_queue.push((chip_id, resp));
                    }
                    self.flush_out_queue();
                    return;
                }
            }
        }

        match self.medium.resolve_tx_packet(chip_id, &packet) {
            Ok(tx_state) => {
                // 1. Ack
                let res = self.medium.ack_frame(chip_id, &tx_state.frame, &mut self.out_queue);
                self.medium.wifi_stats.log_outcome(res, |_, _| {});
                // 2. Infra (AP/Slirp) routing
                match tx_state.infra_target {
                    InfraTarget::Ap => {
                        debug!("ROUTING: Guest -> AP");
                        if let Some(to_ap) = &self.to_ap {
                            self.medium.wifi_stats.log_outcome(
                                to_ap.send(tx_state.get_ieee80211_bytes()).map_err(|e| {
                                    WifiError::Hostapd(Box::from(format!(
                                        "Failed to send to AP: {e}"
                                    )))
                                }),
                                |stats, _| stats.incr_hostapd_frames_tx(),
                            );
                        }
                    }
                    InfraTarget::Slirp => {
                        self.route_to_infra(chip_id, tx_state.get_ieee80211()).await
                    }
                    InfraTarget::None => {
                        if tx_state.stations {
                            // Check matching FTM Request
                            // TODO: Avoid parsing if possible, but we need to check Frame payload.
                            let frame_bytes = tx_state.get_ieee80211_bytes();
                            // Decode to check content
                            if let Ok(frame) = Ieee80211::decode(&frame_bytes) {
                                let da = frame.get_destination();
                                if let Some(peer_id) = self.medium.get_station_chip_id(&da) {
                                    // We found the target chip. Now get positions.
                                    // Initiator: chip_id
                                    // Responder: peer_id
                                    if let (Some(initiator), Some(responder)) = (
                                        self.active_chips.get(&ChipId(chip_id)),
                                        self.active_chips.get(&ChipId(peer_id)),
                                    ) {
                                        if let Some(responses) = crate::ftm::handle_ftm_request(
                                            &frame,
                                            &initiator.pose.position,
                                            &responder.pose.position,
                                        ) {
                                            debug!(
                                                "Simulated FTM Response from {} to {}",
                                                peer_id, chip_id
                                            );
                                            for resp in responses {
                                                self.out_queue.push((chip_id, resp));
                                            }
                                            // Suppress generic transmission.
                                            // Act as a Hardware Offload/Medium Interception to
                                            // ensure ONLY the simulated FTM response is sent.
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // 3. Stations (Loopback/Peers)
                if tx_state.stations {
                    let res = self.medium.transmit(
                        &tx_state.frame,
                        tx_state.get_ieee80211(),
                        &mut self.out_queue,
                    );
                    self.medium.wifi_stats.log_outcome(res, |_, _| {});
                }
            }
            Err(e) => {
                self.medium.wifi_stats.log_and_incr_err_count(&e);
            }
        }
        self.flush_out_queue();
    }

    pub(crate) fn process_ap_packet(&mut self, packet: bytes::Bytes) {
        trace!("AP_PKT: len {}", packet.len());
        self.medium.wifi_stats.incr_hostapd_frames_rx();
        if !packet.is_empty() {
            let res = self.medium.transmit_from_infra(&packet, &mut self.out_queue);
            self.medium.wifi_stats.log_outcome(res, |_, _| {});
        }
        self.flush_out_queue();
    }

    async fn route_to_infra(&mut self, chip_id: u32, ieee80211: &Ieee80211) {
        debug!("ROUTING: Guest -> Infra");
        self.medium.wifi_stats.log_outcome(
            self.gateway.send_80211(ChipId(chip_id), ieee80211).await,
            |stats, payload_len| {
                stats.incr_network_packets_tx();
                stats.record_upload_bytes(payload_len);
            },
        );
    }
}
