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

use std::pin::Pin;

use bytes::Bytes;
use futures::{
    Future,
    sink::Sink,
    stream::Stream,
    task::{Context, Poll},
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// Default buffer capacity for captured packets in [`mock_sink`].
const DEFAULT_SINK_BUFFER_CAPACITY: usize = 100;

/// Default buffer capacity for injected packets in [`mock_stream`].
const DEFAULT_STREAM_BUFFER_CAPACITY: usize = 10;

/// A mock [`Sink`] that captures packets into an `mpsc` channel.
///
/// # Note
/// This sink uses `try_send` which may drop packets if the channel is full.
/// It is intended for testing scenarios where backpressure is not a concern.
#[derive(Debug)]
struct MockSink {
    tx: mpsc::Sender<Vec<u8>>,
}

impl Sink<Bytes> for MockSink {
    type Error = std::io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut pinned = std::pin::pin!(self.get_mut().tx.reserve());
        pinned.as_mut().poll(cx).map(|result| {
            result.map(|_| ()).map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))
        })
    }

    fn start_send(self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        self.tx
            .try_send(item.to_vec())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

/// Creates a mock packet sink and a receiver to check the captured packets.
pub fn mock_sink()
-> (Pin<Box<dyn Sink<Bytes, Error = std::io::Error> + Send + Sync>>, mpsc::Receiver<Vec<u8>>) {
    let (packet_tx, packet_rx) = mpsc::channel(DEFAULT_SINK_BUFFER_CAPACITY);
    let sink = Box::pin(MockSink { tx: packet_tx });
    (sink, packet_rx)
}

/// Creates a mock packet stream and a sender to inject packets into it.
pub fn mock_stream() -> (Box<dyn Stream<Item = Bytes> + Send + Sync + Unpin>, mpsc::Sender<Bytes>) {
    let (packet_tx, packet_rx) = mpsc::channel(DEFAULT_STREAM_BUFFER_CAPACITY);
    let stream = Box::new(ReceiverStream::new(packet_rx));
    (stream, packet_tx)
}

#[cfg(test)]
mod tests {
    use futures::{SinkExt, StreamExt};

    use super::*;

    #[tokio::test]
    async fn test_mock_sink_captures_packets() {
        let (mut sink, mut rx) = mock_sink();

        let packet1 = Bytes::from_static(b"hello");
        let packet2 = Bytes::from_static(b"world");

        sink.send(packet1).await.expect("Failed to send packet1");
        sink.send(packet2).await.expect("Failed to send packet2");

        assert_eq!(rx.recv().await.unwrap(), b"hello".to_vec());
        assert_eq!(rx.recv().await.unwrap(), b"world".to_vec());
    }

    #[tokio::test]
    async fn test_mock_stream_delivers_injected_packets() {
        let (mut stream, tx) = mock_stream();

        let packet1 = Bytes::from_static(b"foo");
        let packet2 = Bytes::from_static(b"bar");

        tx.send(packet1).await.expect("Failed to inject packet1");
        tx.send(packet2).await.expect("Failed to inject packet2");

        assert_eq!(stream.next().await.unwrap(), Bytes::from_static(b"foo"));
        assert_eq!(stream.next().await.unwrap(), Bytes::from_static(b"bar"));
    }
}
