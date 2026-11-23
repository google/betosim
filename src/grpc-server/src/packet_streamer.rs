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
use futures::{
    SinkExt,
    stream::{Stream, StreamExt},
};
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
    let kind = match &e {
        grpcio::Error::RemoteStopped
        | grpcio::Error::QueueShutdown
        | grpcio::Error::RpcFinished(None) => std::io::ErrorKind::ConnectionReset,
        grpcio::Error::RpcFinished(Some(s))
            if matches!(s.code(), grpcio::RpcStatusCode::OK | grpcio::RpcStatusCode::CANCELLED) =>
        {
            std::io::ErrorKind::ConnectionReset
        }
        grpcio::Error::RpcFailure(s) if s.code() == grpcio::RpcStatusCode::CANCELLED => {
            std::io::ErrorKind::ConnectionReset
        }
        _ => std::io::ErrorKind::Other,
    };
    PacketStreamError::Io(std::io::Error::new(kind, e.to_string()))
}

fn is_expected_disconnect(error: &grpcio::Error) -> bool {
    match error {
        grpcio::Error::RpcFinished(None) => true,
        grpcio::Error::RpcFinished(Some(status)) => {
            matches!(status.code(), grpcio::RpcStatusCode::OK | grpcio::RpcStatusCode::CANCELLED)
        }
        _ => false,
    }
}

async fn forward_grpc_requests(
    mut stream: impl Stream<Item = grpcio::Result<PacketRequest>> + Unpin,
    packet_stream_tx: mpsc::Sender<Result<Bytes>>,
) {
    while let Some(res) = stream.next().await {
        match res {
            Ok(packet_request) => {
                let packet_res = packet_stream_converter::packet_request_to_bytes(packet_request);
                if packet_stream_tx.send(packet_res).await.is_err() {
                    break;
                }
            }
            Err(error) if is_expected_disconnect(&error) => {
                tracing::debug!(%error, "gRPC packet stream closed by peer");
                break;
            }
            // grpcio re-arms msg_f before decoding, so stream framing stays intact.
            Err(error @ grpcio::Error::Codec(_)) => {
                if packet_stream_tx.send(Err(grpc_error_to_packet_error(error))).await.is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = packet_stream_tx.send(Err(grpc_error_to_packet_error(error))).await;
                break;
            }
        }
    }
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

    if let Some(packet_streamer::packet_request::Request_type::InitialInfo(info)) =
        initial_info_req.request_type
    {
        Ok((packet_stream_converter::proto_to_chip_info(info), stream, sink))
    } else {
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
            let Ok((chip_info, stream, sink)) = handle_grpc_initial_info(stream, sink).await else {
                return; // Error already logged and sink failed
            };

            // Send the new connection's channels to the listener's accept loop
            let is_bt = chip_info
                .chip
                .as_ref()
                .is_some_and(|c| c.kind == netsim_model::ChipKind::BLUETOOTH);

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
            forward_grpc_requests(stream, packet_stream_tx).await;
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

#[cfg(test)]
mod tests {
    use std::{
        io::{self, ErrorKind},
        sync::Arc,
        time::Duration,
    };

    use grpcio::{
        ChannelBuilder, Environment, Error, RpcStatus, RpcStatusCode, ServerBuilder,
        ServerCredentials, WriteFlags,
    };
    use netsim_proto::{
        common as proto_common,
        hci_packet::{HCIPacket, hcipacket::PacketType},
        packet_streamer_grpc::{PacketStreamerClient, create_packet_streamer},
        startup as proto_startup,
    };
    use protobuf::{EnumOrUnknown, MessageField};

    use super::*;

    fn into_io_error(err: Error) -> io::Error {
        match grpc_error_to_packet_error(err) {
            PacketStreamError::Io(io_err) => io_err,
            other => panic!("expected PacketStreamError::Io, got {other:?}"),
        }
    }

    #[test]
    fn test_grpc_error_remote_stopped_maps_to_connection_reset() {
        let io_err = into_io_error(Error::RemoteStopped);
        assert_eq!(io_err.kind(), ErrorKind::ConnectionReset);
        assert!(!io_err.to_string().is_empty());
    }

    #[test]
    fn test_grpc_error_queue_shutdown_maps_to_connection_reset() {
        let io_err = into_io_error(Error::QueueShutdown);
        assert_eq!(io_err.kind(), ErrorKind::ConnectionReset);
        assert!(!io_err.to_string().is_empty());
    }

    #[test]
    fn test_grpc_error_rpc_finished_none_maps_to_connection_reset() {
        let io_err = into_io_error(Error::RpcFinished(None));
        assert_eq!(io_err.kind(), ErrorKind::ConnectionReset);
        assert!(!io_err.to_string().is_empty());
    }

    #[test]
    fn test_grpc_error_rpc_finished_ok_maps_to_connection_reset() {
        let io_err = into_io_error(Error::RpcFinished(Some(RpcStatus::ok())));
        assert_eq!(io_err.kind(), ErrorKind::ConnectionReset);
        assert!(!io_err.to_string().is_empty());
    }

    #[test]
    fn test_grpc_error_rpc_finished_cancelled_maps_to_connection_reset() {
        let io_err =
            into_io_error(Error::RpcFinished(Some(RpcStatus::new(RpcStatusCode::CANCELLED))));
        assert_eq!(io_err.kind(), ErrorKind::ConnectionReset);
        assert!(!io_err.to_string().is_empty());
    }

    #[test]
    fn test_grpc_error_rpc_failure_cancelled_maps_to_connection_reset() {
        let io_err = into_io_error(Error::RpcFailure(RpcStatus::new(RpcStatusCode::CANCELLED)));
        assert_eq!(io_err.kind(), ErrorKind::ConnectionReset);
        assert!(!io_err.to_string().is_empty());
    }

    #[test]
    fn test_grpc_error_rpc_failure_internal_maps_to_other() {
        let io_err = into_io_error(Error::RpcFailure(RpcStatus::with_message(
            RpcStatusCode::INTERNAL,
            "internal failure".to_string(),
        )));
        assert_eq!(io_err.kind(), ErrorKind::Other);
        assert!(!io_err.to_string().is_empty());
    }

    #[test]
    fn test_grpc_error_rpc_finished_internal_maps_to_other() {
        let io_err = into_io_error(Error::RpcFinished(Some(RpcStatus::with_message(
            RpcStatusCode::INTERNAL,
            "stream internal".to_string(),
        ))));
        assert_eq!(io_err.kind(), ErrorKind::Other);
        assert!(!io_err.to_string().is_empty());
    }

    #[test]
    fn test_grpc_error_shutdown_failed_maps_to_other() {
        let io_err = into_io_error(Error::ShutdownFailed);
        assert_eq!(io_err.kind(), ErrorKind::Other);
        assert!(!io_err.to_string().is_empty());
    }

    #[test]
    fn test_grpc_error_invalid_metadata_maps_to_other() {
        let io_err = into_io_error(Error::InvalidMetadata("bad header".to_string()));
        assert_eq!(io_err.kind(), ErrorKind::Other);
        assert!(!io_err.to_string().is_empty());
    }

    #[test]
    fn test_grpc_error_codec_maps_to_other() {
        let io_err =
            into_io_error(Error::Codec(Box::new(io::Error::new(ErrorKind::InvalidData, "decode"))));
        assert_eq!(io_err.kind(), ErrorKind::Other);
        assert!(!io_err.to_string().is_empty());
    }

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    fn make_hci_packet_request(byte: u8) -> PacketRequest {
        let hci = HCIPacket {
            packet_type: PacketType::COMMAND.into(),
            packet: vec![byte],
            ..Default::default()
        };
        PacketRequest {
            request_type: Some(packet_streamer::packet_request::Request_type::HciPacket(hci)),
            ..Default::default()
        }
    }

    async fn collect_forwarded_requests(
        items: Vec<grpcio::Result<PacketRequest>>,
    ) -> Vec<Result<Bytes>> {
        let (tx, rx) = mpsc::channel(items.len().max(1));
        forward_grpc_requests(futures::stream::iter(items), tx).await;
        ReceiverStream::new(rx).collect().await
    }

    #[tokio::test]
    async fn test_stream_packets_abrupt_client_close_ends_stream_without_error() {
        let env = Arc::new(Environment::new(1));
        let (new_connection_tx, mut new_connection_rx) = mpsc::channel(1);
        let service = create_packet_streamer(PacketStreamerService::new(new_connection_tx));
        let mut server = ServerBuilder::new(env.clone())
            .channel_args(ChannelBuilder::new(env.clone()).reuse_port(false).build_args())
            .register_service(service)
            .build()
            .unwrap();
        let port = server.add_listening_port("localhost:0", ServerCredentials::insecure()).unwrap();
        server.start();

        let channel = ChannelBuilder::new(env).connect(&format!("localhost:{port}"));
        let client = PacketStreamerClient::new(channel);
        let (mut sender, receiver) = client.stream_packets().unwrap();

        let mut chip = proto_startup::Chip::new();
        chip.kind = EnumOrUnknown::new(proto_common::ChipKind::BLUETOOTH);
        let mut chip_info = proto_startup::ChipInfo::new();
        chip_info.name = "test-device".to_string();
        chip_info.chip = MessageField::some(chip);
        let initial_req = PacketRequest {
            request_type: Some(packet_streamer::packet_request::Request_type::InitialInfo(
                chip_info,
            )),
            ..Default::default()
        };
        sender.send((initial_req, WriteFlags::default())).await.unwrap();

        let (_chip_info, _peer_addr, mut packet_stream, _packet_sink) =
            tokio::time::timeout(TEST_TIMEOUT, new_connection_rx.recv()).await.unwrap().unwrap();

        drop(sender);
        drop(receiver);

        assert!(tokio::time::timeout(TEST_TIMEOUT, packet_stream.next()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_forward_grpc_requests_continues_after_codec_error() {
        let forwarded = collect_forwarded_requests(vec![
            Err(grpcio::Error::Codec(Box::new(std::io::Error::other("bad proto")))),
            Ok(make_hci_packet_request(0x01)),
        ])
        .await;

        assert_eq!(forwarded.len(), 2);
        assert!(
            matches!(&forwarded[0], Err(PacketStreamError::Io(e)) if e.to_string().contains("bad proto"))
        );
        assert_eq!(
            forwarded[1].as_ref().unwrap().as_ref(),
            [EnumOrUnknown::new(PacketType::COMMAND).value() as u8, 0x01]
        );
    }

    #[tokio::test]
    async fn test_forward_grpc_requests_stops_after_terminal_non_expected_error() {
        let forwarded = collect_forwarded_requests(vec![
            Err(grpcio::Error::RpcFinished(Some(grpcio::RpcStatus::new(
                grpcio::RpcStatusCode::INTERNAL,
            )))),
            Ok(make_hci_packet_request(0x01)),
        ])
        .await;

        assert_eq!(forwarded.len(), 1);
        assert!(
            matches!(&forwarded[0], Err(PacketStreamError::Io(e)) if e.to_string().contains("INTERNAL"))
        );
    }

    #[tokio::test]
    async fn test_forward_grpc_requests_drops_expected_disconnect_without_forwarding_error() {
        for error in [
            grpcio::Error::RpcFinished(None),
            grpcio::Error::RpcFinished(Some(grpcio::RpcStatus::ok())),
            grpcio::Error::RpcFinished(Some(grpcio::RpcStatus::new(
                grpcio::RpcStatusCode::CANCELLED,
            ))),
        ] {
            let received =
                collect_forwarded_requests(vec![Err(error), Ok(make_hci_packet_request(0x01))])
                    .await;

            assert!(received.is_empty());
        }
    }
}
