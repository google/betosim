// Copyright 2025-2026 The Android Open Source Project
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
use netsim_packets::{AuthenticationFixedFields, Ieee80211, management_subtype};
use tracing::{debug, info, warn};
use zerocopy::FromBytes;

use crate::ap_actor::{ApActor, ApId, WIFI_STREAM_ID};

impl ActorLifecycle for ApActor {
    async fn on_start(&mut self, _ctx: &mut DynContext<Self>) {
        info!("ApActor started");
    }

    async fn on_tick(&mut self, ctx: &mut DynContext<Self>) {
        if let Some(sink) = &self.sink {
            let interval = self.beacon_interval.unwrap_or(200);
            let now = std::time::Instant::now();
            for ap in self.aps.values_mut() {
                if !ap.enabled {
                    continue;
                }

                // Process asynchronous delayed queues initially
                while let Some((time, _)) = ap.delayed_frames.front() {
                    if now >= *time {
                        if let Some((_, frame)) = ap.delayed_frames.pop_front()
                            && sink.send(frame).is_err()
                        {
                            warn!("Sink closed while transmitting delayed frame");
                            ctx.shutdown();
                            self.sink = None;
                            return;
                        }
                    } else {
                        break;
                    }
                }

                // Beacon generation logic
                if let Ok(frames) = self.manager.generate_beacon(ap, interval) {
                    for frame in frames {
                        if sink.send(frame).is_err() {
                            warn!("Sink closed, stopping ApActor");
                            ctx.shutdown();
                            self.sink = None;
                            return;
                        }
                    }
                }
            }
        }
    }

    // We expect stream messages (Mgmt frames or Data frames if bridged)
    async fn on_stream(&mut self, stream_id: ApId, msg: bytes::Bytes, ctx: &mut DynContext<Self>) {
        if stream_id.0 != WIFI_STREAM_ID {
            warn!("Received message on unknown stream_id: {}", stream_id);
            return;
        }
        let source_id = ChipId(0); // Placeholder until we lookup by MAC

        // Parse frame to get Destination Address (Addr1)
        let ieee_frame = match Ieee80211::decode(&msg) {
            Ok(frame) => frame,
            Err(_) => {
                warn!("Failed to parse IEEE 802.11 frame");
                return;
            }
        };

        let dest = ieee_frame.get_addr1();

        // If it's a Unicast Authentication frame (Sequence=1), we can assume the source
        // STA is exclusively attempting to connect to the target AP. We should
        // implicitly disconnect it from any *other* BSSIDs we currently track
        // to prevent stale state.
        if !dest.is_broadcast()
            && ieee_frame.stype() == management_subtype::AUTHENTICATION
            && let Some(auth_payload) = msg.get(24..)
            && let Ok((auth_fields, _)) = AuthenticationFixedFields::read_from_prefix(auth_payload)
            && auth_fields.sequence.get() == 1
        {
            let src = ieee_frame.get_source();
            let mut aps_to_clear = Vec::new();
            for (id, ap) in self.aps.iter() {
                if ap.config.bssid != dest && ap.associations.contains(&src) {
                    aps_to_clear.push(*id);
                }
            }
            for id in aps_to_clear {
                if let Some(ap) = self.aps.get_mut(&id) {
                    ap.clear_station_state(&src);
                    self.shared_keys.remove_session(&src);
                }
            }
        }

        // If broadcast, send to all APs
        if dest.is_broadcast() {
            if let Some(sink) = &self.sink {
                let interval = self.beacon_interval.unwrap_or(200);
                for ap in self.aps.values_mut() {
                    if let Ok(frames) = self.manager.handle_frame(
                        ap,
                        &msg,
                        &self.shared_keys,
                        interval,
                        source_id,
                        ctx,
                    ) {
                        for frame in frames {
                            let _ = sink.send(frame);
                        }
                    }
                }
            }
        } else {
            // Unicast - find matching AP by BSSID
            let mut handled = false;
            if let Some(sink) = &self.sink {
                let interval = self.beacon_interval.unwrap_or(200);
                for ap in self.aps.values_mut() {
                    if ap.config.bssid == dest {
                        if let Ok(frames) = self.manager.handle_frame(
                            ap,
                            &msg,
                            &self.shared_keys,
                            interval,
                            source_id,
                            ctx,
                        ) {
                            for frame in frames {
                                if sink.send(frame).is_err() {
                                    warn!("Sink closed, stopping ApActor");
                                    ctx.shutdown();
                                    self.sink = None;
                                    return;
                                }
                            }
                        }
                        handled = true;
                        break;
                    }
                }
            }

            if !handled {
                debug!("No AP found for unicast frame dest: {}", dest);
            }
        }
    }

    async fn on_stream_closed(&mut self, stream_id: ApId, ctx: &mut DynContext<Self>) {
        if stream_id.0 == WIFI_STREAM_ID {
            info!("WIFI_STREAM_ID closed, stopping ApActor");
            ctx.shutdown();
        }
    }
    async fn on_task_closed(&mut self, _id: ApId, _ctx: &mut DynContext<Self>) {}
}
