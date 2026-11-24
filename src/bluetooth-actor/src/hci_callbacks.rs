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

use std::{collections::VecDeque, error::Error as StdError, ffi::c_int, io, sync::Arc};

use bytes::Bytes;
use device_actor::DeviceClient;
use futures::SinkExt;
use netsim_model::ChipId;
use parking_lot::Mutex;
use rootcanal::{
    controller::{Callbacks as ControllerCallbacks, Id},
    types::Phy,
};
use tokio::sync::mpsc;
use tracing::{debug, error};

/// Callbacks for handling HCI and Link Layer packets from Rootcanal.
pub(crate) struct HciCallbacks {
    /// The ID of the chip associated with these callbacks.
    id: ChipId,
    /// Channel to send HCI packets to the sink task.
    hci_tx: Option<mpsc::Sender<Bytes>>,
    /// Channel to send LL packets (not currently used in this path, usually for
    /// sniffer/link bridging).
    ll_tx: Option<mpsc::Sender<Bytes>>,
    invalid_packets: Option<Arc<Mutex<VecDeque<netsim_model::InvalidPacket>>>>,
}

impl HciCallbacks {
    pub(crate) fn new(id: ChipId) -> Self {
        Self { id, hci_tx: None, ll_tx: None, invalid_packets: None }
    }

    pub(crate) fn with_hci(id: ChipId, hci_tx: mpsc::Sender<Bytes>) -> Self {
        Self { id, hci_tx: Some(hci_tx), ll_tx: None, invalid_packets: None }
    }

    pub(crate) fn with_ll(id: ChipId, ll_tx: mpsc::Sender<Bytes>) -> Self {
        Self { id, hci_tx: None, ll_tx: Some(ll_tx), invalid_packets: None }
    }

    pub(crate) fn with_invalid_packets(
        mut self,
        invalid_packets: Arc<Mutex<VecDeque<netsim_model::InvalidPacket>>>,
    ) -> Self {
        self.invalid_packets = Some(invalid_packets);
        self
    }
}

impl ControllerCallbacks for HciCallbacks {
    fn send_hci(&self, _source_id: Id, h4_packet: Bytes) {
        if let Some(hci_tx) = self.hci_tx.as_ref() {
            debug!("send_hci: Sending packet len={}", h4_packet.len());
            if let Err(error) = hci_tx.try_send(h4_packet) {
                error!(
                    %error,
                    code = crate::err::Code::HciPacketSendFailed as u32,
                    "Failed to send HCI packet, dropping."
                );
            }
        }
    }

    fn on_receive_ll(&self, _source_id: Id, packet: &[u8], phy: Phy, _tx_power: i32) {
        debug!("[{}] Received {:?} LL packet", self.id, phy);
        if let Some(ll_tx) = self.ll_tx.as_ref() {
            let packet = Bytes::copy_from_slice(packet);
            if let Err(error) = ll_tx.try_send(packet) {
                error!(
                    %error,
                    code = crate::err::Code::LlPacketSendFailed as u32,
                    "Failed to send LL packet, dropping."
                );
            }
        }
    }

    fn invalid_packet_received(&self, _source_id: Id, reason: c_int, message: &str, data: &[u8]) {
        let packet_hex = hex::encode(data);
        error!(
            code = crate::err::Code::InvalidPacketReceived as u32,
            "[chip-{}] Invalid packet received by device: reason={reason}, message={message}, packet={packet_hex}",
            self.id
        );
        if let Some(invalid_packets) = &self.invalid_packets {
            let mut packets = invalid_packets.lock();
            netsim_model::NetsimRadioStats::push_bounded_invalid_packet(
                &mut packets,
                netsim_model::InvalidPacket {
                    reason: netsim_model::InvalidPacketReason::from(reason),
                    description: message.to_string(),
                },
            );
        }
    }
}

fn leaf_io_error_kind(err: &io::Error) -> io::ErrorKind {
    let mut kind = err.kind();
    let mut curr: Option<&(dyn StdError + 'static)> = Some(err);
    while let Some(err_ref) = curr {
        if let Some(io_err) = err_ref.downcast_ref::<io::Error>() {
            if io_err.kind() != io::ErrorKind::Other {
                kind = io_err.kind();
            }
            curr = io_err
                .get_ref()
                .map(|inner| inner as &(dyn StdError + 'static))
                .or_else(|| io_err.source());
        } else {
            curr = err_ref.source();
        }
    }
    kind
}

fn should_record_sink_error(err: &io::Error, device_client: &DeviceClient) -> bool {
    !device_client.is_shutting_down()
        && !matches!(
            leaf_io_error_kind(err),
            io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::NotConnected
                | io::ErrorKind::UnexpectedEof
        )
}

/// Runs the task that forwards HCI packets from the callbacks to the packet
/// sink. This task runs in the background and ensures that packets generated by
/// `rootcanal` (via `send_hci`) are sent to the `PacketSink` associated with
/// the chip.
pub(crate) async fn sink_loop(
    mut sink: netsim_model::PacketSink,
    mut receiver: tokio::sync::mpsc::Receiver<bytes::Bytes>,
    id: ChipId,
    device_client: DeviceClient,
) -> ChipId {
    debug!("sink_loop: Started for chip {}", id);
    while let Some(packet) = receiver.recv().await {
        debug!("sink_loop: Received packet len={}", packet.len());
        if let Err(error) = sink.send(packet).await {
            if should_record_sink_error(&error, &device_client) {
                error!(
                    %error,
                    code = crate::err::Code::SinkTaskFailed as u32,
                    "Sink for chip {id} failed"
                );
            } else {
                debug!(%error, "Sink for chip {id} closed during disconnect or shutdown");
            }
            // Skip sink.close(): SinkMapErr panics if sink.close() returns a second error.
            return id;
        }
        debug!("sink_loop: Sent packet successfully");
    }
    let _ = sink.close().await;
    debug!("sink_loop: Sink task for chip {id} finished");
    id
}

#[cfg(test)]
mod tests {
    use std::{
        pin::Pin,
        sync::atomic::{AtomicBool, Ordering},
        task::{Context, Poll},
    };

    use bytes::Bytes;
    use common::util::netsim_logger::isolated_error_stats;
    use futures::Sink;
    use netsim_testing::mock_broken_sink;
    use tokio::sync::{mpsc, mpsc::error::TryRecvError};

    use super::*;

    struct CloseFlagSink {
        closed: Arc<AtomicBool>,
        sent: Arc<Mutex<Vec<Bytes>>>,
    }

    impl Sink<Bytes> for CloseFlagSink {
        type Error = std::io::Error;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
            self.sent.lock().push(item);
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            self.closed.store(true, Ordering::SeqCst);
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_send_hci_drops_when_channel_full() {
        let (table, _guard) = isolated_error_stats();
        // Create an extremely small channel with size 2.
        // This mimics the original issue where mpsc::channel(10) filled up rapidly
        // before tokio executors could awake `sink_loop` to drain the packets.
        let (hci_tx, mut hci_rx) = mpsc::channel(2);

        let callbacks = HciCallbacks::with_hci(ChipId::from(1), hci_tx);

        // Rootcanal produces packets synchronously and rapidly back-to-back:
        callbacks.send_hci(0, Bytes::from_static(b"req1")); // Success
        callbacks.send_hci(0, Bytes::from_static(b"req2")); // Success

        // The queue is now full. This third packet hits `.try_send()` limit and gets
        // dropped immediately, which reproduces the exact packet loss causing test
        // failure.
        callbacks.send_hci(0, Bytes::from_static(b"req3")); // Dropped!
        assert_eq!(table.snapshot(), vec![(crate::err::Code::HciPacketSendFailed as u32, 1)]);

        // Draining the queue reveals only the first two packets reached the sink
        let packet1 = hci_rx.recv().await.unwrap();
        assert_eq!(packet1, Bytes::from_static(b"req1"));

        let packet2 = hci_rx.recv().await.unwrap();
        assert_eq!(packet2, Bytes::from_static(b"req2"));

        // No third packet exists; the channel is empty.
        match hci_rx.try_recv() {
            Err(TryRecvError::Empty) => {}
            _ => panic!("Expected empty channel!"),
        }
    }

    #[test]
    fn test_invalid_packet_received() {
        let (table, _guard) = isolated_error_stats();
        let callbacks = HciCallbacks::new(ChipId::from(1));
        callbacks.invalid_packet_received(0, 1, "test error message", &[0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(table.snapshot(), vec![(crate::err::Code::InvalidPacketReceived as u32, 1)]);
    }

    #[test]
    fn test_invalid_packet_received_records_and_caps() {
        let buffer = Arc::new(Mutex::new(VecDeque::new()));
        let callbacks = HciCallbacks::new(ChipId::from(1)).with_invalid_packets(buffer.clone());

        for i in 0..(netsim_model::NetsimRadioStats::MAX_INVALID_PACKETS + 5) {
            callbacks.invalid_packet_received(0, 1, &format!("error {i}"), &[0xde, 0xad]);
        }

        let packets = buffer.lock();
        assert_eq!(packets.len(), netsim_model::NetsimRadioStats::MAX_INVALID_PACKETS);
        assert_eq!(packets[0].reason, netsim_model::InvalidPacketReason::ParseError);
        assert_eq!(packets[0].description, "error 5");
        assert_eq!(
            packets[netsim_model::NetsimRadioStats::MAX_INVALID_PACKETS - 1].description,
            format!("error {}", netsim_model::NetsimRadioStats::MAX_INVALID_PACKETS + 4)
        );
    }

    struct FailingSink {
        kind: io::ErrorKind,
    }

    impl Sink<Bytes> for FailingSink {
        type Error = io::Error;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, _item: Bytes) -> Result<(), Self::Error> {
            Err(io::Error::new(self.kind, "sink write failed"))
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    fn failing_packet_sink(kind: io::ErrorKind) -> netsim_model::PacketSink {
        Box::pin(FailingSink { kind }.sink_map_err(io::Error::other))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_sink_loop_broken_pipe_map_err_returns_without_panic() {
        netsim_testing::setup(None);
        let (table, _guard) = isolated_error_stats();
        let (_device_actor, device_client) = device_actor::new();
        let (tx, rx) = mpsc::channel(4);
        let sink = mock_broken_sink();

        tx.send(Bytes::from_static(b"packet")).await.expect("send packet");

        let returned_id = sink_loop(sink, rx, ChipId::from(1), device_client).await;
        assert_eq!(returned_id, ChipId::from(1));
        assert_eq!(table.snapshot().as_slice(), &[]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_sink_loop_expected_disconnect_kinds_record_no_error_stat() {
        netsim_testing::setup(None);
        for kind in [
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::ConnectionAborted,
            io::ErrorKind::NotConnected,
            io::ErrorKind::UnexpectedEof,
        ] {
            let (table, _guard) = isolated_error_stats();
            let (_device_actor, device_client) = device_actor::new();
            let (tx, rx) = mpsc::channel(4);
            tx.send(Bytes::from_static(b"packet")).await.expect("send packet");

            let returned_id =
                sink_loop(failing_packet_sink(kind), rx, ChipId::from(1), device_client).await;
            assert_eq!(returned_id, ChipId::from(1));
            assert_eq!(table.snapshot().as_slice(), &[], "unexpected stat for {kind:?}");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_sink_loop_unexpected_io_error_records_sink_task_failed() {
        netsim_testing::setup(None);
        let (table, _guard) = isolated_error_stats();
        let (_device_actor, device_client) = device_actor::new();
        let (tx, rx) = mpsc::channel(4);
        tx.send(Bytes::from_static(b"packet")).await.expect("send packet");

        let returned_id = sink_loop(
            failing_packet_sink(io::ErrorKind::PermissionDenied),
            rx,
            ChipId::from(1),
            device_client,
        )
        .await;
        assert_eq!(returned_id, ChipId::from(1));
        assert_eq!(table.snapshot().as_slice(), &[(crate::err::Code::SinkTaskFailed as u32, 1)]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_sink_loop_shutdown_suppresses_sink_task_failed() {
        netsim_testing::setup(None);
        let (table, _guard) = isolated_error_stats();
        let (device_actor, device_client) = device_actor::new();
        drop(device_actor);
        let _ = device_client.shutdown().await;
        assert!(device_client.is_shutting_down());
        let (tx, rx) = mpsc::channel(4);
        tx.send(Bytes::from_static(b"packet")).await.expect("send packet");

        let returned_id = sink_loop(
            failing_packet_sink(io::ErrorKind::PermissionDenied),
            rx,
            ChipId::from(1),
            device_client,
        )
        .await;
        assert_eq!(returned_id, ChipId::from(1));
        assert_eq!(table.snapshot().as_slice(), &[]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_sink_loop_clean_eof_closes_sink() {
        netsim_testing::setup(None);
        let (table, _guard) = isolated_error_stats();
        let (_device_actor, device_client) = device_actor::new();
        let closed = Arc::new(AtomicBool::new(false));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let sink: netsim_model::PacketSink = Box::pin(
            CloseFlagSink { closed: Arc::clone(&closed), sent: Arc::clone(&sent) }
                .sink_map_err(std::io::Error::other),
        );
        let (tx, rx) = mpsc::channel(4);

        tx.send(Bytes::from_static(b"packet")).await.expect("send packet");
        drop(tx);

        let returned_id = sink_loop(sink, rx, ChipId::from(1), device_client).await;
        assert_eq!(returned_id, ChipId::from(1));
        assert_eq!(sent.lock().as_slice(), &[Bytes::from_static(b"packet")]);
        assert!(closed.load(Ordering::SeqCst));
        assert_eq!(table.snapshot().as_slice(), &[]);
    }

    #[test]
    fn test_send_ll_full_records_error_stat() {
        let (table, _guard) = isolated_error_stats();

        let (ll_tx, _ll_rx) = mpsc::channel(1);
        let callbacks = HciCallbacks::with_ll(ChipId::from(1), ll_tx);

        callbacks.on_receive_ll(0, &[0x01], Phy::LowEnergy, 0);
        callbacks.on_receive_ll(0, &[0x02], Phy::LowEnergy, 0);

        assert_eq!(table.snapshot(), vec![(crate::err::Code::LlPacketSendFailed as u32, 1)]);
    }
}
