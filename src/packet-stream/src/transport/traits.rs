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
//=============================================================================
// src/transport/traits.rs - Transport traits
//=============================================================================
use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use futures::{Sink, Stream};

use crate::{
    error::{PacketStreamError, Result},
    types::{ChipInfo, StreamAddress},
};

pub type PacketStream = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send + Sync>>;
pub type PacketSink = Pin<Box<dyn Sink<Bytes, Error = PacketStreamError> + Send + Sync>>;

#[async_trait]
pub trait TransportListener: Send {
    async fn accept(&mut self) -> Result<(PacketStream, PacketSink, ChipInfo, String)>;
    fn local_addr(&self) -> Result<StreamAddress>;
    async fn shutdown(&mut self) -> Result<()>;
}

/// Converts an asynchronous byte stream into framed packet stream and sink.
pub(crate) fn into_packet_stream_sink<S>(stream: S) -> (PacketStream, PacketSink)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + 'static,
{
    use futures::{SinkExt, StreamExt};
    use tokio_util::codec::{Framed, LengthDelimitedCodec};

    let framed = Framed::new(stream, LengthDelimitedCodec::new());
    let (sink, stream) = framed.split();
    let stream = stream.map(|item| item.map(|b| b.freeze()).map_err(PacketStreamError::Io));
    let sink = sink.sink_map_err(PacketStreamError::Io);
    (Box::pin(stream), Box::pin(sink))
}
