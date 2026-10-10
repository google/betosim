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
use std::collections::hash_map::Entry;

use actor_framework::{ActorService, DynContext};
#[cfg(not(feature = "cuttlefish"))]
use libslirp_rs::LibSlirp;
#[cfg(not(feature = "cuttlefish"))]
use slirp::ProxyManager;

use crate::{
    error::SlirpError,
    slirp_actor::{
        ClientInfo, SlirpActor, SlirpBackend, SlirpBackendInstance, SlirpCreate, SlirpReq,
        SlirpStatus,
    },
};

#[cfg(not(feature = "cuttlefish"))]
fn to_native_config(config: &libslirp_rs::SlirpConfig) -> slirp::Config {
    let mut guest_ipv6_octets = config.vprefix_addr6.octets();
    guest_ipv6_octets[15] = 1;
    let guest_ipv6 = std::net::Ipv6Addr::from(guest_ipv6_octets);

    let dns_search =
        if config.vdnssearch.is_empty() { None } else { Some(config.vdnssearch.clone()) };

    let dns_servers = config.host_dns.iter().map(|addr| addr.ip()).collect::<Vec<_>>();

    slirp::Config {
        host_ipv4: config.vhost,
        guest_ipv4: config.vdhcp_start,
        host_ipv6: config.vhost6,
        guest_ipv6,
        boot_file: config.bootfile.clone(),
        tftp_server_name: config.tftp_server_name.clone(),
        domain_name: config.vdomainname.clone(),
        dns_search,
        client_hostname: config.vhostname.clone(),
        dns_servers,
        tftp_root: config.tftp_path.clone(),
        ..Default::default()
    }
}

impl SlirpActor {
    fn init_backend(&mut self, ctx: &mut DynContext<Self>) -> Result<(), SlirpError> {
        match self.backend_type {
            #[cfg(not(feature = "cuttlefish"))]
            SlirpBackend::CFfi => self.init_c_ffi_backend(ctx),
            #[cfg(feature = "cuttlefish")]
            SlirpBackend::CFfi => {
                tracing::warn!(
                    "C-FFI backend requested on cuttlefish build; defaulting to Native slirp"
                );
                self.backend_type = SlirpBackend::Native;
                self.init_native_backend(ctx)
            }
            SlirpBackend::Native => self.init_native_backend(ctx),
        }
    }

    #[cfg(not(feature = "cuttlefish"))]
    fn init_c_ffi_backend(&mut self, ctx: &mut DynContext<Self>) -> Result<(), SlirpError> {
        let mut config = self.config.clone();
        let mut proxy_manager = None;
        let mut tx_proxy_bytes = None;

        if let Some(ref proxy) = self.http_proxy {
            let (tx, rx) = std::sync::mpsc::channel();
            config.http_proxy_on = true;
            let manager = http_proxy::Manager::new(proxy, rx).map_err(|e| {
                tracing::error!("Failed to initialize HTTP proxy '{proxy}': {e}");
                crate::error::SlirpError::Internal(e.to_string())
            })?;
            proxy_manager = Some(Box::new(manager) as Box<dyn ProxyManager>);
            tx_proxy_bytes = Some(tx);
        }

        // Create internal downlink channel
        let (downlink_tx, downlink_rx) = tokio::sync::mpsc::unbounded_channel();

        // Wrap tx in Box<dyn PacketSender>, effectively removing the bridge
        // thread
        let slirp = LibSlirp::new(config, Box::new(downlink_tx), proxy_manager, tx_proxy_bytes);
        self.backend_instance = Some(SlirpBackendInstance::CFfi(slirp));

        let stream_id = self.next_stream_id;
        self.next_stream_id += 1;
        self.active_stream_id = Some(stream_id);

        // Register internal downlink stream as a typed stream
        ctx.add_typed_stream(
            stream_id,
            Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(downlink_rx)),
        );
        Ok(())
    }

    fn init_native_backend(&mut self, ctx: &mut DynContext<Self>) -> Result<(), SlirpError> {
        #[cfg(feature = "cuttlefish")]
        let mut native_config = self.config.clone();
        #[cfg(not(feature = "cuttlefish"))]
        let mut native_config = to_native_config(&self.config);

        if let Some(ref proxy) = self.http_proxy {
            native_config.http_proxy = Some(proxy.clone());
            #[cfg(feature = "http-proxy")]
            {
                let (tx, rx) = std::sync::mpsc::channel();
                match http_proxy::Manager::new(proxy, rx) {
                    Ok(manager) => {
                        native_config.proxy_manager = Some(std::sync::Arc::new(manager));
                        native_config.proxy_tx =
                            Some(std::sync::Arc::new(std::sync::Mutex::new(tx)));
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to initialize HTTP Proxy Manager for Native backend: {e}"
                        );
                    }
                }
            }
        }

        let (downlink_tx, downlink_rx) = tokio::sync::mpsc::unbounded_channel::<bytes::Bytes>();
        let (uplink_tx, uplink_rx) = tokio::sync::mpsc::unbounded_channel::<bytes::Bytes>();

        self.backend_instance = Some(SlirpBackendInstance::Native { uplink_tx });

        let stream_id = self.next_stream_id;
        self.next_stream_id += 1;
        self.active_stream_id = Some(stream_id);

        ctx.add_typed_stream(
            stream_id,
            Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(downlink_rx)),
        );

        let task_id = self.next_task_id;
        self.next_task_id += 1;
        self.active_task_id = Some(task_id);

        ctx.spawn(
            task_id,
            Box::pin(async move {
                // If `dns_servers` is empty or matches default configuration
                // [8.8.8.8], automatically discover host DNS
                // servers so guest queries are not
                // routed to hardcoded public servers. Explicitly configured DNS
                // server lists are preserved and never
                // overridden.
                if native_config.dns_servers.is_empty()
                    || native_config.dns_servers == [slirp::DEFAULT_DNS_SERVER]
                {
                    let discovered = slirp::discover_host_dns_servers().await;
                    if !discovered.is_empty() {
                        native_config.dns_servers = discovered;
                    } else if native_config.dns_servers.is_empty() {
                        native_config.dns_servers = vec![slirp::DEFAULT_DNS_SERVER];
                    }
                }
                crate::native::run_native_slirp_loop(native_config, downlink_tx, uplink_rx).await;
                task_id
            }),
        );

        Ok(())
    }
}

impl ActorService for SlirpActor {
    type Id = u32;
    type Create = SlirpCreate;
    type Update = ();
    type Action = SlirpReq;
    type ActionResult = ();
    type Error = SlirpError;
    type Entity = SlirpStatus;
    type TypedStream = bytes::Bytes;

    async fn handle_create(
        &mut self,
        _id: Option<Self::Id>,
        _create: Self::Create,
        _: &mut DynContext<Self>,
    ) -> Result<Self::Id, Self::Error> {
        Err(SlirpError::Internal(
            "SlirpActor::create should not be called. Use RegisterSink action instead.".into(),
        ))
    }

    async fn handle_get(
        &self,
        _id: Self::Id,
        _: &mut DynContext<Self>,
    ) -> Result<Option<Self::Entity>, Self::Error> {
        Ok(Some(SlirpStatus { initialized: self.backend_instance.is_some() }))
    }

    async fn handle_update(
        &mut self,
        _id: Self::Id,
        _: Self::Update,
        _: &mut DynContext<Self>,
    ) -> Result<Self::Entity, Self::Error> {
        Err(SlirpError::Internal("Update not supported".into()))
    }

    async fn handle_delete(
        &mut self,
        _id: Self::Id,
        _: &mut DynContext<Self>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn handle_list(
        &mut self,
        _: &mut DynContext<Self>,
    ) -> Result<Vec<Self::Entity>, Self::Error> {
        Ok(vec![])
    }

    async fn handle_action(
        &mut self,
        _id: Option<Self::Id>,
        action: Self::Action,
        ctx: &mut DynContext<Self>,
    ) -> Result<Self::ActionResult, Self::Error> {
        match action {
            SlirpReq::SendPacket(data) => {
                if let Some(instance) = &self.backend_instance {
                    instance.input(data);
                } else {
                    return Err(SlirpError::Internal(
                        "SlirpActor: SendPacket called without an active backend".to_string(),
                    ));
                }
            }
            // Register a new Layer-2 client port (WiFi or Ethernet/Cellular).
            // - WiFiActor: Registers a single L2 port representing the wireless gateway interface.
            // - EthernetActor: Registers individual L2 ports representing each emulated guest
            //   Ethernet or Cellular chip.
            // Both actors exchange pre-formatted L2 802.3 Ethernet frames with the switch.
            SlirpReq::Register { client_id, stream, mut sink, notifier, isolated } => {
                if self.backend_instance.is_none() {
                    self.init_backend(ctx)?;
                }

                if let Entry::Vacant(e) = self.clients.entry(client_id) {
                    tracing::info!("SlirpActor: Registering client {client_id}");
                    let (downlink_tx, mut downlink_rx) = tokio::sync::mpsc::unbounded_channel();
                    e.insert(ClientInfo::new(downlink_tx, notifier, isolated));
                    ctx.add_stream(client_id, stream);

                    // Spawn isolated background forwarding task on SlirpActor's
                    // runtime context
                    ctx.spawn(
                        client_id,
                        Box::pin(async move {
                            use futures::sink::SinkExt;
                            while let Some(bytes) = downlink_rx.recv().await {
                                if sink.send(bytes).await.is_err() {
                                    break;
                                }
                            }
                            client_id
                        }),
                    );
                } else {
                    tracing::warn!("SlirpActor: Register called twice for client {client_id}");
                }
            }
            SlirpReq::Unregister { client_id } => {
                tracing::info!("SlirpActor: Unregistering client {client_id}");
                self.clients.remove(&client_id);
                self.mac_table.retain(|_, v| *v != client_id);
                ctx.remove_stream(client_id);
            }
            SlirpReq::AllocateLease { chip_id, respond_to } => {
                let lease = self.allocate_lease(chip_id);
                let _ = respond_to.send(lease);
            }
            SlirpReq::ReleaseLease { chip_id } => {
                self.release_lease(chip_id);
            }
            SlirpReq::SwitchBackend(new_backend) => {
                if self.backend_type != new_backend {
                    tracing::info!("SlirpActor: Switching backend to {:?}", new_backend);
                    if let Some(old_instance) = self.backend_instance.take() {
                        old_instance.shutdown();
                    }
                    self.backend_type = new_backend;
                    if !self.clients.is_empty() {
                        self.init_backend(ctx)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use actor_framework::Context;
    use tokio::sync::mpsc;

    use super::*;

    struct ServiceTestContext;

    impl Context<SlirpActor> for ServiceTestContext {
        fn set_interval(&mut self, _duration: Duration) {}
        fn add_stream(&mut self, _id: u32, _stream: actor_framework::BoxStream) {}
        fn remove_stream(&mut self, _id: u32) {}
        fn add_typed_stream(
            &mut self,
            _id: usize,
            _stream: actor_framework::BoxTypedStream<bytes::Bytes>,
        ) {
        }
        fn remove_typed_stream(&mut self, _id: usize) {}
        fn spawn(&mut self, _id: u32, task: futures::future::BoxFuture<'static, u32>) {
            tokio::spawn(task);
        }
        fn abort(&mut self, _id: u32) {}
        fn shutdown(&mut self) {}
        fn run_later(
            &mut self,
            _duration: Duration,
            _f: Box<dyn FnOnce(&mut SlirpActor, &mut dyn Context<SlirpActor>) + Send>,
        ) -> actor_framework::TimerKey {
            unimplemented!()
        }
        fn cancel_timer(&mut self, _key: actor_framework::TimerKey) {}
    }

    // Proxy support is implemented in the C-FFI backend (libslirp-rs).
    // On cuttlefish builds, netsim uses the Native backend which does not
    // support HTTP proxies.
    #[tokio::test]
    #[cfg(not(feature = "cuttlefish"))]
    async fn test_invalid_http_proxy_returns_error() {
        let mut actor = SlirpActor::new(
            Default::default(),
            Some("http://invalid-unresolvable-proxy.invalid:8080".to_string()),
            None,
        )
        .await;
        let mut ctx = ServiceTestContext;

        let (tx_out, _rx_out) = mpsc::unbounded_channel::<bytes::Bytes>();
        let (_stream_tx, stream_rx) = mpsc::unbounded_channel::<bytes::Bytes>();
        let stream = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx));
        let sink: netsim_model::PacketSink =
            Box::pin(futures::sink::unfold(tx_out, |tx, bytes| async move {
                let _ = tx.send(bytes);
                Ok(tx)
            }));

        let result = actor
            .handle_action(
                None,
                SlirpReq::Register { client_id: 1, stream, sink, notifier: None, isolated: false },
                &mut ctx,
            )
            .await;
        assert!(result.is_err());
        assert!(actor.backend_instance.is_none());
    }

    #[tokio::test]
    async fn test_send_packet_before_register_returns_error() {
        let mut actor = SlirpActor::new(Default::default(), None, None).await;
        let mut ctx = ServiceTestContext;

        let result = actor
            .handle_action(
                None,
                SlirpReq::SendPacket(bytes::Bytes::from_static(b"unregistered_packet")),
                &mut ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[cfg(not(feature = "cuttlefish"))]
    async fn test_valid_http_proxy_registers_successfully() {
        let mut actor =
            SlirpActor::new(Default::default(), Some("http://127.0.0.1:8080".to_string()), None)
                .await;
        let mut ctx = ServiceTestContext;

        let (tx_out, _rx_out) = mpsc::unbounded_channel::<bytes::Bytes>();
        let (_stream_tx, stream_rx) = mpsc::unbounded_channel::<bytes::Bytes>();
        let stream = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx));
        let sink: netsim_model::PacketSink =
            Box::pin(futures::sink::unfold(tx_out, |tx, bytes| async move {
                let _ = tx.send(bytes);
                Ok(tx)
            }));

        let result = actor
            .handle_action(
                None,
                SlirpReq::Register { client_id: 1, stream, sink, notifier: None, isolated: false },
                &mut ctx,
            )
            .await;
        assert!(result.is_ok());
        assert!(actor.backend_instance.is_some());

        // Also verify SendPacket succeeds when backend is active
        let send_result = actor
            .handle_action(
                None,
                SlirpReq::SendPacket(bytes::Bytes::from_static(b"test_packet")),
                &mut ctx,
            )
            .await;
        assert!(send_result.is_ok());
    }

    #[tokio::test]
    async fn test_none_http_proxy_registers_successfully() {
        let mut actor = SlirpActor::new(Default::default(), None, None).await;
        let mut ctx = ServiceTestContext;

        let (tx_out, _rx_out) = mpsc::unbounded_channel::<bytes::Bytes>();
        let (_stream_tx, stream_rx) = mpsc::unbounded_channel::<bytes::Bytes>();
        let stream = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx));
        let sink: netsim_model::PacketSink =
            Box::pin(futures::sink::unfold(tx_out, |tx, bytes| async move {
                let _ = tx.send(bytes);
                Ok(tx)
            }));

        let result = actor
            .handle_action(
                None,
                SlirpReq::Register { client_id: 1, stream, sink, notifier: None, isolated: false },
                &mut ctx,
            )
            .await;
        assert!(result.is_ok());
        assert!(actor.backend_instance.is_some());
    }
}
