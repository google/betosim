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

use std::{io, net::SocketAddr};

use bytes::Bytes;
use device_actor::DeviceClient;
use device_api::{DeviceAddChip, DeviceConfig};
use futures::StreamExt;
use netsim_model::{Chip, ChipKind, ChipVariant, PacketSink, PacketStream};
use packet_stream::transport::H4Codec;
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::codec::FramedRead;
use tracing::{info, warn};

const CHANNEL_BUFFER_CAPACITY: usize = 100;
const UNKNOWN_PORT: u16 = 0;

/// Start the async TCP transport for HCI connections
///
/// Loops, passing each accepted connection to `handle_hci_client`.
pub async fn run(listener: TcpListener, device_client: DeviceClient) {
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(UNKNOWN_PORT);
    info!("Hci socket server is listening on: {port}");

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                info!("Hci client address: {addr}");
                let dc_clone = device_client.clone();
                tokio::spawn(async move {
                    handle_hci_client(stream, addr, dc_clone).await;
                });
            }
            Err(error) => {
                warn!(%error, "Error accepting HCI connection");
            }
        }
    }
}

/// Helper for constructing HCI Bluetooth chip parameters and device requests.
struct HciChipBuilder;

impl HciChipBuilder {
    fn build_chip(name: &str) -> Chip {
        Chip {
            name: name.to_string(),
            kind: ChipKind::BLUETOOTH,
            variant: ChipVariant::try_from(ChipKind::BLUETOOTH).ok(),
            ..Default::default()
        }
    }

    fn build_device_add_chip_request(
        name: String,
        packet_stream: PacketStream,
        packet_sink: PacketSink,
    ) -> DeviceAddChip {
        let chip = Self::build_chip(&name);
        DeviceAddChip {
            device_guid: name.clone(),
            packet_stream: Some(packet_stream),
            packet_sink: Some(packet_sink),
            device_config: DeviceConfig { name, visible: true, ..Default::default() },
            chip,
        }
    }
}

async fn handle_hci_client(stream: TcpStream, addr: SocketAddr, device_client: DeviceClient) {
    let (socket_rx, mut socket_tx) = stream.into_split();

    // 1. Setup the AsyncStream bridging to `packet_stream::PacketStream`
    let (stream_tx, stream_rx) = mpsc::channel::<Result<Bytes, io::Error>>(CHANNEL_BUFFER_CAPACITY);
    // 2. Setup the AsyncSink bridging from `packet_stream::PacketSink`
    let (sink_tx, mut sink_rx) = mpsc::channel::<Bytes>(CHANNEL_BUFFER_CAPACITY);

    // Read Task: Read H4 packets from the socket and send to `stream_tx`
    let read_addr = addr;
    tokio::spawn(async move {
        let mut framed = FramedRead::new(socket_rx, H4Codec);
        while let Some(packet_result) = framed.next().await {
            match packet_result {
                Ok(packet) => {
                    if stream_tx.send(Ok(packet)).await.is_err() {
                        break;
                    }
                }
                Err(error) => {
                    warn!(%error, "Error reading from HCI socket {read_addr}");
                    break;
                }
            }
        }
        info!("End socket reader connection with {read_addr}.");
    });

    // Write Task: Receive payloads from `sink_rx` and write them to the socket
    tokio::spawn(async move {
        while let Some(bytes) = sink_rx.recv().await {
            if socket_tx.write_all(&bytes).await.is_err() {
                break;
            }
        }
    });

    // 3. Create the Chip parameters and device request
    let name = format!("socket-{addr}");
    let packet_stream: PacketStream =
        Box::new(ReceiverStream::new(stream_rx).filter_map(|res| std::future::ready(res.ok())));
    let packet_sink: PacketSink = Box::pin(futures::sink::unfold(
        sink_tx,
        |tx: mpsc::Sender<Bytes>, item: Bytes| async move {
            tx.send(item).await.map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;
            Ok(tx)
        },
    ));

    let request = HciChipBuilder::build_device_add_chip_request(name, packet_stream, packet_sink);

    // 4. Send request to add chip
    if let Err(error) = device_client.add_chip(request).await {
        warn!(%error, "Failed to add virtual HCI chip for {addr}");
    }
}

#[cfg(test)]
mod tests {
    use futures::SinkExt;

    use super::*;

    #[test]
    fn test_build_chip_defaults() {
        let chip = HciChipBuilder::build_chip("socket-127.0.0.1:1234");
        assert_eq!(chip.name, "socket-127.0.0.1:1234");
        assert!(chip.manufacturer.is_empty());
        assert!(chip.product_name.is_empty());
        assert_eq!(chip.kind, ChipKind::BLUETOOTH);
        assert!(matches!(chip.variant, Some(ChipVariant::Bluetooth(_))));
    }

    #[test]
    fn test_build_device_add_chip_request() {
        let stream: PacketStream = Box::new(futures::stream::empty());
        let sink: PacketSink = Box::pin(futures::sink::drain().sink_map_err(|e| match e {}));
        let request = HciChipBuilder::build_device_add_chip_request(
            "socket-127.0.0.1:1234".to_string(),
            stream,
            sink,
        );
        assert_eq!(request.device_guid, "socket-127.0.0.1:1234");
        assert_eq!(request.device_config.name, "socket-127.0.0.1:1234");
        assert!(request.device_config.visible);
        assert!(!request.device_config.builtin);
    }
}
