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

use actor_framework::{ActorService, DynContext};
use tracing::{error, info, warn};

use crate::{
    ap_actor::{ApActor, ApId, ApReq, ApResponse, ApState, WIFI_STREAM_ID},
    error::ApError,
};

// 1024 microseconds per Time Unit (TU)
const TU_INTERVAL_US: u128 = 1024;

// Reason Code 3: deauthenticated because sending STA is leaving (or has left) IBSS or ESS.
const DEAUTH_REASON_STA_LEAVING: u16 = 3;

impl ActorService for ApActor {
    type Id = ApId;
    type Create = crate::ap_actor::ApConfig;
    type Update = crate::ap_actor::ApActorUpdate;
    type Action = ApReq;
    type ActionResult = ApResponse;

    type Error = ApError;
    type Entity = ApState;
    type TypedStream = ();

    async fn handle_create(
        &mut self,
        id: Option<Self::Id>,
        params: Self::Create,
        _: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        let id_val = match id {
            Some(i) => {
                if i.0 >= self.next_ap_id {
                    self.next_ap_id = i.0 + 1;
                }
                i
            }
            None => {
                let i = self.next_ap_id;
                self.next_ap_id += 1;
                ApId(i)
            }
        };

        if self.aps.contains_key(&id_val) {
            return Err(ApError::ApAlreadyExists(id_val.0));
        }

        let state = ApState::new(id_val, params);
        self.shared_keys.add_bssid(state.config.bssid);
        self.aps.insert(id_val, state);

        info!("Created AP with ID: {}", id_val);
        Ok(id_val)
    }

    async fn handle_get(
        &self,
        id: Self::Id,
        _: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        Ok(self.aps.get(&id).cloned())
    }

    async fn handle_update(
        &mut self,
        id: Self::Id,
        patch: Self::Update,
        _: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        let ap_state = self.aps.get_mut(&id).ok_or(ApError::ApNotFound(id.0))?;

        if let Some(pos) = patch.position {
            ap_state.config.position = pos;
        }

        if let Some(ap_u) = patch.ap_update {
            if let Some(ssid) = ap_u.ssid {
                ap_state.config.ssid = ssid;
            }
            if let Some(channel) = ap_u.channel {
                ap_state.config.channel = channel;
            }
            for mac_str in ap_u.force_disconnect {
                // Find MAC for this string
                let mac = match mac_str.parse::<netsim_packets::MacAddr>() {
                    Ok(m) => m,
                    Err(_) => {
                        warn!("Invalid MAC address in force_disconnect: {}", mac_str);
                        continue;
                    }
                };

                if !ap_state.associations.remove(&mac) {
                    warn!("Requested force disconnect for unknown MAC: {}", mac);
                    continue;
                }

                // Also clear sessions if any
                if let Some(wpa) = &mut ap_state.wpa {
                    wpa.remove_session(&mac);
                }
                ap_state.sae_sessions.remove(&mac);
                ap_state.eap_sessions.remove(&mac);
                self.shared_keys.remove_session(&mac);
                self.shared_keys.remove_station_bssid(&mac);

                // Send Deauth Frame
                send_deauth(self.sink.as_ref(), &self.manager, ap_state, mac);
            }
        }

        if let Some(enabled) = patch.enabled {
            ap_state.enabled = enabled;
        }

        Ok(ap_state.clone())
    }

    async fn handle_delete(
        &mut self,
        id: Self::Id,
        _: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        if let Some(state) = self.aps.remove(&id) {
            let bssid = state.config.bssid;
            self.shared_keys.bssids.write().remove(&bssid);
            self.shared_keys.gtks.write().remove(&bssid);

            // Find and remove all stations associated with the deleted BSSID
            let mut station_bssids = self.shared_keys.station_bssids.write();
            let mut stations_to_remove = Vec::new();
            station_bssids.retain(|sta, v| {
                let matched = *v == bssid;
                if matched {
                    stations_to_remove.push(*sta);
                }
                !matched
            });

            // Remove their session keys
            let mut sessions = self.shared_keys.sessions.write();
            for sta in &stations_to_remove {
                sessions.remove(sta);
            }

            info!("Deleted AP with ID: {}", id);
        } else {
            warn!("Attempted to delete non-existent AP with ID: {}", id);
        }
        Ok(())
    }

    async fn handle_list(
        &mut self,
        _: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        Ok(self.aps.values().cloned().collect())
    }

    async fn handle_action(
        &mut self,
        id: Option<Self::Id>,
        action: Self::Action,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        match action {
            ApReq::Register { stream, sink, shared_keys, beacon_interval } => {
                if self.sink.is_some() {
                    return Err(ApError::Internal(
                        "ApActor: Register called more than once!".into(),
                    ));
                }
                info!("Registering AP singleton stream/sink with interval: {:?}", beacon_interval);
                self.sink = Some(sink);
                // Register new ApActor instances in the global shared keys context
                // No longer need Contextual Strangler because shared_keys supports HashSet.
                self.shared_keys = shared_keys;

                let tus = (beacon_interval.as_micros() / TU_INTERVAL_US) as u16;
                self.beacon_interval = Some(tus);

                let stream_id = ApId(WIFI_STREAM_ID);
                ctx.add_stream(stream_id, stream);

                ctx.set_interval(beacon_interval);
                Ok(ApResponse::Ok)
            }
            ApReq::Disconnect { mac } => {
                let id = id.ok_or_else(|| {
                    ApError::Internal("ApActor: Disconnect requires an ID".into())
                })?;
                let ap_state = self.aps.get_mut(&id).ok_or(ApError::ApNotFound(id.0))?;

                if ap_state.associations.remove(&mac) {
                    info!("ApActor: Force disconnecting MAC {} from AP {}", mac, id);

                    // Clear sessions if any
                    if let Some(wpa) = &mut ap_state.wpa {
                        wpa.remove_session(&mac);
                    }
                    ap_state.sae_sessions.remove(&mac);
                    ap_state.eap_sessions.remove(&mac);
                    self.shared_keys.remove_session(&mac);
                    self.shared_keys.remove_station_bssid(&mac);

                    // Send Deauth Frame
                    send_deauth(self.sink.as_ref(), &self.manager, ap_state, mac);
                } else {
                    warn!("ApActor: Requested disconnect for unknown MAC {} on AP {}", mac, id);
                }
                Ok(ApResponse::Ok)
            }
        }
    }
}

fn send_deauth(
    sink: Option<&tokio::sync::mpsc::UnboundedSender<bytes::Bytes>>,
    manager: &crate::ieee802_11::Ieee80211Manager,
    ap_state: &mut ApState,
    mac: netsim_packets::MacAddr,
) {
    if let Some(sink) = sink {
        let frame = manager.build_deauth_frame(ap_state, mac, DEAUTH_REASON_STA_LEAVING);
        if let Err(error) = sink.send(bytes::Bytes::from(frame)) {
            error!(%error, "Failed to send Deauth frame");
        }
    }
}
