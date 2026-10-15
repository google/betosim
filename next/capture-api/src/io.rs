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

//! Capture I/O utilities for wrapping streams and sinks with capture logic.
//!
//! This module provides wrappers for `PacketStream` and `PacketSink` that
//! automatically capture packets and invoke a callback for processing (e.g.,
//! writing to a PCAP file).
//!
//! The wrappers use an `AtomicBool` flag to dynamically enable/disable
//! capturing without needing to reconstruct the stream pipeline. This is
//! crucial for performance, as we only want to incur the cost of capturing when
//! it is actually enabled.

use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Sink, Stream};
use netsim_model::{PacketSink, PacketStream};

/// A wrapper around `PacketStream` that automatically captures received
/// packets.
///
/// When packets are read from the inner stream, they are passed to the capture
/// callback before being returned to the caller, if capturing is enabled.
///
/// This is used for packets received by the device (e.g., from the network).
pub struct CapturedStream {
    inner: PacketStream,
    sender: Option<
        tokio::sync::mpsc::UnboundedSender<(std::time::SystemTime, crate::Direction, Bytes)>,
    >,
    enabled: Arc<AtomicBool>,
}

impl CapturedStream {
    /// Creates a new `CapturedStream`.
    ///
    /// # Arguments
    /// * `inner` - The underlying packet stream to wrap.
    /// * `sender` - Optional concrete sender for the packets. If None or
    ///   disconnected, packets are not captured.
    /// * `enabled` - Thread-safe flag to enable/disable capturing.
    pub fn new(
        inner: PacketStream,
        sender: Option<
            tokio::sync::mpsc::UnboundedSender<(std::time::SystemTime, crate::Direction, Bytes)>,
        >,
        enabled: Arc<AtomicBool>,
    ) -> Self {
        Self { inner, sender, enabled }
    }
}

impl Stream for CapturedStream {
    type Item = Bytes;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        use futures::StreamExt;
        match self.inner.poll_next_unpin(cx) {
            Poll::Ready(Some(bytes)) => {
                if self.enabled.load(Ordering::SeqCst)
                    && let Some(sender) = &self.sender
                {
                    let _ = sender.send((
                        std::time::SystemTime::now(),
                        crate::Direction::Received,
                        bytes.clone(),
                    ));
                }
                Poll::Ready(Some(bytes))
            }
            other => other,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

/// A wrapper around `PacketSink` that automatically captures sent packets.
///
/// When packets are sent to the inner sink, they are passed to the capture
/// callback before being forwarded to the inner sink, if capturing is enabled.
///
/// This is used for packets sent by the device (e.g., to the network).
pub struct CapturedSink {
    inner: PacketSink,
    sender: Option<
        tokio::sync::mpsc::UnboundedSender<(std::time::SystemTime, crate::Direction, Bytes)>,
    >,
    enabled: Arc<AtomicBool>,
}

impl CapturedSink {
    pub fn new(
        inner: PacketSink,
        sender: Option<
            tokio::sync::mpsc::UnboundedSender<(std::time::SystemTime, crate::Direction, Bytes)>,
        >,
        enabled: Arc<AtomicBool>,
    ) -> Self {
        Self { inner, sender, enabled }
    }
}

impl Sink<Bytes> for CapturedSink {
    type Error = std::io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        use futures::SinkExt;
        self.inner.poll_ready_unpin(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        use futures::SinkExt;
        if self.enabled.load(Ordering::SeqCst)
            && let Some(sender) = &self.sender
        {
            let _ =
                sender.send((std::time::SystemTime::now(), crate::Direction::Sent, item.clone()));
        }
        self.inner.start_send_unpin(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        use futures::SinkExt;
        self.inner.poll_flush_unpin(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        use futures::SinkExt;
        self.inner.poll_close_unpin(cx)
    }
}
