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

//! Backend-agnostic Slirp Test Driver.
//!
//! Encapsulates all underlying messaging/actor framework logic so that test
//! cases are written purely against a clean packet I/O interface (`send_packet`
//! / `recv_packet`).

use std::time::Duration;

use actor_framework::{ActorLifecycle, ActorService, BoxStream, BoxTypedStream, Context};
use futures::future::BoxFuture;
use slirp_actor::{SlirpActor, SlirpBackend, SlirpReq};
use tokio::sync::mpsc;
use tokio_stream::{StreamExt, StreamMap};

/// A minimal `Context` for driving `SlirpActor` outside the actor runtime.
///
/// Unlike a throwaway stub, this retains the typed streams the actor registers.
/// `SlirpActor` publishes its backend's downlink channel via
/// `ctx.add_typed_stream(..)` and relies on the runtime to poll it and call
/// `on_typed_stream`, which is what forwards frames to each client's sink.
/// Dropping those streams silently disables the entire downlink path, so
/// `SlirpTestDriver::recv_packet` pumps them here instead.
#[derive(Default)]
pub struct MockContext {
    typed_streams: StreamMap<usize, BoxTypedStream<bytes::Bytes>>,
}

impl Context<SlirpActor> for MockContext {
    fn set_interval(&mut self, _duration: Duration) {}
    fn add_stream(&mut self, _id: u32, _stream: BoxStream) {}
    fn remove_stream(&mut self, _id: u32) {}
    fn add_typed_stream(&mut self, id: usize, stream: BoxTypedStream<bytes::Bytes>) {
        self.typed_streams.insert(id, stream);
    }
    fn remove_typed_stream(&mut self, id: usize) {
        self.typed_streams.remove(&id);
    }
    fn spawn(&mut self, _id: u32, task: BoxFuture<'static, u32>) {
        tokio::spawn(task);
    }
    fn abort(&mut self, _id: u32) {}
    fn shutdown(&mut self) {}
    fn run_later(
        &mut self,
        _duration: Duration,
        _f: Box<dyn FnOnce(&mut SlirpActor, &mut dyn Context<SlirpActor>) + Send>,
    ) -> actor_framework::TimerKey {
        unimplemented!("run_later not implemented for MockContext")
    }
    fn cancel_timer(&mut self, _key: actor_framework::TimerKey) {}
}

/// A backend-agnostic driver for testing Slirp packet forwarding.
#[allow(dead_code)]
pub struct SlirpTestDriver {
    actor: SlirpActor,
    ctx: MockContext,
    rx_out: mpsc::UnboundedReceiver<bytes::Bytes>,
}

#[allow(dead_code)]
impl SlirpTestDriver {
    async fn new_with_config(
        backend: SlirpBackend,
        config: slirp_actor::SlirpConfig,
        host_dns: Option<String>,
    ) -> Self {
        let (tx_out, rx_out) = mpsc::unbounded_channel::<bytes::Bytes>();
        let mut actor = SlirpActor::new_with_backend(config, None, host_dns, backend).await;
        let mut ctx = MockContext::default();

        let (_stream_tx, stream_rx) = mpsc::unbounded_channel::<bytes::Bytes>();
        let stream = Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(stream_rx));
        let sink: netsim_model::PacketSink =
            Box::pin(futures::sink::unfold(tx_out, |tx, bytes| async move {
                let _ = tx.send(bytes);
                Ok(tx)
            }));

        actor
            .handle_action(
                None,
                SlirpReq::Register { client_id: 0, stream, sink, notifier: None, isolated: false },
                &mut ctx,
            )
            .await
            .expect("Failed to register client");

        actor.on_start(&mut ctx).await;
        Self { actor, ctx, rx_out }
    }

    /// Creates a new SlirpTestDriver for the given backend.
    pub async fn new(backend: SlirpBackend, host_dns: Option<String>) -> Self {
        Self::new_with_config(backend, slirp_actor::SlirpConfig::default(), host_dns).await
    }

    /// Creates a new SlirpTestDriver with a TFTP root directory.
    pub async fn new_with_tftp_dir(backend: SlirpBackend, tftp_dir: std::path::PathBuf) -> Self {
        let mut config = slirp_actor::SlirpConfig::default();
        #[cfg(not(feature = "cuttlefish"))]
        {
            config.tftp_path = Some(tftp_dir);
        }
        #[cfg(feature = "cuttlefish")]
        {
            config.tftp_root = Some(tftp_dir);
        }
        Self::new_with_config(backend, config, None).await
    }

    /// Resolves the active backend's gateway MAC via ARP and populates C `libslirp`'s
    /// ARP cache for `guest_mac`.
    pub async fn resolve_gateway_mac(
        &mut self,
        guest_mac: [u8; 6],
        guest_ip: std::net::Ipv4Addr,
    ) -> [u8; 6] {
        let arp_req = netsim_packets::build_arp_frame(
            guest_mac,
            guest_ip,
            std::net::Ipv4Addr::new(10, 0, 2, 2),
        );
        self.send_packet(arp_req).await.expect("Failed to send ARP request");
        let is_arp_reply = |p: &bytes::Bytes| {
            netsim_packets::EthernetFrame::parse(p)
                .filter(|(eth, _)| eth.ethertype.get() == netsim_packets::ether_type::ARP)
                .and_then(|(_, payload)| netsim_packets::ArpPacket::parse(payload))
                .is_some_and(|arp| arp.opcode.get() == netsim_packets::arp_op::REPLY)
        };
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            self.recv_matching(is_arp_reply),
        )
        .await
        .expect("Timeout waiting for ARP reply")
        .expect("Stream closed");
        let (_, eth_payload) =
            netsim_packets::EthernetFrame::parse(&reply).expect("Failed to parse Ethernet frame");
        let arp =
            netsim_packets::ArpPacket::parse(eth_payload).expect("Failed to parse ARP packet");
        arp.sender_hardware_addr.bytes
    }

    /// Returns the active SLiRP backend after any feature fallback.
    pub fn backend(&self) -> SlirpBackend {
        self.actor.backend()
    }

    /// Sends a packet into the Slirp stack.
    pub async fn send_packet(&mut self, packet: bytes::Bytes) -> Result<(), ()> {
        let req = SlirpReq::SendPacket(packet);
        self.actor.handle_action(None, req, &mut self.ctx).await.map(|_| ()).map_err(|_e| ())
    }

    /// Sends a batch of packets into the Slirp stack in a single action
    /// execution.
    pub async fn send_packet_batch(&mut self, packets: Vec<bytes::Bytes>) -> Result<(), ()> {
        for packet in packets {
            self.send_packet(packet).await?;
        }
        Ok(())
    }

    /// Receives a packet generated by the Slirp stack.
    ///
    /// Outside the real runtime nothing polls the backend's downlink stream, so
    /// this drives that half of the actor loop itself: poll the registered
    /// typed streams and hand each frame to `on_typed_stream`, which routes it
    /// to the matching client.
    ///
    /// The routed frame does not land in `rx_out` synchronously -- the actor
    /// queues it for the per-client forwarding task spawned at registration --
    /// so both the sink and the streams must be awaited together.
    ///
    /// Loops until a frame arrives; callers bound it with `tokio::time::timeout`.
    pub async fn recv_packet(&mut self) -> Option<bytes::Bytes> {
        loop {
            // `on_typed_stream` needs `&mut self.ctx`, so the map cannot stay
            // borrowed across the dispatch; move it out and put it back.
            let mut streams = std::mem::take(&mut self.ctx.typed_streams);
            let (id, msg) = tokio::select! {
                biased;
                packet = self.rx_out.recv() => {
                    self.ctx.typed_streams = streams;
                    return packet;
                }
                Some(event) = streams.next(), if !streams.is_empty() => event,
            };
            self.ctx.typed_streams = streams;
            self.actor.on_typed_stream(id, msg, &mut self.ctx).await;
        }
    }

    /// Receives the next frame satisfying `predicate`, discarding others.
    ///
    /// Slirp emits its own control traffic -- an ARP request or NDP neighbor
    /// solicitation to resolve the guest, for instance -- ahead of the reply a
    /// test is waiting for, so tests that assert on a specific frame must skip
    /// past it rather than inspect whatever arrives first.
    ///
    /// Loops until a frame matches; callers bound it with `tokio::time::timeout`.
    pub async fn recv_matching(
        &mut self,
        predicate: impl Fn(&bytes::Bytes) -> bool,
    ) -> Option<bytes::Bytes> {
        loop {
            let packet = self.recv_packet().await?;
            if predicate(&packet) {
                return Some(packet);
            }
        }
    }

    /// Queries whether the underlying backend is initialized.
    pub async fn is_initialized(&mut self) -> bool {
        self.actor
            .handle_get(0, &mut self.ctx)
            .await
            .map(|opt| opt.map(|s| s.initialized).unwrap_or(false))
            .unwrap_or(false)
    }
}

pub use super::packet_helpers::{
    create_tcp_packet_with_options as create_tcp_packet, create_udp_packet,
};

/// The builders track a `payload_len` that starts at zero and is only set by
/// `payload()`, so a helper that reaches for `payload_mut()` instead emits a
/// frame whose IP and UDP headers both claim an empty payload. That shipped
/// here once and no test noticed, because a truncated length still routes.
#[test]
fn harness_emits_consistent_lengths() {
    const ETH: usize = 14;
    const IPV4: usize = 20;
    const UDP: usize = 8;

    let src = std::net::Ipv4Addr::new(10, 0, 2, 15);
    let dst = std::net::Ipv4Addr::new(10, 0, 2, 2);
    let payload = b"hello world";

    let frame = create_udp_packet(src, dst, 1111, 53, payload);
    assert_eq!(
        u16::from_be_bytes([frame[16], frame[17]]) as usize,
        frame.len() - ETH,
        "IPv4 total_length"
    );
    assert_eq!(
        u16::from_be_bytes([frame[ETH + IPV4 + 4], frame[ETH + IPV4 + 5]]) as usize,
        UDP + payload.len(),
        "UDP length"
    );
    assert_ne!(
        u16::from_be_bytes([frame[ETH + IPV4 + 6], frame[ETH + IPV4 + 7]]),
        0,
        "UDP checksum"
    );

    let frame =
        create_tcp_packet(src, dst, 1111, 80, 1, 0, false, true, false, false, 8192, &[], payload);
    assert_eq!(
        u16::from_be_bytes([frame[16], frame[17]]) as usize,
        frame.len() - ETH,
        "IPv4 total_length"
    );
    assert_ne!(
        u16::from_be_bytes([frame[ETH + IPV4 + 16], frame[ETH + IPV4 + 17]]),
        0,
        "TCP checksum"
    );
}
