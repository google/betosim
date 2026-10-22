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

use std::sync::atomic::{AtomicU16, Ordering};

use actor_framework::DynContext;
use ap_actor::SharedKeyStore;
use netsim_model::ChipId;
use netsim_packets::{FrameDirection, Ieee80211};
use slirp_actor::SlirpClient;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

use crate::{
    gateway::GatewayTrait,
    lifecycle::SLIRP_ID,
    medium::Medium,
    wifi_actor::{SlirpPendingRequest, WifiActor},
};

/// Gateway implementation for User-Mode Networking (Slirp).
///
/// This is the *default* gateway when no TAP interface is configured.
/// It routes traffic to the `SlirpActor`, which wraps `libslirp` to provide
/// network connectivity without requiring special privileges or kernel
/// interfaces.
#[derive(Debug)]
pub struct SlirpGateway {
    sender: UnboundedSender<bytes::Bytes>,
    pending_channels: Option<SlirpPendingRequest>,
    client: Option<SlirpClient>,
    seq: AtomicU16,
}

impl SlirpGateway {
    pub fn new(client: Option<SlirpClient>) -> Self {
        let (uplink_tx, uplink_rx) = tokio::sync::mpsc::unbounded_channel();
        let (downlink_tx, downlink_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            sender: uplink_tx,
            pending_channels: Some((uplink_rx, downlink_tx, downlink_rx)),
            client,
            seq: AtomicU16::new(100),
        }
    }
}

#[async_trait::async_trait]
impl GatewayTrait for SlirpGateway {
    async fn send_80211(
        &self,
        _chip_id: ChipId,
        ieee80211: &Ieee80211,
    ) -> Result<usize, crate::error::WifiError> {
        // Drop QosNodata frames (keep-alives/null data) as they contain no payload
        // and cannot be converted to Ethernet.
        if ieee80211.is_qos_nodata() {
            return Ok(0);
        }

        ieee80211
            .to_ieee8023()
            .map_err(|e| {
                let fc = ieee80211.get_fc();
                let ftype = ieee80211.is_data();
                let stype = ieee80211.stype();
                crate::error::WifiError::Frame(Box::from(format!(
                    "Slirp conversion failed: {}. Frame (Data: {}), Subtype: {}, FC: {:#06x}",
                    e, ftype, stype, fc
                )))
            })
            .and_then(|eth| {
                let payload_len = eth.len().saturating_sub(crate::gateway::ETHERNET_HEADER_LEN);
                self.sender.send(bytes::Bytes::from(eth)).map(|_| payload_len).map_err(|e| {
                    crate::error::WifiError::Transmission(Box::from(format!(
                        "Slirp send failed: {}",
                        e
                    )))
                })
            })
    }

    fn should_handle(&self, chip_id: ChipId) -> bool {
        chip_id == crate::lifecycle::SLIRP_ID
    }

    fn handle_incoming(
        &self,
        _chip_id: ChipId,
        packet: bytes::Bytes,
        medium: &mut Medium,
        shared_keys: &SharedKeyStore,
        out_queue: &mut Vec<(u32, bytes::Bytes)>,
    ) {
        debug!("SLIRP_PKT: len {}", packet.len());
        medium.wifi_stats.incr_network_packets_rx();

        // Extract the original destination MAC (which is the station)
        let Some(dest_mac_bytes) = packet.first_chunk::<6>() else {
            warn!("SLIRP_PKT: packet too short ({} bytes)", packet.len());
            return;
        };
        let dest_mac = netsim_packets::MacAddress::new(*dest_mac_bytes);

        medium.wifi_stats.record_download_bytes(
            packet.len().saturating_sub(crate::gateway::ETHERNET_HEADER_LEN),
        );

        if dest_mac.is_broadcast() || dest_mac.is_multicast() {
            let bssids = shared_keys.bssids.read().clone();
            for bssid in bssids {
                let seq = self.seq.fetch_add(1, Ordering::Relaxed);
                if let Ok(ieee80211) = netsim_packets::Ieee80211::from_ieee8023_qos(
                    &packet,
                    bssid,
                    FrameDirection::FromAp,
                    true,
                    seq,
                ) && let Ok(bytes) = ieee80211.encode_to_vec()
                {
                    let _ = medium.transmit_from_infra(&bytes::Bytes::from(bytes), out_queue);
                }
            }
            return;
        }

        let Some(bssid) = shared_keys.get_station_bssid(&dest_mac) else {
            tracing::warn!("Dropping unicast packet to unknown station {}", dest_mac);
            return;
        };

        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let ieee80211 = match netsim_packets::Ieee80211::from_ieee8023_qos(
            &packet,
            bssid,
            FrameDirection::FromAp,
            true,
            seq,
        ) {
            Ok(frame) => frame,
            Err(e) => {
                medium
                    .wifi_stats
                    .log_and_incr_err_count(&crate::error::WifiError::Frame(Box::from(e)));
                return;
            }
        };

        let res = ieee80211.encode_to_vec().map_err(|_| {
            crate::error::WifiError::Frame("Failed to encode Slirp packet to 802.11".into())
        });
        match res {
            Ok(bytes) => {
                let _ = medium.transmit_from_infra(&bytes::Bytes::from(bytes), out_queue);
            }
            Err(_) => {
                medium.wifi_stats.log_and_incr_err_count(&crate::error::WifiError::Frame(
                    Box::from("Failed to encode Slirp packet to 802.11"),
                ));
            }
        }
    }

    async fn on_start(&mut self, ctx: &mut DynContext<WifiActor>) {
        if let Some(client) = self.client.as_ref()
            && let Some((uplink_rx, downlink_tx, downlink_rx)) = self.pending_channels.take()
        {
            debug!("Registering Slirp channels in on_start");
            // Register with SlirpActor
            let stream = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(uplink_rx));
            let sink: netsim_model::PacketSink =
                Box::pin(futures::sink::unfold(downlink_tx, |tx, bytes| async move {
                    let _ = tx.send(bytes);
                    Ok(tx)
                }));

            if let Err(error) = client.register(SLIRP_ID.0, stream, sink, None, false).await {
                warn!(%error, "Failed to register with SlirpActor");
            }

            // Add downlink stream to context
            ctx.add_stream(
                SLIRP_ID,
                Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(downlink_rx)),
            );
        }
    }

    async fn on_chip_create(&mut self, _chip_id: ChipId, _ctx: &mut DynContext<WifiActor>) {
        // No-op for Slirp
    }

    async fn on_chip_remove(&mut self, _chip_id: ChipId, _ctx: &mut DynContext<WifiActor>) {}

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
