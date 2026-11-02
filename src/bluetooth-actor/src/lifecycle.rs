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

use std::time::Duration;

use actor_framework::{ActorLifecycle, ActorService, DynContext};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{error, info};

use crate::{BluetoothEvent, bluetooth_actor::BluetoothActor};

const ROOTCANAL_TICK_INTERVAL: Duration = Duration::from_millis(10);
const INTERNAL_EVENT_STREAM_ID: usize = 0;

impl ActorLifecycle for BluetoothActor {
    async fn on_start(&mut self, runtime: &mut DynContext<Self>) {
        runtime.set_interval(ROOTCANAL_TICK_INTERVAL);

        // Register internal event stream
        let rx = self.event_rx.lock().take();
        if let Some(rx) = rx {
            runtime.add_typed_stream(
                INTERNAL_EVENT_STREAM_ID,
                Box::pin(UnboundedReceiverStream::new(rx)),
            );
        }
    }

    async fn on_tick(&mut self, _runtime: &mut DynContext<Self>) {
        self.rootcanal.tick();
    }

    async fn on_stream(
        &mut self,
        id: netsim_model::ChipId,
        message: bytes::Bytes,
        _ctx: &mut DynContext<Self>,
    ) {
        if let Err(error) = self.rootcanal.receive_hci(id.0, message) {
            error!(%error, "Receive HCI error for chip {id}");
        }
    }

    async fn on_stream_closed(&mut self, id: netsim_model::ChipId, ctx: &mut DynContext<Self>) {
        info!("Stream closed for chip {id}");
        // If the stream closes, we should also ensure the sink task is aborted.
        ctx.abort(id);
        if let Err(error) = self.handle_delete(id, ctx).await {
            error!(%error, "Failed to delete chip {id} after stream closed");
        }
    }

    async fn on_task_closed(&mut self, id: netsim_model::ChipId, ctx: &mut DynContext<Self>) {
        info!("Sink task closed for chip {id}");
        // If the sink task closes, we should also ensure the stream is removed.
        ctx.remove_stream(id);
        if let Err(error) = self.handle_delete(id, ctx).await {
            error!(%error, "Failed to delete chip {id} after sink task closed");
        }
    }

    async fn on_typed_stream(
        &mut self,
        _id: usize,
        item: Self::TypedStream,
        _ctx: &mut DynContext<Self>,
    ) {
        match item {
            BluetoothEvent::DeliverPacket { receiver_id, packet, phy, rssi } => {
                self.rootcanal.deliver_packet(receiver_id.0, &packet, phy, rssi);
            }
        }
    }
}
