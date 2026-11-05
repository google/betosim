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

//! Tier 2 BDD `World` harness for `SlirpActor` and `SlirpClient` (b/577212628).
//!
//! Wraps a live `SlirpActor` event loop and `SlirpClient` handle, managing
//! registered guest endpoints, static lease allocations, disconnect
//! notifications, loopback TCP target sockets, and synthetic downlink frame
//! injection behind `given_*`, `when_*`, and `then_*` step methods.
//!
//! Why `World` coexists alongside `SlirpTestDriver` (`tests/test_driver.rs`):
//! - `SlirpTestDriver` drives `SlirpActor::handle_action` directly on a single
//!   pre-registered client `0` without spawning the actor loop, which keeps
//!   existing protocol-level unit/integration tests lightweight.
//! - `World` runs the full asynchronous actor loop over `ResourceClient` and
//!   `SlirpClient` with multiple dynamically registered/unregistered clients,
//!   disconnect notification channels, and a synthetic downlink typed stream.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use actor_framework::{
    ActorLifecycle, ActorService, BoxStream, BoxTypedStream, Context, ResourceClient,
    ResourceRequest, TimerCallback, TimerKey,
};
use bytes::Bytes;
use futures::future::BoxFuture;
use netsim_model::{ChipId, PacketSink};
use netsim_packets::{
    ArpPacket, EthernetFrame, IP_P_ICMP, IP_P_TCP, IcmpType, Icmpv6Type, IpPacket, PacketBuilder,
    TCP_FLAG_ACK, TCP_FLAG_FIN, TCP_FLAG_RST, TCP_FLAG_SYN, TransportPacket, arp_op,
    build_arp_frame, build_icmpv6_ns_frame, ether_type, ipv4_checksum, parse,
};
use slirp_actor::{SlirpActor, SlirpBackend, SlirpClient, SlirpConfig};
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tokio_stream::{StreamExt, StreamMap, StreamNotifyClose, wrappers::UnboundedReceiverStream};

const GATEWAY_MAC: [u8; 6] = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35];
const UNKNOWN_UNICAST_MAC: [u8; 6] = [0x02, 0x99, 0x99, 0x99, 0x99, 0x99];
const GATEWAY_IPV4: Ipv4Addr = Ipv4Addr::new(10, 0, 2, 2);
const GATEWAY_IPV6: Ipv6Addr = Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 2);
const STEP_TIMEOUT: Duration = Duration::from_secs(3);

/// Guest registration configuration for [`World::given_registered_client`].
#[derive(Clone, Copy, Debug)]
pub struct ClientConfig {
    isolated: bool,
}

impl ClientConfig {
    /// Standard shared-LAN guest configuration (`isolated: false`).
    pub fn lan() -> Self {
        Self { isolated: false }
    }

    /// Isolated cellular/point-to-point guest configuration (`isolated: true`),
    /// which also allocates a static IPv4/IPv6 lease via
    /// [`SlirpClient::allocate_lease`].
    pub fn isolated() -> Self {
        Self { isolated: true }
    }
}

struct ClientEndpoint {
    mac: [u8; 6],
    ipv4: Ipv4Addr,
    ipv6: Ipv6Addr,
    uplink_tx: Option<mpsc::UnboundedSender<Bytes>>,
    downlink_rx: mpsc::UnboundedReceiver<Bytes>,
}

struct WorldContext {
    streams: StreamMap<u32, StreamNotifyClose<BoxStream>>,
    typed_streams: StreamMap<usize, StreamNotifyClose<BoxTypedStream<Bytes>>>,
    tasks: tokio::task::JoinSet<u32>,
    task_handles: HashMap<u32, tokio::task::AbortHandle>,
    active_backend_stream_id: Option<usize>,
    active_backend_task_id: Option<u32>,
    expecting_backend_task: bool,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl WorldContext {
    fn new() -> (Self, oneshot::Receiver<()>) {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        (
            Self {
                streams: StreamMap::new(),
                typed_streams: StreamMap::new(),
                tasks: tokio::task::JoinSet::new(),
                task_handles: HashMap::new(),
                active_backend_stream_id: None,
                active_backend_task_id: None,
                expecting_backend_task: false,
                shutdown_tx: Some(shutdown_tx),
            },
            shutdown_rx,
        )
    }
}

impl Context<SlirpActor> for WorldContext {
    fn set_interval(&mut self, _duration: Duration) {}

    fn add_stream(&mut self, id: u32, stream: BoxStream) {
        self.expecting_backend_task = false;
        self.streams.insert(id, StreamNotifyClose::new(stream));
    }

    fn remove_stream(&mut self, id: u32) {
        self.streams.remove(&id);
    }

    fn add_typed_stream(&mut self, id: usize, stream: BoxTypedStream<Bytes>) {
        if id >= 1 {
            if let Some(prev_stream_id) = self.active_backend_stream_id.replace(id) {
                self.typed_streams.remove(&prev_stream_id);
            }
            if let Some(prev_task_id) = self.active_backend_task_id.take() {
                self.abort(prev_task_id);
            }
            self.expecting_backend_task = true;
        }
        self.typed_streams.insert(id, StreamNotifyClose::new(stream));
    }

    fn remove_typed_stream(&mut self, id: usize) {
        self.typed_streams.remove(&id);
    }

    fn spawn(&mut self, id: u32, task: BoxFuture<'static, u32>) {
        if self.expecting_backend_task {
            self.expecting_backend_task = false;
            self.active_backend_task_id = Some(id);
        }
        let handle = self.tasks.spawn(task);
        self.task_handles.insert(id, handle);
    }

    fn abort(&mut self, id: u32) {
        if let Some(handle) = self.task_handles.remove(&id) {
            handle.abort();
        }
    }

    fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    fn run_later(&mut self, _duration: Duration, _f: TimerCallback<SlirpActor>) -> TimerKey {
        unimplemented!("timers are not scheduled directly by SlirpActor")
    }

    fn cancel_timer(&mut self, _key: TimerKey) {}
}

async fn dispatch_request(
    actor: &mut SlirpActor,
    req: ResourceRequest<SlirpActor>,
    ctx: &mut WorldContext,
) {
    match req {
        ResourceRequest::Create { params, id, respond_to } => {
            let _ = respond_to.send(actor.handle_create(id, params, ctx).await);
        }
        ResourceRequest::Get { id, respond_to } => {
            let _ = respond_to.send(actor.handle_get(id, ctx).await);
        }
        ResourceRequest::Update { id, update, respond_to } => {
            let _ = respond_to.send(actor.handle_update(id, update, ctx).await);
        }
        ResourceRequest::Delete { id, respond_to } => {
            let _ = respond_to.send(actor.handle_delete(id, ctx).await);
        }
        ResourceRequest::Action { id, action, respond_to } => {
            let _ = respond_to.send(actor.handle_action(id, action, ctx).await);
        }
        ResourceRequest::List { respond_to } => {
            let _ = respond_to.send(actor.handle_list(ctx).await);
        }
        ResourceRequest::Shutdown { respond_to } => {
            ctx.shutdown();
            let _ = respond_to.send(Ok(()));
        }
    }
}

/// Tier 2 BDD test harness for `SlirpActor` and `SlirpClient`.
pub struct World {
    client: SlirpClient,
    raw_client: ResourceClient<SlirpActor>,
    downlink_tx: mpsc::UnboundedSender<Bytes>,
    disconnect_tx: mpsc::UnboundedSender<ChipId>,
    disconnect_rx: mpsc::UnboundedReceiver<ChipId>,
    loopback_addr: SocketAddr,
    loopback_accept_handle: JoinHandle<()>,
    clients: HashMap<u32, ClientEndpoint>,
    actor_handle: JoinHandle<()>,
}

impl World {
    /// Creates a new `World` running `SlirpActor` with the requested backend.
    ///
    /// Runs a `biased;` event loop with typed stream `0` pre-registered for
    /// synthetic downlink frame injection and `req_rx` polled after stream
    /// branches so [`Self::sync_actor`] acts as a deterministic barrier.
    pub async fn new(backend: SlirpBackend) -> Self {
        let (req_tx, mut req_rx) = mpsc::channel::<ResourceRequest<SlirpActor>>(256);
        let raw_client = ResourceClient::new(req_tx);
        let client = SlirpClient::new(raw_client.clone());

        let (downlink_tx, downlink_rx) = mpsc::unbounded_channel::<Bytes>();
        let (disconnect_tx, disconnect_rx) = mpsc::unbounded_channel::<ChipId>();

        let listener =
            TcpListener::bind("127.0.0.1:0").await.expect("loopback TcpListener should bind");
        let loopback_addr = listener.local_addr().expect("loopback listener should have addr");
        let loopback_accept_handle = tokio::spawn(async move {
            let mut accepted = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                accepted.push(stream);
            }
        });

        let mut actor =
            SlirpActor::new_with_backend(SlirpConfig::default(), None, None, backend).await;
        let (mut ctx, mut shutdown_rx) = WorldContext::new();
        // Stream ID 0 is reserved for synthetic downlink frame injection;
        // SlirpActor::next_stream_id starts at 1.
        ctx.add_typed_stream(0, Box::pin(UnboundedReceiverStream::new(downlink_rx)));

        let actor_handle = tokio::spawn(async move {
            actor.on_start(&mut ctx).await;
            loop {
                tokio::select! {
                    biased;
                    Some((id, msg_opt)) = ctx.streams.next(), if !ctx.streams.is_empty() => {
                        match msg_opt {
                            Some(msg) => actor.on_stream(id, msg, &mut ctx).await,
                            None => actor.on_stream_closed(id, &mut ctx).await,
                        }
                    }
                    Some((id, msg_opt)) = ctx.typed_streams.next(),
                        if !ctx.typed_streams.is_empty() =>
                    {
                        match msg_opt {
                            Some(msg) => actor.on_typed_stream(id, msg, &mut ctx).await,
                            None => {
                                ctx.remove_typed_stream(id);
                                actor.on_typed_stream_closed(id, &mut ctx).await;
                            }
                        }
                    }
                    Some(res) = ctx.tasks.join_next(), if !ctx.tasks.is_empty() => {
                        if let Ok(id) = res {
                            actor.on_task_closed(id, &mut ctx).await;
                        }
                    }
                    Some(req) = req_rx.recv() => {
                        dispatch_request(&mut actor, req, &mut ctx).await;
                    }
                    _ = &mut shutdown_rx => {
                        actor.on_shutdown().await;
                        drop(actor);
                        ctx.tasks.abort_all();
                        ctx.task_handles.clear();
                        break;
                    }
                }
            }
        });

        Self {
            client,
            raw_client,
            downlink_tx,
            disconnect_tx,
            disconnect_rx,
            loopback_addr,
            loopback_accept_handle,
            clients: HashMap::new(),
            actor_handle,
        }
    }

    /// Registers a guest client via [`SlirpClient::register`] and, when
    /// isolated, allocates a static IPv4/IPv6 lease via
    /// [`SlirpClient::allocate_lease`].
    pub async fn given_registered_client(&mut self, client_id: u32, config: ClientConfig) {
        let mac = [0x02, 0x15, 0xb2, 0x00, 0x00, client_id as u8];
        let (ipv4, ipv6) = if config.isolated {
            let lease = self
                .client
                .allocate_lease(ChipId(client_id))
                .await
                .expect("allocate_lease RPC should succeed")
                .expect("static IP pool should have available lease");
            let v6_addr = lease.v6.expect("IPv6 lease should be present").ip_address;
            (lease.v4.ip_address, v6_addr)
        } else {
            (
                Ipv4Addr::new(10, 0, 2, 14 + client_id as u8),
                Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0x10 + client_id as u16),
            )
        };

        let (uplink_tx, uplink_rx) = mpsc::unbounded_channel::<Bytes>();
        let (downlink_out_tx, downlink_rx) = mpsc::unbounded_channel::<Bytes>();
        let stream = Box::pin(UnboundedReceiverStream::new(uplink_rx));
        let sink: PacketSink =
            Box::pin(futures::sink::unfold(downlink_out_tx, |tx, bytes| async move {
                let _ = tx.send(bytes);
                Ok(tx)
            }));

        self.client
            .register(client_id, stream, sink, Some(self.disconnect_tx.clone()), config.isolated)
            .await
            .expect("register RPC should succeed");

        self.clients.insert(
            client_id,
            ClientEndpoint { mac, ipv4, ipv6, uplink_tx: Some(uplink_tx), downlink_rx },
        );
    }

    /// Unregisters a guest client via [`SlirpClient::unregister`].
    pub async fn when_client_is_unregistered(&mut self, client_id: u32) {
        self.client.unregister(client_id).await.expect("unregister RPC should succeed");
    }

    /// Drops the guest's uplink packet sender to simulate an unexpected channel
    /// disconnect.
    pub async fn when_client_uplink_channel_drops(&mut self, client_id: u32) {
        if let Some(endpoint) = self.clients.get_mut(&client_id) {
            endpoint.uplink_tx.take();
        }
        self.sync_actor().await;
    }

    /// Releases a guest's static lease via [`SlirpClient::release_lease`].
    pub async fn when_client_releases_lease(&mut self, client_id: u32) {
        self.client
            .release_lease(ChipId(client_id))
            .await
            .expect("release_lease RPC should succeed");
    }

    /// Dynamically switches the active SLIRP backend via
    /// [`SlirpClient::switch_backend`].
    pub async fn when_backend_is_switched(&mut self, backend: SlirpBackend) {
        self.client.switch_backend(backend).await.expect("switch_backend RPC should succeed");
        self.sync_actor().await;
    }

    /// Shuts down `SlirpActor` cleanly via [`SlirpClient::shutdown`].
    pub async fn when_actor_is_shut_down(&mut self) {
        self.client.shutdown().await.expect("SlirpClient::shutdown should succeed");
    }

    /// Sends an ARP Who-Has request for the SLIRP gateway (`10.0.2.2`) from
    /// `client_id`'s uplink stream.
    pub async fn when_client_queries_gateway_arp(&mut self, client_id: u32) {
        let endpoint = self.clients.get(&client_id).expect("client must be registered");
        let arp_req = build_arp_frame(endpoint.mac, endpoint.ipv4, GATEWAY_IPV4);
        endpoint
            .uplink_tx
            .as_ref()
            .expect("client uplink channel must be open")
            .send(arp_req)
            .expect("uplink send should succeed");
        self.sync_actor().await;
    }

    /// Sends an ICMPv4 Echo Request to the SLIRP gateway (`10.0.2.2`) from
    /// `client_id`'s uplink stream.
    pub async fn when_client_pings_gateway(&mut self, client_id: u32, seq: u16) {
        let endpoint = self.clients.get(&client_id).expect("client must be registered");
        let mut icmp_bytes = [0u8; 12];
        icmp_bytes[0] = IcmpType::EchoRequest as u8;
        icmp_bytes[4..6].copy_from_slice(&0xBEEFu16.to_be_bytes());
        icmp_bytes[6..8].copy_from_slice(&seq.to_be_bytes());
        icmp_bytes[8..12].copy_from_slice(b"ping");
        let csum = ipv4_checksum(&icmp_bytes);
        icmp_bytes[2..4].copy_from_slice(&csum.to_be_bytes());

        let frame = Bytes::from(
            PacketBuilder::new(GATEWAY_MAC, endpoint.mac, ether_type::IPV4)
                .ipv4(endpoint.ipv4.octets(), GATEWAY_IPV4.octets(), IP_P_ICMP)
                .payload(&icmp_bytes),
        );
        endpoint
            .uplink_tx
            .as_ref()
            .expect("client uplink channel must be open")
            .send(frame)
            .expect("uplink send should succeed");
        self.sync_actor().await;
    }

    /// Injects a downlink unicast frame addressed to an unknown unicast MAC so
    /// `SlirpActor::on_typed_stream` exercises its unknown-unicast flood path.
    pub async fn when_downlink_unknown_unicast_arrives(&mut self) {
        let unicast = Bytes::from(
            PacketBuilder::new(UNKNOWN_UNICAST_MAC, GATEWAY_MAC, ether_type::IPV4)
                .ipv4(GATEWAY_IPV4.octets(), [10, 0, 2, 99], 17)
                .udp(4000, 4001)
                .payload(b"unknown-unicast"),
        );
        self.downlink_tx.send(unicast).expect("downlink unknown-unicast send should succeed");
        self.sync_actor().await;
    }

    /// Injects a non-ARP LAN broadcast frame on the gateway downlink stream.
    pub async fn when_downlink_lan_broadcast_arrives(&mut self) {
        let bcast = Bytes::from(
            PacketBuilder::new([0xff; 6], GATEWAY_MAC, ether_type::IPV4)
                .ipv4(GATEWAY_IPV4.octets(), [255, 255, 255, 255], 17)
                .udp(5000, 5001)
                .payload(b"lan-broadcast"),
        );
        self.downlink_tx.send(bcast).expect("downlink send should succeed");
        self.sync_actor().await;
    }

    /// Injects a downlink ARP request targeting `client_id`'s IPv4 address.
    pub async fn when_downlink_arp_targets_client(&mut self, client_id: u32) {
        let target_ip = self.clients.get(&client_id).expect("client must be registered").ipv4;
        let arp = build_arp_frame(GATEWAY_MAC, GATEWAY_IPV4, target_ip);
        self.downlink_tx.send(arp).expect("downlink ARP send should succeed");
        self.sync_actor().await;
    }

    /// Injects a downlink ICMPv6 Neighbor Solicitation targeting `client_id`'s
    /// IPv6 address.
    pub async fn when_downlink_ipv6_ns_targets_client(&mut self, client_id: u32) {
        let target_ip = self.clients.get(&client_id).expect("client must be registered").ipv6;
        let octets = target_ip.octets();
        let solicited_mac = [0x33, 0x33, 0xff, octets[13], octets[14], octets[15]];
        let solicited_ip = Ipv6Addr::new(
            0xff02,
            0,
            0,
            0,
            0,
            1,
            0xff00 | u16::from(octets[13]),
            u16::from_be_bytes([octets[14], octets[15]]),
        );
        let ns = build_icmpv6_ns_frame(
            GATEWAY_MAC,
            solicited_mac,
            GATEWAY_IPV6,
            solicited_ip,
            target_ip,
        );
        self.downlink_tx.send(ns).expect("downlink IPv6 NS send should succeed");
        self.sync_actor().await;
    }

    /// Establishes a TCP connection from `client_id:src_port` to the `World`'s
    /// loopback `TcpListener` (`SYN` -> `SYN-ACK` -> `ACK`).
    pub async fn when_client_establishes_loopback_tcp_flow(
        &mut self,
        client_id: u32,
        src_port: u16,
        initial_seq: u32,
    ) {
        self.when_client_sends_loopback_tcp_syn(client_id, src_port, initial_seq).await;
        let server_seq = self
            .recv_loopback_syn_ack(client_id, src_port, initial_seq.wrapping_add(1))
            .await
            .expect("client should receive SYN-ACK from loopback TCP listener");

        let endpoint = self.clients.get(&client_id).expect("client must be registered");
        let dst = self.loopback_ipv4_target();
        let ack_pkt = build_tcp_segment(
            endpoint.mac,
            endpoint.ipv4,
            (dst.0, src_port, dst.1),
            (initial_seq.wrapping_add(1), server_seq.wrapping_add(1)),
            TCP_FLAG_ACK,
            &[],
        );
        endpoint
            .uplink_tx
            .as_ref()
            .expect("client uplink channel must be open")
            .send(ack_pkt)
            .expect("uplink ACK send should succeed");
        self.sync_actor().await;
    }

    /// Sends a TCP `SYN` from `client_id:src_port` to the `World`'s loopback
    /// `TcpListener`.
    pub async fn when_client_sends_loopback_tcp_syn(
        &mut self,
        client_id: u32,
        src_port: u16,
        seq: u32,
    ) {
        let endpoint = self.clients.get(&client_id).expect("client must be registered");
        let dst = self.loopback_ipv4_target();
        let syn = build_tcp_segment(
            endpoint.mac,
            endpoint.ipv4,
            (dst.0, src_port, dst.1),
            (seq, 0),
            TCP_FLAG_SYN,
            &[],
        );
        endpoint
            .uplink_tx
            .as_ref()
            .expect("client uplink channel must be open")
            .send(syn)
            .expect("uplink SYN send should succeed");
        self.sync_actor().await;
    }

    /// Drives `stream_count` concurrent multi-stream TCP flows against the
    /// loopback `TcpListener`: establishes each handshake (`SYN` -> `SYN-ACK`
    /// -> `ACK` with payload) and tears down even flows with `RST` and odd
    /// flows with `FIN | ACK`, interleaving `SlirpClient::send_packet` and the
    /// guest uplink stream.
    pub async fn when_concurrent_tcp_streams_stress(&mut self, client_id: u32, stream_count: u16) {
        let syn_handles: Vec<_> = self
            .concurrent_stream_contexts(client_id, stream_count)
            .into_iter()
            .map(|s| {
                tokio::spawn(async move {
                    let syn = build_tcp_segment(
                        s.src_mac,
                        s.src_ip,
                        (s.dst.0, s.src_port, s.dst.1),
                        (s.seq_base, 0),
                        TCP_FLAG_SYN,
                        &[],
                    );
                    s.send(syn).await;
                })
            })
            .collect();

        for handle in syn_handles {
            handle.await.expect("concurrent TCP SYN task should not panic");
        }
        self.sync_actor().await;

        let mut server_seqs = HashMap::with_capacity(usize::from(stream_count));
        while server_seqs.len() < usize::from(stream_count) {
            let pkt = self
                .recv_matching(client_id, |bytes| {
                    let Some(parsed) = parse(bytes) else {
                        return false;
                    };
                    let Some(TransportPacket::Tcp(tcp, _)) = parsed.transport else {
                        return false;
                    };
                    let dst_port = tcp.dest_port.get();
                    tcp.syn()
                        && tcp.ack()
                        && (25000..25000 + stream_count).contains(&dst_port)
                        && !server_seqs.contains_key(&dst_port)
                })
                .await
                .expect("timed out waiting for SYN-ACK on concurrent TCP stream");
            let parsed = parse(&pkt).expect("SYN-ACK frame must parse");
            if let Some(TransportPacket::Tcp(tcp, _)) = parsed.transport {
                server_seqs.insert(tcp.dest_port.get(), tcp.sequence_num.get());
            }
        }

        let teardown_handles: Vec<_> = self
            .concurrent_stream_contexts(client_id, stream_count)
            .into_iter()
            .map(|s| {
                let srv_seq = *server_seqs.get(&s.src_port).expect("server ISN recorded");
                tokio::spawn(async move {
                    let endpoints = (s.dst.0, s.src_port, s.dst.1);
                    let payload = b"GET / HTTP/1.1\r\n\r\n";
                    let data = build_tcp_segment(
                        s.src_mac,
                        s.src_ip,
                        endpoints,
                        (s.seq_base + 1, srv_seq.wrapping_add(1)),
                        TCP_FLAG_ACK,
                        payload,
                    );
                    let teardown_flags = if s.offset.is_multiple_of(2) {
                        TCP_FLAG_RST
                    } else {
                        TCP_FLAG_FIN | TCP_FLAG_ACK
                    };
                    let teardown = build_tcp_segment(
                        s.src_mac,
                        s.src_ip,
                        endpoints,
                        (s.seq_base + 1 + payload.len() as u32, srv_seq.wrapping_add(1)),
                        teardown_flags,
                        &[],
                    );
                    s.send(data).await;
                    s.send(teardown).await;
                })
            })
            .collect();

        for handle in teardown_handles {
            handle.await.expect("concurrent TCP teardown task should not panic");
        }
        self.sync_actor().await;
    }

    /// Asserts that `client_id` receives an ARP Reply from the SLIRP gateway.
    pub async fn then_client_receives_gateway_arp_reply(&mut self, client_id: u32) {
        let expected_ip = self.clients.get(&client_id).expect("client must be registered").ipv4;
        let pkt = self
            .recv_matching(client_id, |bytes| {
                let Some(arp) = parse_arp_frame(bytes) else {
                    return false;
                };
                arp.opcode.get() == arp_op::REPLY
                    && Ipv4Addr::from(arp.sender_protocol_addr) == GATEWAY_IPV4
                    && Ipv4Addr::from(arp.target_protocol_addr) == expected_ip
            })
            .await;
        assert!(pkt.is_some(), "client {client_id} did not receive gateway ARP reply");
    }

    /// Asserts that `client_id` receives an ICMPv4 Echo Reply from the SLIRP
    /// gateway with the expected sequence number.
    pub async fn then_client_receives_gateway_icmp_reply(
        &mut self,
        client_id: u32,
        expected_seq: u16,
    ) {
        let expected_ip = self.clients.get(&client_id).expect("client must be registered").ipv4;
        let pkt = self
            .recv_matching(client_id, |bytes| {
                let Some(TransportPacket::Icmp(icmp_hdr, _)) =
                    parse_ipv4_transport(bytes, GATEWAY_IPV4, Some(expected_ip))
                else {
                    return false;
                };
                let seq = u16::from_be_bytes([icmp_hdr.rest[2], icmp_hdr.rest[3]]);
                icmp_hdr.icmp_type == IcmpType::EchoReply as u8 && seq == expected_seq
            })
            .await;
        assert!(
            pkt.is_some(),
            "client {client_id} did not receive gateway ICMP Echo Reply (seq={expected_seq})"
        );
    }

    /// Verifies that `client_id` resolves the SLIRP gateway via ARP and
    /// exchanges an ICMPv4 Echo Request/Reply with sequence number `seq`.
    pub async fn then_client_has_gateway_connectivity(&mut self, client_id: u32, seq: u16) {
        self.when_client_queries_gateway_arp(client_id).await;
        self.then_client_receives_gateway_arp_reply(client_id).await;
        self.when_client_pings_gateway(client_id, seq).await;
        self.then_client_receives_gateway_icmp_reply(client_id, seq).await;
    }

    /// Asserts that `client_id` receives a TCP `SYN-ACK` from the loopback
    /// listener on `src_port` acknowledging `expected_ack`.
    pub async fn then_client_receives_loopback_tcp_syn_ack(
        &mut self,
        client_id: u32,
        src_port: u16,
        expected_ack: u32,
    ) {
        let syn_ack = self.recv_loopback_syn_ack(client_id, src_port, expected_ack).await;
        assert!(
            syn_ack.is_some(),
            "client {client_id} missed TCP SYN-ACK on port {src_port} (ack={expected_ack})"
        );
    }

    /// Asserts that all `RST`-torn-down 4-tuples from
    /// [`Self::when_concurrent_tcp_streams_stress`] accept a fresh `SYN` and
    /// return a new `SYN-ACK`.
    pub async fn then_rst_torn_down_tcp_streams_accept_reuse_syn(
        &mut self,
        client_id: u32,
        stream_count: u16,
    ) {
        for offset in (0..stream_count).step_by(2) {
            let src_port = 25000 + offset;
            let reuse_seq = 500_000 + u32::from(offset) * 100;
            self.when_client_sends_loopback_tcp_syn(client_id, src_port, reuse_seq).await;
            self.then_client_receives_loopback_tcp_syn_ack(
                client_id,
                src_port,
                reuse_seq.wrapping_add(1),
            )
            .await;
        }
    }

    /// Asserts that `client_id` receives a downlink ARP Request targeting
    /// `target_client_id`'s IPv4 address.
    pub async fn then_client_receives_arp_for_client(
        &mut self,
        client_id: u32,
        target_client_id: u32,
    ) {
        let target_ip =
            self.clients.get(&target_client_id).expect("target client must be registered").ipv4;
        let pkt = self
            .recv_matching(client_id, |bytes| {
                let Some(arp) = parse_arp_frame(bytes) else {
                    return false;
                };
                arp.opcode.get() == arp_op::REQUEST
                    && Ipv4Addr::from(arp.target_protocol_addr) == target_ip
            })
            .await;
        assert!(
            pkt.is_some(),
            "client {client_id} did not receive ARP request for client {target_client_id}"
        );
    }

    /// Asserts that `client_id` receives a downlink ICMPv6 Neighbor
    /// Solicitation targeting `target_client_id`'s IPv6 address.
    pub async fn then_client_receives_ipv6_ns_for_client(
        &mut self,
        client_id: u32,
        target_client_id: u32,
    ) {
        let target_ip =
            self.clients.get(&target_client_id).expect("target client must be registered").ipv6;
        let expected_octets = target_ip.octets();
        let pkt = self
            .recv_matching(client_id, |bytes| {
                let Some(parsed) = parse(bytes) else {
                    return false;
                };
                let Some(TransportPacket::Icmpv6(hdr, payload)) = parsed.transport else {
                    return false;
                };
                hdr.icmpv6_type == Icmpv6Type::NeighborSolicitation as u8
                    && payload.get(..16) == Some(&expected_octets[..])
            })
            .await;
        assert!(
            pkt.is_some(),
            "client {client_id} did not receive IPv6 NS for client {target_client_id} ({target_ip})"
        );
    }

    /// Asserts that `client_id` receives the flooded unknown-unicast frame.
    pub async fn then_client_receives_unknown_unicast_frame(&mut self, client_id: u32) {
        let pkt = self
            .recv_matching(client_id, |bytes| {
                let Some((eth, _)) = EthernetFrame::parse(bytes) else {
                    return false;
                };
                eth.dst_addr.bytes == UNKNOWN_UNICAST_MAC
            })
            .await;
        assert!(pkt.is_some(), "client {client_id} did not receive flooded unknown-unicast frame");
    }

    /// Asserts that `client_id` receives a broadcast Ethernet frame.
    pub async fn then_client_receives_broadcast_frame(&mut self, client_id: u32) {
        let pkt = self
            .recv_matching(client_id, |bytes| {
                let Some((eth, _)) = EthernetFrame::parse(bytes) else {
                    return false;
                };
                eth.dst_addr.is_broadcast()
            })
            .await;
        assert!(pkt.is_some(), "client {client_id} did not receive broadcast frame");
    }

    /// Asserts that `client_id` has no pending downlink frames after actor
    /// synchronization.
    pub async fn then_client_receives_no_frame(&mut self, client_id: u32) {
        self.sync_actor().await;
        let endpoint = self.clients.get_mut(&client_id).expect("client must be registered");
        let received = endpoint.downlink_rx.try_recv();
        assert!(
            received.is_err(),
            "expected no frame for client {client_id}, but received {} bytes",
            received.map(|b| b.len()).unwrap_or(0)
        );
    }

    /// Asserts that no disconnect notification has been emitted yet.
    pub fn then_no_disconnect_notification_is_pending(&mut self) {
        assert!(
            self.disconnect_rx.try_recv().is_err(),
            "unexpected disconnect notification received"
        );
    }

    /// Asserts that `notify_disconnected` fired for `expected_chip`.
    pub async fn then_disconnect_notification_fired_for(&mut self, expected_chip: ChipId) {
        let notified = tokio::time::timeout(STEP_TIMEOUT, self.disconnect_rx.recv())
            .await
            .expect("timed out waiting for disconnect notification")
            .expect("disconnect notification channel closed unexpectedly");
        assert_eq!(notified, expected_chip);
    }

    fn loopback_ipv4_target(&self) -> (Ipv4Addr, u16) {
        let IpAddr::V4(ip) = self.loopback_addr.ip() else {
            panic!("loopback listener must bind IPv4");
        };
        (ip, self.loopback_addr.port())
    }

    fn concurrent_stream_contexts(
        &self,
        client_id: u32,
        stream_count: u16,
    ) -> Vec<ConcurrentStreamCtx> {
        let endpoint = self.clients.get(&client_id).expect("client must be registered");
        let uplink_tx = endpoint.uplink_tx.as_ref().expect("uplink channel must be open");
        let dst = self.loopback_ipv4_target();
        (0..stream_count)
            .map(|offset| ConcurrentStreamCtx {
                offset,
                src_port: 25000 + offset,
                seq_base: 10_000 + u32::from(offset) * 1000,
                src_mac: endpoint.mac,
                src_ip: endpoint.ipv4,
                dst,
                client: self.client.clone(),
                tx: uplink_tx.clone(),
            })
            .collect()
    }

    async fn recv_loopback_syn_ack(
        &mut self,
        client_id: u32,
        src_port: u16,
        expected_ack: u32,
    ) -> Option<u32> {
        let (dst_ip, dst_port) = self.loopback_ipv4_target();
        let pkt = self
            .recv_matching(client_id, |bytes| {
                let Some(TransportPacket::Tcp(tcp, _)) = parse_ipv4_transport(bytes, dst_ip, None)
                else {
                    return false;
                };
                tcp.source_port.get() == dst_port
                    && tcp.dest_port.get() == src_port
                    && tcp.syn()
                    && tcp.ack()
                    && tcp.ack_num.get() == expected_ack
            })
            .await?;
        let parsed = parse(&pkt)?;
        let Some(TransportPacket::Tcp(tcp, _)) = parsed.transport else {
            return None;
        };
        Some(tcp.sequence_num.get())
    }

    async fn sync_actor(&self) {
        let status = self
            .raw_client
            .get(0)
            .await
            .expect("actor get(0) barrier should succeed")
            .expect("SlirpStatus entity should be returned");
        assert!(status.initialized, "SlirpActor backend should be initialized");
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }

    async fn recv_matching(
        &mut self,
        client_id: u32,
        mut predicate: impl FnMut(&Bytes) -> bool,
    ) -> Option<Bytes> {
        let endpoint = self.clients.get_mut(&client_id).expect("client must be registered");
        let fut = async {
            while let Some(pkt) = endpoint.downlink_rx.recv().await {
                if predicate(&pkt) {
                    return Some(pkt);
                }
            }
            None
        };
        tokio::time::timeout(STEP_TIMEOUT, fut).await.ok().flatten()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.clients.clear();
        self.loopback_accept_handle.abort();
        self.actor_handle.abort();
    }
}

struct ConcurrentStreamCtx {
    offset: u16,
    src_port: u16,
    seq_base: u32,
    src_mac: [u8; 6],
    src_ip: Ipv4Addr,
    dst: (Ipv4Addr, u16),
    client: SlirpClient,
    tx: mpsc::UnboundedSender<Bytes>,
}

impl ConcurrentStreamCtx {
    async fn send(&self, pkt: Bytes) {
        if self.offset.is_multiple_of(3) {
            self.client.send_packet(pkt).await;
        } else {
            let _ = self.tx.send(pkt);
        }
    }
}

fn parse_ipv4_transport(
    bytes: &Bytes,
    expected_src: Ipv4Addr,
    expected_dst: Option<Ipv4Addr>,
) -> Option<TransportPacket<'_>> {
    let parsed = parse(bytes)?;
    let Some(IpPacket::V4(ip_hdr, _)) = parsed.ip else {
        return None;
    };
    if Ipv4Addr::from(ip_hdr.source_addr) != expected_src {
        return None;
    }
    if expected_dst.is_some_and(|dst| Ipv4Addr::from(ip_hdr.dest_addr) != dst) {
        return None;
    }
    parsed.transport
}

fn parse_arp_frame(bytes: &Bytes) -> Option<&ArpPacket> {
    let (eth, rem) = EthernetFrame::parse(bytes)?;
    if eth.ethertype.get() != ether_type::ARP {
        return None;
    }
    ArpPacket::parse(rem).map(zerocopy::Ref::into_ref)
}

fn build_tcp_segment(
    src_mac: [u8; 6],
    src_ip: Ipv4Addr,
    endpoints: (Ipv4Addr, u16, u16),
    seq_ack: (u32, u32),
    flags: u16,
    payload: &[u8],
) -> Bytes {
    Bytes::from(
        PacketBuilder::new(GATEWAY_MAC, src_mac, ether_type::IPV4)
            .ipv4(src_ip.octets(), endpoints.0.octets(), IP_P_TCP)
            .tcp(endpoints.1, endpoints.2, seq_ack.0, seq_ack.1, flags, 8192)
            .payload(payload),
    )
}
