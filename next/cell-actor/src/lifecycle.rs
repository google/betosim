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

use actor_framework::{ActorLifecycle, ActorService, DynContext};
use bytes::Bytes;
use modem_rs::HostEvent;
use tracing::{error, info, warn};

use crate::cell_actor::CellActor;

impl ActorLifecycle for CellActor {
    async fn on_start(&mut self, ctx: &mut DynContext<Self>) {
        if let Some(rx) = self.event_receiver.take() {
            let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
            // Internal event stream subscription (ID 0)
            ctx.add_typed_stream(0, Box::pin(stream));
        }
    }

    async fn on_stream(&mut self, id: Self::Id, message: Bytes, ctx: &mut DynContext<Self>) {
        if message.is_empty() {
            return;
        }
        self.handle_packet_stream(id, message, ctx).await;
    }

    async fn on_stream_closed(&mut self, id: Self::Id, ctx: &mut DynContext<Self>) {
        info!("Stream closed for chip {}", id);
        use actor_framework::ActorService;
        let _ = self.handle_delete(id, ctx).await;
        ctx.abort(id);
    }

    async fn on_typed_stream(
        &mut self,
        id: usize,
        event: modem_rs::HostEvent,
        ctx: &mut DynContext<Self>,
    ) {
        if id == 0 {
            self.handle_host_event(event, ctx).await;
        }
    }
}

impl CellActor {
    async fn handle_packet_stream(
        &mut self,
        id: netsim_model::ChipId,
        message: Bytes,
        _ctx: &mut DynContext<Self>,
    ) {
        if let Err(e) = self.controller.send_data(id.0, &message) {
            error!("Failed to send data to controller for chip {}: {:?}", id, e);
        }
    }

    async fn handle_host_event(&mut self, event: HostEvent, ctx: &mut DynContext<Self>) {
        match event {
            HostEvent::SinkError(id) => {
                let chip_id = netsim_model::ChipId(id);
                warn!("Sink error for chip {}, deleting.", chip_id);
                let _ = self.handle_delete(chip_id, ctx).await;
                ctx.remove_stream(chip_id);
                ctx.abort(chip_id);
            }

            HostEvent::TimerRequest { chip_id: id, duration } => {
                ctx.run_later(
                    duration,
                    Box::new(move |actor, _ctx| {
                        if let Err(e) = actor.controller.on_timer(id) {
                            error!("Timer error for chip {}: {:?}", id, e);
                        }
                    }),
                );
            }
        }
    }
}
