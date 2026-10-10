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

pub mod packet_helpers;
pub mod test_driver;

#[cfg(test)]
mod arp_icmp_test;
#[cfg(test)]
mod backend_perf_test;
#[cfg(test)]
mod backend_test;
#[cfg(test)]
mod deficiencies_test;
#[cfg(test)]
mod dhcp_dns_test;
#[cfg(test)]
mod http_proxy_test;
#[cfg(test)]
mod ip_frag_multicast_test;
#[cfg(test)]
mod ipv6_advanced_test;
#[cfg(test)]
mod mac_routing_test;
#[cfg(test)]
mod napt_test;
#[cfg(test)]
mod ndp_ipv6_test;
#[cfg(test)]
mod port_forwarding_test;
#[cfg(test)]
mod recent_fixes_test;
#[cfg(test)]
mod risk_areas_test;
#[cfg(test)]
mod snapshot_test;
#[cfg(test)]
mod tcp_options_test;
#[cfg(test)]
mod tcp_state_test;
#[cfg(test)]
mod tftp_test;

use actor_framework::{ActorLifecycle, ActorService, BoxStream, Context};
use futures::future::BoxFuture;
use slirp_actor::{SlirpActor, SlirpReq};
use tokio::sync::mpsc;
use zerocopy::FromBytes;

struct MockContext;

impl Context<SlirpActor> for MockContext {
    fn set_interval(&mut self, _duration: std::time::Duration) {}
    fn add_stream(&mut self, _id: u32, _stream: BoxStream) {}
    fn remove_stream(&mut self, _id: u32) {}
    fn add_typed_stream(
        &mut self,
        _id: usize,
        _stream: actor_framework::BoxTypedStream<bytes::Bytes>,
    ) {
    }
    fn remove_typed_stream(&mut self, _id: usize) {}
    fn spawn(&mut self, _id: u32, task: BoxFuture<'static, u32>) {
        tokio::spawn(task);
    }
    fn abort(&mut self, _id: u32) {}
    fn shutdown(&mut self) {}
    fn run_later(
        &mut self,
        _duration: std::time::Duration,
        _f: Box<dyn FnOnce(&mut SlirpActor, &mut dyn Context<SlirpActor>) + Send>,
    ) -> actor_framework::TimerKey {
        unimplemented!("run_later not implemented for MockContext")
    }
    fn cancel_timer(&mut self, _key: actor_framework::TimerKey) {}
}

#[tokio::test]
async fn test_slirp_actor_lifecycle() {
    // Given a SlirpActor
    let (tx_out, _rx_out) = mpsc::unbounded_channel::<bytes::Bytes>();
    let mut actor = SlirpActor::new(Default::default(), None, None).await;
    let mut ctx = MockContext;

    // Register Sink
    let (_stream_tx, stream_rx) = mpsc::unbounded_channel::<bytes::Bytes>();
    let stream = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx));
    let sink: netsim_model::PacketSink =
        Box::pin(futures::sink::unfold(tx_out, |tx, bytes| async move {
            let _ = tx.send(bytes);
            Ok(tx)
        }));
    let _ = actor
        .handle_action(
            None,
            SlirpReq::Register { client_id: 0, stream, sink, notifier: None, isolated: false },
            &mut ctx,
        )
        .await;

    // When I start the actor
    actor.on_start(&mut ctx).await;

    // Then it should initialize (we can check via handle_get)
    let status = actor.handle_get(0, &mut ctx).await;
    assert!(status.is_ok());
    assert!(status.unwrap().unwrap().initialized);

    // When I send a packet
    let dummy_packet = bytes::Bytes::from(vec![0u8; 64]);
    let req = SlirpReq::SendPacket(dummy_packet);

    // handle_action
    let result = actor.handle_action(None, req, &mut ctx).await;

    // Then the request should be handled successfully
    assert!(result.is_ok());

    // When I drop the actor, it shuts down (via Drop)
    drop(actor);
}

#[tokio::test]
async fn test_slirp_mac_learning_and_switching() {
    let (tx_out1, mut rx_out1) = mpsc::unbounded_channel::<bytes::Bytes>();
    let (tx_out2, mut rx_out2) = mpsc::unbounded_channel::<bytes::Bytes>();
    let mut actor = SlirpActor::new(Default::default(), None, None).await;
    let mut ctx = MockContext;

    let (_stream_tx1, stream_rx1) = mpsc::unbounded_channel::<bytes::Bytes>();
    let (_stream_tx2, stream_rx2) = mpsc::unbounded_channel::<bytes::Bytes>();
    let stream1: std::pin::Pin<Box<dyn tokio_stream::Stream<Item = bytes::Bytes> + Sync + Send>> =
        Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx1));
    let stream2: std::pin::Pin<Box<dyn tokio_stream::Stream<Item = bytes::Bytes> + Sync + Send>> =
        Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx2));

    let sink1: netsim_model::PacketSink =
        Box::pin(futures::sink::unfold(tx_out1, |tx, bytes| async move {
            let _ = tx.send(bytes);
            Ok(tx)
        }));
    let sink2: netsim_model::PacketSink =
        Box::pin(futures::sink::unfold(tx_out2, |tx, bytes| async move {
            let _ = tx.send(bytes);
            Ok(tx)
        }));
    let _ = actor
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
        .await;
    let _ = actor
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
        .await;

    actor.on_start(&mut ctx).await;

    // 1. Client 1 sends uplink packet with source MAC 00:11:22:33:44:55 (dest MAC
    //    00:00:00:00:00:00 is unknown unicast)
    let mut uplink_payload = vec![0u8; 64];
    uplink_payload[6..12].copy_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
    actor.on_stream(1, bytes::Bytes::from(uplink_payload), &mut ctx).await;

    // Verify client 2 does not receive the uplink packet (uplink is unicast to
    // libslirp only)
    assert!(rx_out2.try_recv().is_err());

    // 2. Downlink unicast packet arrives for 00:11:22:33:44:55
    let mut downlink_unicast = vec![0u8; 64];
    downlink_unicast[0..6].copy_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
    actor.on_typed_stream(0, bytes::Bytes::from(downlink_unicast), &mut ctx).await;
    tokio::task::yield_now().await;

    // Verify client 1 receives it
    let pkt1 = rx_out1.try_recv();
    assert!(pkt1.is_ok());
    assert!(rx_out2.try_recv().is_err());

    // 3. Downlink broadcast packet arrives
    let mut downlink_bcast = vec![0u8; 64];
    downlink_bcast[0..6].copy_from_slice(&[0xff; 6]);
    actor.on_typed_stream(0, bytes::Bytes::from(downlink_bcast), &mut ctx).await;
    tokio::task::yield_now().await;

    // Verify both clients receive it
    assert!(rx_out1.try_recv().is_ok());
    assert!(rx_out2.try_recv().is_ok());
}

async fn run_slirp_backend_lifecycle_test(backend: slirp_actor::SlirpBackend) {
    let (tx_out, _rx_out) = mpsc::unbounded_channel::<bytes::Bytes>();
    let mut actor = SlirpActor::new_with_backend(Default::default(), None, None, backend).await;
    let mut ctx = MockContext;

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
            SlirpReq::Register { client_id: 0, stream, sink, notifier: None, isolated: false },
            &mut ctx,
        )
        .await;
    assert!(result.is_ok());

    actor.on_start(&mut ctx).await;

    let status = actor.handle_get(0, &mut ctx).await;
    assert!(status.is_ok());
    assert!(status.unwrap().unwrap().initialized);

    let dummy_packet = bytes::Bytes::from(vec![0u8; 64]);
    let req = SlirpReq::SendPacket(dummy_packet);
    let result = actor.handle_action(None, req, &mut ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_slirp_backend_lifecycle() {
    for backend in [slirp_actor::SlirpBackend::CFfi, slirp_actor::SlirpBackend::Native] {
        run_slirp_backend_lifecycle_test(backend).await;
    }
}

#[tokio::test]
async fn test_switch_backend() {
    let mut actor = SlirpActor::new(Default::default(), None, None).await;
    let mut ctx = MockContext;

    let result = actor
        .handle_action(None, SlirpReq::SwitchBackend(slirp_actor::SlirpBackend::Native), &mut ctx)
        .await;
    assert!(result.is_ok());
    assert_eq!(actor.backend(), slirp_actor::SlirpBackend::Native);
}

async fn run_slirp_backend_icmp_ping_test(backend: slirp_actor::SlirpBackend) {
    let (tx_out, _rx_out) = mpsc::unbounded_channel::<bytes::Bytes>();
    let mut actor = SlirpActor::new_with_backend(Default::default(), None, None, backend).await;
    let mut ctx = MockContext;

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
            SlirpReq::Register { client_id: 0, stream, sink, notifier: None, isolated: false },
            &mut ctx,
        )
        .await;
    assert!(result.is_ok());

    actor.on_start(&mut ctx).await;

    let icmp_header_len = std::mem::size_of::<netsim_packets::IcmpHeader>();
    let payload = b"netsim icmp ping payload";
    let total_icmp_len = icmp_header_len + payload.len();

    let mut icmp_data = vec![0u8; total_icmp_len];
    let (icmp_hdr_slice, echo_payload_slice) = icmp_data.split_at_mut(icmp_header_len);
    let icmp_header = netsim_packets::IcmpHeader::mut_from_bytes(icmp_hdr_slice).unwrap();
    icmp_header.icmp_type = 8;
    icmp_header.icmp_code = 0;
    icmp_header.icmp_checksum.set(0);
    icmp_header.rest[..2].copy_from_slice(&0x1234u16.to_be_bytes());
    icmp_header.rest[2..].copy_from_slice(&1u16.to_be_bytes());
    echo_payload_slice.copy_from_slice(payload);

    let checksum = netsim_packets::ipv4_checksum(&icmp_data);
    let icmp_header_mut =
        netsim_packets::IcmpHeader::mut_from_bytes(&mut icmp_data[..icmp_header_len]).unwrap();
    icmp_header_mut.icmp_checksum.set(checksum);

    let mut ip_data = vec![0u8; 20 + total_icmp_len];
    let mut ipv4_builder = netsim_packets::Ipv4Builder::new(
        &mut ip_data,
        netsim_packets::IP_P_ICMP,
        std::net::Ipv4Addr::new(10, 0, 2, 15),
        std::net::Ipv4Addr::new(8, 8, 8, 8),
    )
    .unwrap();
    ipv4_builder.payload(&icmp_data).unwrap();
    ipv4_builder.build().unwrap();

    let mut eth_data = vec![0u8; 14 + ip_data.len()];
    let (eth_header_slice, eth_payload_slice) = eth_data.split_at_mut(14);
    let eth_frame = netsim_packets::EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
    eth_frame.dst_addr = netsim_packets::MacAddr { bytes: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56] };
    eth_frame.src_addr = netsim_packets::MacAddr { bytes: [0x02, 0x15, 0xb2, 0x00, 0x00, 0x00] };
    eth_frame.ethertype = 0x0800.into();
    eth_payload_slice.copy_from_slice(&ip_data);

    let icmp_packet = eth_data;

    let req = SlirpReq::SendPacket(bytes::Bytes::from(icmp_packet));
    let result = actor.handle_action(None, req, &mut ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_slirp_backend_icmp_ping() {
    for backend in [slirp_actor::SlirpBackend::CFfi, slirp_actor::SlirpBackend::Native] {
        run_slirp_backend_icmp_ping_test(backend).await;
    }
}

#[tokio::test]
async fn test_static_ip_allocation_and_reclamation() {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use netsim_model::ChipId;
    use tokio::sync::oneshot;

    let mut actor = SlirpActor::new(Default::default(), None::<String>, None::<String>).await;
    let mut ctx = MockContext;

    async fn allocate_lease(
        actor: &mut SlirpActor,
        chip_id: ChipId,
        ctx: &mut MockContext,
    ) -> Option<slirp_actor::SlirpLease> {
        let (tx, rx) = oneshot::channel();
        actor
            .handle_action(None, SlirpReq::AllocateLease { chip_id, respond_to: tx }, ctx)
            .await
            .unwrap();
        rx.await.unwrap()
    }

    async fn release_lease(actor: &mut SlirpActor, chip_id: ChipId, ctx: &mut MockContext) {
        actor.handle_action(None, SlirpReq::ReleaseLease { chip_id }, ctx).await.unwrap();
    }

    let chip1 = ChipId(1);
    let lease1 =
        allocate_lease(&mut actor, chip1, &mut ctx).await.expect("first allocation should succeed");
    assert_eq!(lease1.v4.ip_address, Ipv4Addr::new(10, 0, 2, 32));
    assert_eq!(lease1.v4.prefixlen, 24);
    assert_eq!(lease1.v4.gateway, Ipv4Addr::new(10, 0, 2, 2));
    let (expected_dns, expected_dns6) = if cfg!(feature = "cuttlefish") {
        (Ipv4Addr::new(10, 0, 2, 2), Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 2))
    } else {
        (Ipv4Addr::new(10, 0, 2, 3), Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 3))
    };
    assert_eq!(lease1.v4.dns, expected_dns);
    assert!(lease1.v6.is_some());
    let v6_1 = lease1.v6.unwrap();
    assert_eq!(v6_1.ip_address, Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0x20));
    assert_eq!(v6_1.prefixlen, 64);
    assert_eq!(v6_1.gateway, Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 2));
    assert_eq!(v6_1.dns, expected_dns6);

    let lease1_repeat = allocate_lease(&mut actor, chip1, &mut ctx)
        .await
        .expect("repeat allocation should succeed");
    assert_eq!(lease1, lease1_repeat);

    let chip2 = ChipId(2);
    let lease2 = allocate_lease(&mut actor, chip2, &mut ctx)
        .await
        .expect("second allocation should succeed");
    assert_eq!(lease2.v4.ip_address, Ipv4Addr::new(10, 0, 2, 33));

    release_lease(&mut actor, chip1, &mut ctx).await;
    let chip3 = ChipId(3);
    let lease3 =
        allocate_lease(&mut actor, chip3, &mut ctx).await.expect("third allocation should succeed");
    assert_eq!(lease3.v4.ip_address, Ipv4Addr::new(10, 0, 2, 32));

    for id in 4..=224 {
        assert!(allocate_lease(&mut actor, ChipId(id), &mut ctx).await.is_some());
    }

    assert!(allocate_lease(&mut actor, ChipId(999), &mut ctx).await.is_none());
}

#[tokio::test]
async fn test_downlink_arp_and_broadcast_scoping() {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use netsim_model::{ChipId, PacketSink};
    use tokio::sync::oneshot;

    let mut actor = SlirpActor::new(Default::default(), None::<String>, None::<String>).await;
    let mut ctx = MockContext;

    let (tx1, mut rx1) = mpsc::unbounded_channel();
    let (_stream_tx1, stream_rx1) = mpsc::unbounded_channel();
    let stream1 = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx1));
    let sink1: PacketSink = Box::pin(futures::sink::unfold(tx1, |tx, bytes| async move {
        let _ = tx.send(bytes);
        Ok(tx)
    }));
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

    let (tx2, mut rx2) = mpsc::unbounded_channel();
    let (_stream_tx2, stream_rx2) = mpsc::unbounded_channel();
    let stream2 = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx2));
    let sink2: PacketSink = Box::pin(futures::sink::unfold(tx2, |tx, bytes| async move {
        let _ = tx.send(bytes);
        Ok(tx)
    }));
    actor
        .handle_action(
            None,
            SlirpReq::Register {
                client_id: 2,
                stream: stream2,
                sink: sink2,
                notifier: None,
                isolated: true,
            },
            &mut ctx,
        )
        .await
        .unwrap();

    let (tx_alloc, rx_alloc) = oneshot::channel();
    actor
        .handle_action(
            None,
            SlirpReq::AllocateLease { chip_id: ChipId(2), respond_to: tx_alloc },
            &mut ctx,
        )
        .await
        .unwrap();
    let lease1 = rx_alloc.await.unwrap().unwrap();
    assert_eq!(lease1.v4.ip_address, Ipv4Addr::new(10, 0, 2, 32));
    assert_eq!(lease1.v6.unwrap().ip_address, Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0x20));

    let static_arp = netsim_packets::build_arp_frame(
        [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
        Ipv4Addr::new(10, 0, 2, 2),
        Ipv4Addr::new(10, 0, 2, 32),
    );
    actor.on_typed_stream(0, static_arp, &mut ctx).await;
    let rx2_pkt = rx2.recv().await;
    assert!(rx2_pkt.is_some(), "isolated client must receive static IP ARP");
    assert!(rx1.try_recv().is_err(), "non-isolated client must not receive static IP ARP");

    let dhcp_arp = netsim_packets::build_arp_frame(
        [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
        Ipv4Addr::new(10, 0, 2, 2),
        Ipv4Addr::new(10, 0, 2, 15),
    );
    actor.on_typed_stream(0, dhcp_arp, &mut ctx).await;
    let rx1_pkt = rx1.recv().await;
    assert!(rx1_pkt.is_some(), "non-isolated client must receive DHCP ARP");
    assert!(rx2.try_recv().is_err(), "isolated client must not receive DHCP ARP");

    let non_arp_bcast = bytes::Bytes::from(vec![
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x08, 0x00,
    ]);
    actor.on_typed_stream(0, non_arp_bcast, &mut ctx).await;
    let rx1_pkt = rx1.recv().await;
    assert!(rx1_pkt.is_some(), "non-isolated client receives LAN broadcast");
    assert!(rx2.try_recv().is_err(), "isolated client must not receive LAN broadcast");

    let unknown_unicast = bytes::Bytes::from(vec![
        0x00, 0x99, 0x88, 0x77, 0x66, 0x55, 0x52, 0x54, 0x00, 0x12, 0x34, 0x56, 0x08, 0x00,
    ]);
    actor.on_typed_stream(0, unknown_unicast, &mut ctx).await;
    assert!(rx1.recv().await.is_some(), "non-isolated client must receive unknown unicast");
    assert!(rx2.try_recv().is_err(), "isolated client must not receive unknown LAN unicast");

    let static_ns = netsim_packets::build_icmpv6_ns_frame(
        [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
        [0x33, 0x33, 0xff, 0x00, 0x00, 0x20],
        Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 2),
        Ipv6Addr::new(0xff02, 0, 0, 0, 0, 1, 0xff00, 0x20),
        Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0x20),
    );
    actor.on_typed_stream(0, static_ns, &mut ctx).await;
    let rx2_v6_pkt = rx2.recv().await;
    assert!(rx2_v6_pkt.is_some(), "isolated client must receive static IPv6 NS");
    assert!(rx1.try_recv().is_err(), "non-isolated client must not receive static IPv6 NS");

    let lan_ns = netsim_packets::build_icmpv6_ns_frame(
        [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
        [0x33, 0x33, 0xff, 0x00, 0x00, 0x15],
        Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 2),
        Ipv6Addr::new(0xff02, 0, 0, 0, 0, 1, 0xff00, 0x15),
        Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0x15),
    );
    actor.on_typed_stream(0, lan_ns, &mut ctx).await;
    let rx1_v6_pkt = rx1.recv().await;
    assert!(rx1_v6_pkt.is_some(), "non-isolated client must receive LAN IPv6 NS");
    assert!(rx2.try_recv().is_err(), "isolated client must not receive LAN IPv6 NS");
}
