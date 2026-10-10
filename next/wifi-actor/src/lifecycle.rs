// Copyright 2025 The Android Open Source Project
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

use actor_framework::{ActorLifecycle, DynContext};
use netsim_model::ChipId;
use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{error, info};

use crate::wifi_actor::WifiActor;

/// ID for the Slirp infrastructure stream
pub(crate) const SLIRP_ID: ChipId = ChipId(u32::MAX - 1);

/// Stream ID for AP downlink messages in the typed_stream map
const AP_SUBSCRIPTION_ID: usize = 0;

/// Stream ID for mDNS packets from host
const MDNS_SUBSCRIPTION_ID: usize = 1;

impl ActorLifecycle for WifiActor {
    async fn on_start(&mut self, ctx: &mut DynContext<Self>) {
        info!("WifiActor started");

        if let Some(ap_client) = &self.ap_client {
            // Uplink: Wifi -> AP (Wifi writes to tx, AP reads from rx)
            let (ap_uplink_tx, ap_uplink_rx) = create_channel_stream();

            // Downlink: AP -> Wifi (AP writes to tx, Wifi reads from rx)
            let (ap_downlink_tx, ap_downlink_rx) = create_channel_stream();

            ctx.add_typed_stream(AP_SUBSCRIPTION_ID, Box::pin(ap_downlink_rx));

            // Set initial beacon interval to 100ms
            if let Err(e) = ap_client
                .register(
                    Box::pin(ap_uplink_rx),
                    ap_downlink_tx,
                    self.shared_keys.clone(),
                    std::time::Duration::from_millis(100),
                )
                .await
            {
                error!("Failed to register with AP client: {}", e);
            }
            self.to_ap = Some(ap_uplink_tx);
        }
        if self.forward_host_mdns {
            let (mdns_tx, mdns_rx) = create_channel_stream();
            ctx.add_typed_stream(MDNS_SUBSCRIPTION_ID, Box::pin(mdns_rx));

            tokio::spawn(async move {
                if let Err(e) = crate::mdns_forwarder::run_mdns_forwarder(mdns_tx).await {
                    tracing::error!("mDNS Forwarder failed: {}", e);
                }
            });
        }

        // Initialize Gateway
        self.gateway.on_start(ctx).await;
    }

    async fn on_shutdown(&mut self) {
        info!("WifiActor stopped");
    }

    async fn on_stream(
        &mut self,
        chip_id: Self::Id,
        packet: bytes::Bytes,
        _ctx: &mut DynContext<Self>,
    ) {
        if self.gateway.should_handle(chip_id) {
            self.gateway.handle_incoming(
                chip_id,
                packet,
                &mut self.medium,
                &self.shared_keys,
                &mut self.out_queue,
            );
            self.flush_out_queue();
        } else {
            self.process_guest_packet(chip_id.0, packet).await;
        }
    }

    async fn on_stream_closed(&mut self, id: Self::Id, ctx: &mut DynContext<Self>) {
        info!("Stream closed for chip {id}");
        ctx.abort(id);
        if let Err(e) = self.handle_delete_impl(id, ctx).await {
            error!("Failed to delete chip {id} after stream closed: {e}");
        }
    }

    async fn on_task_closed(&mut self, id: Self::Id, ctx: &mut DynContext<Self>) {
        info!("Sink task closed for chip {id}");
        ctx.remove_stream(id);
        if let Err(e) = self.handle_delete_impl(id, ctx).await {
            error!("Failed to delete chip {id} after sink task closed: {e}");
        }
    }

    async fn on_typed_stream(
        &mut self,
        id: usize,
        packet: bytes::Bytes,
        _ctx: &mut DynContext<Self>,
    ) {
        if id == AP_SUBSCRIPTION_ID || id == MDNS_SUBSCRIPTION_ID {
            self.process_ap_packet(packet);
        }
    }
}

/// Creates an unbounded channel and wraps the receiver in a pinned BoxStream.
fn create_channel_stream<T>() -> (UnboundedSender<T>, UnboundedReceiverStream<T>)
where
    T: Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (tx, UnboundedReceiverStream::new(rx))
}
