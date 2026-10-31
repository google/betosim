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

use std::sync::atomic::Ordering;

use actor_framework::{ActorLifecycle, ActorService, DynContext};
use bytes::Bytes;
use netsim_model::ChipId;

use crate::{service::parse_and_count_uci_command, uwb_actor::UwbActor};

impl ActorLifecycle for UwbActor {
    async fn on_stream(&mut self, id: ChipId, message: Bytes, ctx: &mut DynContext<Self>) {
        let Some(&handle) = self.chip_to_handle.get(&id) else {
            return;
        };
        if let Some(state) = self.chip_states.read().get(&handle) {
            state.tx_count.fetch_add(1, Ordering::Relaxed);
        }
        parse_and_count_uci_command(&message, &self.uwb_stats);
        self.inject_uci(handle, &message, ctx);
    }

    async fn on_stream_closed(&mut self, id: ChipId, ctx: &mut DynContext<Self>) {
        let _ = self.handle_delete(id, ctx).await;
    }

    async fn on_task_closed(&mut self, id: ChipId, ctx: &mut DynContext<Self>) {
        let _ = self.handle_delete(id, ctx).await;
    }
}
