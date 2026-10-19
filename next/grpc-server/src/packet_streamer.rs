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

use bytes::Bytes;
use futures::{SinkExt, TryStreamExt, pin_mut, stream::StreamExt};
use netsim_model::ChipInfo;
use netsim_proto::{
    packet_streamer::{self, PacketRequest, PacketResponse},
    packet_streamer_grpc::PacketStreamer,
};
use packet_stream::{
    error::{PacketStreamError, Result},
    transport::traits::{PacketSink, PacketStream},
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, warn};

use crate::packet_stream_converter;

#[derive(Clone)]
pub struct PacketStreamerService {
    // Sender to the GrpcTransportListener (or equivalent) to pass new connections
    new_connection_tx: mpsc::Sender<(ChipInfo, String, PacketStream, PacketSink)>,
}

impl PacketStreamerService {
    pub fn new(
        new_connection_tx: mpsc::Sender<(ChipInfo, String, PacketStream, PacketSink)>,
    ) -> Self {
        Self { new_connection_tx }
    }
}

fn grpc_error_to_packet_error(e: grpcio::Error) -> PacketStreamError {
    PacketStreamError::Io(std::io::Error::other(e.to_string()))
}

async fn handle_grpc_initial_info(
    mut stream: ::grpcio::RequestStream<PacketRequest>,
    sink: ::grpcio::DuplexSink<PacketResponse>,
) -> Result<(ChipInfo, ::grpcio::RequestStream<PacketRequest>, ::grpcio::DuplexSink<PacketResponse>)>
{
    let initial_info_req = match stream.next().await {
        Some(Ok(req)) => req,
        Some(Err(error)) => {
            error!(%error, "gRPC stream error on initial info");
            let _ = sink
                .fail(grpcio::RpcStatus::with_message(
                    grpcio::RpcStatusCode::INVALID_ARGUMENT,
                    format!("Error receiving initial info: {error}"),
                ))
                .await;
            return Err(grpc_error_to_packet_error(error));
        }
        None => {
            error!("gRPC stream closed before initial info");
            let _ = sink
                .fail(grpcio::RpcStatus::with_message(
                    grpcio::RpcStatusCode::INVALID_ARGUMENT,
                    "Stream closed before initial info".to_string(),
                ))
                .await;
            return Err(PacketStreamError::ConnectionClosed);
        }
    };

    match initial_info_req.request_type {
        Some(packet_streamer::packet_request::Request_type::InitialInfo(info)) => {
            Ok((packet_stream_converter::proto_to_chip_info(info), stream, sink))
        }
        _ => {
            error!("First message was not InitialInfo");
            let _ = sink
                .fail(grpcio::RpcStatus::with_message(
                    grpcio::RpcStatusCode::INVALID_ARGUMENT,
                    "First message must be InitialInfo".to_string(),
                ))
                .await;
            Err(PacketStreamError::Protocol(packet_stream::error::ProtocolError::InvalidFormat(
                "First message must be InitialInfo".to_string(),
            )))
        }
    }
}

impl PacketStreamer for PacketStreamerService {
    fn stream_packets(
        &mut self,
        ctx: ::grpcio::RpcContext,
        stream: ::grpcio::RequestStream<PacketRequest>,
        sink: ::grpcio::DuplexSink<PacketResponse>,
    ) {
        let new_connection_tx = self.new_connection_tx.clone();
        let peer_addr = ctx.peer();

        ctx.spawn(async move {
            // Do not call Tokio I/O from within this future, it will panic!
            // https://github.com/tikv/grpc-rs/issues/338
            let (chip_info, stream, sink) = match handle_grpc_initial_info(stream, sink).await {
                Ok(info) => info,
                Err(_) => return, // Error already logged and sink failed
            };

            // Send the new connection's channels to the listener's accept loop
            let is_bt = chip_info
                .chip
                .as_ref()
                .is_some_and(|c| c.kind == netsim_model::ChipKind::BLUETOOTH);

            let grpc_stream =
                stream.map_err(grpc_error_to_packet_error).and_then(|packet_request| async {
                    packet_stream_converter::packet_request_to_bytes(packet_request)
                });
            let (packet_stream_tx, packet_stream_rx) = mpsc::channel(100);
            let packet_stream = Box::pin(ReceiverStream::new(packet_stream_rx));

            let packet_sink = Box::pin(
                sink.with_flat_map(move |bytes: Bytes| {
                    match packet_stream_converter::bytes_to_packet_response(bytes, is_bt) {
                        Ok(packet_response) => futures::stream::iter(vec![Ok((
                            packet_response,
                            grpcio::WriteFlags::default()
                                .buffer_hint(false)
                                .force_no_compress(true),
                        ))]),
                        Err(error) => {
                            warn!(%error, "Error converting bytes to packet response");
                            futures::stream::iter(vec![])
                        }
                    }
                })
                .sink_map_err(grpc_error_to_packet_error),
            );

            if new_connection_tx
                .send((chip_info, peer_addr, packet_stream, packet_sink))
                .await
                .is_err()
            {
                warn!("Failed to send new connection to listener. Dropping connection.");
            }

            // The stream must be driven within gRPC, or tokio won't get a wake.
            // https://github.com/tikv/grpc-rs/issues/338#issuecomment-512845420
            pin_mut!(grpc_stream);
            while let Some(packet_res) = grpc_stream.next().await {
                if packet_stream_tx.send(packet_res).await.is_err() {
                    break;
                }
            }
        });
    }
}

pub struct ChannelTransportListener {
    pub rx: mpsc::Receiver<(ChipInfo, String, PacketStream, PacketSink)>,
    pub local_addr: packet_stream::StreamAddress,
}

#[async_trait::async_trait]
impl packet_stream::transport::traits::TransportListener for ChannelTransportListener {
    async fn accept(
        &mut self,
    ) -> packet_stream::error::Result<(PacketStream, PacketSink, ChipInfo, String)> {
        match self.rx.recv().await {
            Some((chip_info, guid, app_stream, app_sink)) => {
                Ok((app_stream, app_sink, chip_info, guid))
            }
            None => Err(packet_stream::error::PacketStreamError::ConnectionClosed),
        }
    }
    fn local_addr(&self) -> packet_stream::error::Result<packet_stream::StreamAddress> {
        Ok(self.local_addr.clone())
    }
    async fn shutdown(&mut self) -> packet_stream::error::Result<()> {
        Ok(())
    }
}
