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

use actor_framework::{ActorLifecycle, ActorService, DynContext};
use futures::FutureExt;
use netsim_model::ChipId;
use pica::PicaEvent;
use tokio::sync::broadcast::error::TryRecvError;
use tracing::{error, info, warn};

use crate::uwb_actor::UwbActor;

const PICA_SENTINEL_CHIP_ID: ChipId = ChipId(u32::MAX);

impl ActorLifecycle for UwbActor {
    async fn on_start(&mut self, ctx: &mut DynContext<Self>) {
        info!("UwbActor starting Pica run loop");

        let pica = self.pica.take().expect("lifecycle starts only once");
        ctx.spawn(
            PICA_SENTINEL_CHIP_ID,
            async move {
                if let Err(err) = pica.run().await {
                    error!("Pica run loop failed: {err}");
                }
                PICA_SENTINEL_CHIP_ID
            }
            .boxed(),
        );
        ctx.set_interval(Self::TICK_INTERVAL);
    }

    async fn on_tick(&mut self, ctx: &mut DynContext<Self>) {
        loop {
            match self.pica_on_tick_events.try_recv() {
                Ok(PicaEvent::Disconnected { handle, .. }) => {
                    // Received in response to either `PicaCommand::Disconnect` or stream/sink
                    // closure.
                    let id =
                        self.chip_states.read().get(&handle).map(|state| ChipId(state.chip.id));
                    if let Some(id) = id {
                        let _ = self.handle_delete(id, ctx).await;
                    }
                }
                Ok(PicaEvent::Connected { .. }) => {}
                Err(TryRecvError::Lagged(skipped)) => {
                    warn!(
                        "UWB actor `on_tick` is too slow -- {skipped} messages were missed from Pica. There may be stale chips."
                    );
                    continue;
                }
                Err(TryRecvError::Empty | TryRecvError::Closed) => {
                    break;
                }
            }
        }
    }

    async fn on_shutdown(&mut self) {
        // Pica is running within the actor context and does not need to be
        // explicitly shut down.
    }
}
