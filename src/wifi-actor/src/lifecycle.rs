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
            if let Err(error) = ap_client
                .register(
                    Box::pin(ap_uplink_rx),
                    ap_downlink_tx,
                    self.shared_keys.clone(),
                    std::time::Duration::from_millis(100),
                )
                .await
            {
                error!(
                    code = crate::err::Code::LifecycleApClientRegisterFailed as u32,
                    %error,
                    "Failed to register with AP client"
                );
            }
            self.to_ap = Some(ap_uplink_tx);
        }
        #[cfg(any(test, feature = "testing"))]
        if let Some(socket) = self.test_mdns_socket.take() {
            start_mdns_stream(ctx, |tx| {
                crate::mdns_forwarder::run_mdns_forwarder_on_socket(socket, tx)
            });
        }
        if self.forward_host_mdns {
            start_mdns_stream(ctx, crate::mdns_forwarder::run_mdns_forwarder);
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
        if self.gateway.should_handle(id) {
            return;
        }
        ctx.abort(id);
        if self.active_chips.contains_key(&id)
            && let Err(error) = self.handle_delete_impl(id, ctx).await
        {
            error!(
                code = crate::err::Code::LifecycleStreamClosedDeleteFailed as u32,
                %error,
                "Failed to delete chip {id} after stream closed"
            );
        }
    }

    async fn on_task_closed(&mut self, id: Self::Id, ctx: &mut DynContext<Self>) {
        info!("Sink task closed for chip {id}");
        ctx.remove_stream(id);
        if self.active_chips.contains_key(&id)
            && let Err(error) = self.handle_delete_impl(id, ctx).await
        {
            error!(
                code = crate::err::Code::LifecycleTaskClosedDeleteFailed as u32,
                %error,
                "Failed to delete chip {id} after sink task closed"
            );
        }
    }

    async fn on_typed_stream(
        &mut self,
        id: usize,
        packet: bytes::Bytes,
        _ctx: &mut DynContext<Self>,
    ) {
        if id == AP_SUBSCRIPTION_ID {
            self.process_ap_packet(packet);
        } else if id == MDNS_SUBSCRIPTION_ID {
            let res = self.medium.process_ieee8023_response(&packet, &mut self.out_queue);
            self.medium.wifi_stats.log_outcome(res, |_, _| {});
            self.flush_out_queue();
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

fn start_mdns_stream<F, Fut>(ctx: &mut DynContext<WifiActor>, run: F)
where
    F: FnOnce(UnboundedSender<bytes::Bytes>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    let (mdns_tx, mdns_rx) = create_channel_stream();
    ctx.add_typed_stream(MDNS_SUBSCRIPTION_ID, Box::pin(mdns_rx));

    tokio::spawn(async move {
        if let Err(error) = run(mdns_tx).await {
            tracing::error!(
                code = crate::err::Code::LifecycleMdnsForwarderFailed as u32,
                %error,
                "mDNS Forwarder failed"
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, sync::Arc};

    use actor_framework::ResourceActor;
    use ap_actor::{ApActor, ApClient, SharedKeyStore};
    use common::util::netsim_logger::{ErrorStatsLayer, ErrorStatsTable};
    use netsim_packets::MacAddress;
    use tracing::instrument::WithSubscriber;
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;
    use crate::{MockClock, Station, err::Code, test_support::NoopContext};

    const MDNS_TEST_CHIP_ID: ChipId = ChipId(1);
    const MDNS_TEST_STA_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x10];
    const MDNS_TEST_HOST_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 50);

    fn mdns_test_actor() -> (WifiActor, tokio::sync::mpsc::UnboundedReceiver<bytes::Bytes>) {
        let (_dev_runner, dev_base) = ResourceActor::<device_actor::DeviceActor>::new(1);
        let device_client = device_actor::DeviceClient::new(Box::new(dev_base));
        let mut actor = WifiActor::new(
            None,
            None,
            device_client,
            None,
            Arc::new(SharedKeyStore::new()),
            Arc::new(MockClock::new()),
            false,
        );
        let sta_mac = MacAddress::new(MDNS_TEST_STA_MAC);
        actor.medium.add(MDNS_TEST_CHIP_ID.0);
        actor.medium.stations.insert(sta_mac, Station::new(MDNS_TEST_CHIP_ID.0, sta_mac, sta_mac));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        actor.senders.insert(MDNS_TEST_CHIP_ID, tx);
        (actor, rx)
    }

    #[tokio::test]
    async fn test_on_start_ap_client_register_failed_records_error_stat() {
        let _multi_dispatch_guard = tracing::Dispatch::new(tracing_subscriber::registry());
        let table = Arc::new(ErrorStatsTable::new());
        let subscriber =
            tracing_subscriber::registry().with(ErrorStatsLayer::with_table(Arc::clone(&table)));

        let (ap_runner, ap_base) = ResourceActor::<ApActor>::new(1);
        drop(ap_runner);
        let ap_client = Arc::new(ApClient::new(ap_base));

        let (_dev_runner, dev_base) = ResourceActor::<device_actor::DeviceActor>::new(1);
        let device_client = device_actor::DeviceClient::new(Box::new(dev_base));

        let actor = WifiActor::new(
            Some(ap_client),
            None,
            device_client,
            None,
            Arc::new(SharedKeyStore::new()),
            Arc::new(MockClock::new()),
            false,
        );
        let (wifi_runner, wifi_client) = crate::new();
        tokio::spawn(tracing::instrument::WithSubscriber::with_subscriber(
            wifi_runner.run(actor),
            subscriber,
        ));

        let _ = wifi_client.get_global_stats_proto().await.unwrap();

        assert_eq!(table.snapshot(), vec![(Code::LifecycleApClientRegisterFailed as u32, 1)]);
    }

    #[tokio::test]
    async fn test_on_typed_stream_mdns_without_ap_drops_cleanly() {
        let (mut actor, mut rx) = mdns_test_actor();
        let mdns_eth =
            crate::mdns_forwarder::create_mdns_frame(b"host-mdns-no-ap", MDNS_TEST_HOST_IP);

        let mut ctx = NoopContext;
        actor.on_typed_stream(MDNS_SUBSCRIPTION_ID, bytes::Bytes::from(mdns_eth), &mut ctx).await;

        assert!(rx.try_recv().is_err(), "host mDNS must drop cleanly when no AP is active");
        let stats = actor.medium.wifi_stats.to_proto().wifi_stats;
        assert_eq!(stats.hostapd_frames_rx(), 0);
        assert_eq!(stats.other_errors(), 0);
        assert_eq!(stats.mdns_count(), 0);
    }

    #[tokio::test]
    async fn test_on_typed_stream_mdns_malformed_packet_records_error_stat() {
        let _multi_dispatch_guard = tracing::Dispatch::new(tracing_subscriber::registry());
        let table = Arc::new(ErrorStatsTable::new());
        let subscriber =
            tracing_subscriber::registry().with(ErrorStatsLayer::with_table(Arc::clone(&table)));
        let (mut actor, mut rx) = mdns_test_actor();

        let mut ctx = NoopContext;
        actor
            .on_typed_stream(
                MDNS_SUBSCRIPTION_ID,
                bytes::Bytes::from_static(&[0x01, 0x02, 0x03]),
                &mut ctx,
            )
            .with_subscriber(subscriber)
            .await;

        assert!(rx.try_recv().is_err(), "truncated mDNS packet must not deliver a frame");
        let stats = actor.medium.wifi_stats.to_proto().wifi_stats;
        assert_eq!(stats.hostapd_frames_rx(), 0);
        assert_eq!(stats.other_errors(), 1);
        assert_eq!(table.snapshot(), vec![(Code::StatsInternalError as u32, 1)]);
    }
}
