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

use actor_framework::{ActorLifecycle, DynContext};
use tracing::info;

use crate::slirp_actor::SlirpActor;

impl ActorLifecycle for SlirpActor {
    async fn on_start(&mut self, _ctx: &mut DynContext<Self>) {
        info!("SlirpActor starting");
    }

    async fn on_tick(&mut self, _ctx: &mut DynContext<Self>) {}

    async fn on_stream(&mut self, id: u32, msg: bytes::Bytes, ctx: &mut DynContext<Self>) {
        let Some((frame, _remainder)) = netsim_packets::EthernetFrame::parse(&msg) else {
            tracing::warn!("SlirpActor: Dropping runt uplink packet from {id}, len {}", msg.len());
            return;
        };

        let src_mac = frame.src_addr;

        // MAC Address Learning & Conflict Detection
        if let Some(&existing_id) = self.mac_table.get(&src_mac)
            && existing_id != id
            && self.clients.contains_key(&existing_id)
        {
            tracing::warn!(
                "SlirpActor: MAC conflict! Client {id} tried to claim {src_mac} owned by {existing_id}, disconnecting {id}"
            );
            // Disconnect the offending client `id`
            self.clients.remove(&id);
            ctx.remove_stream(id);
            return;
        }
        self.mac_table.insert(src_mac, id);

        // UPLINK FLOW (Client -> backend)
        if let Some(instance) = &self.backend_instance {
            instance.input(msg);
        }
    }

    async fn on_stream_closed(&mut self, id: u32, _ctx: &mut DynContext<Self>) {
        info!("SlirpActor: Client {id} disconnected");
        self.mac_table.retain(|_, v| *v != id);
        if let Some(client) = self.clients.remove(&id) {
            client.notify_disconnected(id);
        }
    }

    async fn on_typed_stream(
        &mut self,
        _id: usize,
        msg: bytes::Bytes,
        _ctx: &mut DynContext<Self>,
    ) {
        let Some((frame, remainder)) = netsim_packets::EthernetFrame::parse(&msg) else {
            tracing::warn!("SlirpActor: Dropping runt downlink packet, len {}", msg.len());
            return;
        };

        let dest_mac = frame.dst_addr;

        // DOWNLINK FLOW (libslirp -> SlirpActor -> Clients)
        if !dest_mac.is_multicast() {
            if let Some(&client_id) = self.mac_table.get(&dest_mac) {
                if let Some(client) = self.clients.get(&client_id) {
                    client.send(msg);
                    return;
                }
                tracing::warn!("SlirpActor: Stale MAC mapping {dest_mac} -> {client_id}, flooding");
            } else {
                tracing::warn!("SlirpActor: Unknown unicast MAC {dest_mac}, flooding");
            }
            for client in self.clients.values() {
                if !client.is_isolated() {
                    client.send(msg.clone());
                }
            }
        } else {
            // Flood broadcast/multicast packets according to client isolation policy
            let is_cellular_resolution = if frame.ethertype.get() == netsim_packets::ether_type::ARP
                && let Some(arp) = netsim_packets::ArpPacket::parse(remainder)
            {
                let ar_tip = std::net::Ipv4Addr::from(arp.target_protocol_addr);
                self.static_ips.values().any(|&ip| ip == ar_tip)
            } else if frame.ethertype.get() == netsim_packets::ether_type::IPV6
                && let Some(packet) = netsim_packets::parse(&msg)
                && let Some(netsim_packets::TransportPacket::Icmpv6(icmpv6_header, icmpv6_payload)) =
                    packet.transport
                && icmpv6_header.icmpv6_type
                    == netsim_packets::Icmpv6Type::NeighborSolicitation as u8
                && let Some(&target_bytes) = icmpv6_payload.first_chunk::<16>()
            {
                self.matches_static_ipv6(std::net::Ipv6Addr::from(target_bytes))
            } else {
                false
            };

            for client in self.clients.values() {
                let should_deliver = client.is_isolated() == is_cellular_resolution;
                if should_deliver {
                    client.send(msg.clone());
                }
            }
        }
    }

    async fn on_typed_stream_closed(&mut self, id: usize, _ctx: &mut DynContext<Self>) {
        if Some(id) != self.active_stream_id {
            tracing::info!(
                "SlirpActor: Ignored closed event for stale gateway downlink stream, id {id} (active: {:?})",
                self.active_stream_id
            );
            return;
        }
        tracing::warn!(
            "SlirpActor: Gateway downlink stream closed, id {id}. Clearing network stack."
        );
        self.active_stream_id = None;
        self.active_task_id = None;
        if let Some(instance) = self.backend_instance.take() {
            instance.shutdown();
        }
    }

    async fn on_task_closed(&mut self, id: u32, _ctx: &mut DynContext<Self>) {
        if Some(id) == self.active_task_id {
            tracing::warn!("SlirpActor: Active native loop task closed, id {id}");
            self.active_task_id = None;
            self.active_stream_id = None;
            if let Some(instance) = self.backend_instance.take() {
                instance.shutdown();
            }
        } else {
            tracing::info!("SlirpActor: Ignored closed event for stale task, id {id}");
        }
    }
}

#[cfg(test)]
mod tests {
    use actor_framework::{ActorService, Context};
    use tokio::sync::mpsc;

    use super::*;
    use crate::slirp_actor::SlirpReq;

    struct TestContext {
        streams: std::collections::HashSet<u32>,
    }

    impl Context<SlirpActor> for TestContext {
        fn set_interval(&mut self, _duration: std::time::Duration) {}
        fn add_stream(&mut self, id: u32, _stream: actor_framework::BoxStream) {
            self.streams.insert(id);
        }
        fn remove_stream(&mut self, id: u32) {
            self.streams.remove(&id);
        }
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
            _duration: std::time::Duration,
            _f: Box<dyn FnOnce(&mut SlirpActor, &mut dyn Context<SlirpActor>) + Send>,
        ) -> actor_framework::TimerKey {
            unimplemented!("run_later not implemented for TestContext")
        }
        fn cancel_timer(&mut self, _key: actor_framework::TimerKey) {}
    }

    #[tokio::test]
    async fn test_mac_address_conflict_disconnection() {
        // Create actor
        let mut actor = SlirpActor::new(Default::default(), None::<String>, None::<String>).await;
        let mut ctx = TestContext { streams: std::collections::HashSet::new() };

        // Setup client 1
        let (tx1, _rx1) = mpsc::unbounded_channel();
        let (_stream_tx1, stream_rx1) = mpsc::unbounded_channel();
        let stream1 = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx1));
        let sink1: netsim_model::PacketSink =
            Box::pin(futures::sink::unfold(tx1, |tx, bytes| async move {
                let _ = tx.send(bytes);
                Ok(tx)
            }));

        // Register client 1
        actor
            .handle_action(
                None,
                SlirpReq::Register {
                    client_id: 1,
                    stream: stream1,
                    sink: sink1,
                    notifier: None,
                    isolated: false,
                },
                &mut ctx,
            )
            .await
            .unwrap();
        ctx.streams.insert(1);

        // Setup client 2
        let (tx2, _rx2) = mpsc::unbounded_channel();
        let (_stream_tx2, stream_rx2) = mpsc::unbounded_channel();
        let stream2 = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx2));
        let sink2: netsim_model::PacketSink =
            Box::pin(futures::sink::unfold(tx2, |tx, bytes| async move {
                let _ = tx.send(bytes);
                Ok(tx)
            }));

        // Register client 2
        actor
            .handle_action(
                None,
                SlirpReq::Register {
                    client_id: 2,
                    stream: stream2,
                    sink: sink2,
                    notifier: None,
                    isolated: false,
                },
                &mut ctx,
            )
            .await
            .unwrap();
        ctx.streams.insert(2);

        // Create an Ethernet packet from client 1 with MAC 00:11:22:33:44:55
        let packet1 = bytes::Bytes::from(vec![
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Dst MAC
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, // Src MAC
            0x08, 0x00, // EtherType (IPv4)
        ]);

        // Deliver packet 1 from client 1
        actor.on_stream(1, packet1, &mut ctx).await;

        // Assert client 1 is learned in mac_table
        let mac = netsim_packets::MacAddress::new([0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
        assert_eq!(actor.mac_table.get(&mac), Some(&1));
        assert!(actor.clients.contains_key(&1));
        assert!(ctx.streams.contains(&1));

        // Create a packet from client 2 with the SAME MAC (conflict!)
        let packet2 = bytes::Bytes::from(vec![
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Dst MAC
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, // Src MAC
            0x08, 0x00, // EtherType
        ]);

        // Deliver packet 2 from client 2
        actor.on_stream(2, packet2, &mut ctx).await;

        // Assert client 2 is disconnected due to conflict
        assert!(!actor.clients.contains_key(&2), "Client 2 should be removed from clients map");
        assert!(!ctx.streams.contains(&2), "Client 2 stream should be removed from context");
        assert_eq!(
            actor.mac_table.get(&mac),
            Some(&1),
            "MAC route should still belong to client 1"
        );
    }
}
