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

use actor_framework::ResourceClient;
use futures::TryFutureExt;
use netsim_model::{ClientError, Position};

use crate::ap_actor::{ApActor, ApConfig, ApId, ApReq, ApState};

/// Client for interacting with the Access Point Actor.
///
/// Wraps a `ResourceClient<ApActor>` and provides helper methods for
/// managing AP lifecycles and configuration.
#[derive(Clone)]
pub struct ApClient {
    pub client: ResourceClient<ApActor>,
    // Optional interceptor for capturing packets in tests
    #[cfg(feature = "testing")]
    interceptor: Option<
        std::sync::Arc<dyn Fn(&tokio::sync::mpsc::UnboundedSender<bytes::Bytes>) + Send + Sync>,
    >,
}

impl std::fmt::Debug for ApClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApClient").finish()
    }
}

impl ApClient {
    /// Create a new `ApClient` from a resource client.
    pub fn new(client: ResourceClient<ApActor>) -> Self {
        Self {
            client,
            #[cfg(feature = "testing")]
            interceptor: None,
        }
    }

    /// Create a new `ApClient` with a packet interceptor for testing.
    #[cfg(feature = "testing")]
    pub fn new_with_interceptor(
        client: ResourceClient<ApActor>,
        interceptor: impl Fn(&tokio::sync::mpsc::UnboundedSender<bytes::Bytes>) + Send + Sync + 'static,
    ) -> Self {
        Self { client, interceptor: Some(std::sync::Arc::new(interceptor)) }
    }

    /// Registers the AP Actor with the packet stream and sink.
    pub async fn register(
        &self,
        stream: std::pin::Pin<Box<dyn tokio_stream::Stream<Item = bytes::Bytes> + Sync + Send>>,
        sink: tokio::sync::mpsc::UnboundedSender<bytes::Bytes>,
        shared_keys: std::sync::Arc<super::shared::SharedKeyStore>,
        beacon_interval: std::time::Duration,
    ) -> Result<(), ClientError> {
        #[cfg(feature = "testing")]
        if let Some(interceptor) = &self.interceptor {
            interceptor(&sink);
        }
        let _ = self
            .client
            .perform_action(None, ApReq::Register { stream, sink, shared_keys, beacon_interval })
            .err_into::<ClientError>()
            .await?;
        Ok(())
    }

    /// Creates a new Access Point with the given configuration.
    pub async fn create_ap(&self, id: Option<u32>, config: ApConfig) -> Result<u32, ClientError> {
        let id_val = match id {
            Some(id_val) => {
                self.client.create_with_id(ApId(id_val), config).err_into::<ClientError>().await?
            }
            None => self.client.create(config).err_into::<ClientError>().await?,
        };
        Ok(id_val.0)
    }

    /// Destroys an Access Point by ID.
    pub async fn destroy_ap(&self, id: u32) -> Result<(), ClientError> {
        self.client.delete(ApId(id)).err_into::<ClientError>().await
    }

    /// Resets all APs and restores the default AP to its initial state.
    pub async fn reset(&self) -> Result<(), ClientError> {
        self.client.perform_action(None, ApReq::Reset).err_into::<ClientError>().await.map(|_| ())
    }

    /// Retrieves the state of an Access Point by ID.
    pub async fn get_ap(&self, id: u32) -> Result<Option<crate::ApState>, ClientError> {
        self.client.get(ApId(id)).err_into::<ClientError>().await
    }

    /// Lists all active Access Points.
    pub async fn list_aps(&self) -> Result<Vec<(u32, crate::ApState)>, ClientError> {
        let aps = self.client.list().err_into::<ClientError>().await?;
        Ok(aps.into_iter().map(|ap| (ap.id.0, ap)).collect())
    }

    /// Updates existing Access Point configuration.
    pub async fn update_ap(
        &self,
        id: u32,
        ssid: Option<String>,
        position: Option<Position>,
    ) -> Result<ApState, ClientError> {
        use crate::ap_actor::{ApActorUpdate, ApUpdate};

        let patch = ApActorUpdate {
            position,
            enabled: None,
            ap_update: Some(ApUpdate {
                ssid: ssid.clone(),
                channel: None,
                force_disconnect: Vec::new(),
            }),
        };
        self.client.update(ApId(id), patch).err_into::<ClientError>().await
    }

    /// Updates Access Point configuration with advanced fields.
    pub async fn update_ap_config(
        &self,
        id: u32,
        ssid: Option<String>,
        channel: Option<u8>,
        force_disconnect: Option<Vec<String>>,
        enabled: Option<bool>,
    ) -> Result<ApState, ClientError> {
        use crate::ap_actor::{ApActorUpdate, ApUpdate};

        let force_disconnect_macs = force_disconnect.unwrap_or_default();

        let patch = ApActorUpdate {
            position: None,
            enabled,
            ap_update: Some(ApUpdate {
                ssid: ssid.clone(),
                channel,
                force_disconnect: force_disconnect_macs,
            }),
        };
        self.client.update(ApId(id), patch).err_into::<ClientError>().await
    }

    /// Disconnects a device from an Access Point.
    pub async fn disconnect(&self, id: u32, mac_str: String) -> Result<(), ClientError> {
        let mac = mac_str.parse::<netsim_packets::MacAddr>().map_err(|e| {
            ClientError::Chip(netsim_model::ChipError::Internal(
                format!("Invalid MAC: {}", e).into(),
            ))
        })?;
        self.client
            .perform_action(Some(ApId(id)), ApReq::Disconnect { mac })
            .await
            .map(|_| ())
            .map_err(|e| ClientError::Chip(netsim_model::ChipError::Internal(e.to_string().into())))
    }
}
