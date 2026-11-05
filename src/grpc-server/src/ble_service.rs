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

use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use device_actor::DeviceClient;
use futures::SinkExt;
use grpcio::{RpcContext, ServerStreamingSink, WriteFlags};
use netsim_model::{
    BluetoothMode, DeviceAddChip, DeviceConfig, Pose, Position, ScannerParams, SnifferParams,
};
use netsim_proto::{
    ble_service::{ScanRequest, ScanResponse, SniffRequest, SniffResponse},
    ble_service_grpc::BleService,
};
use tokio::sync::mpsc;
use tracing::{error, warn};

static SCANNER_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn build_ble_chip(
    name: &'static str,
    product_name: &'static str,
    mode: BluetoothMode,
    proto_position: Option<&netsim_proto::model::Position>,
) -> (DeviceAddChip, mpsc::Receiver<Bytes>) {
    let (packet_tx, packet_rx) = mpsc::channel::<Bytes>(100);

    let app_sink: netsim_model::PacketSink =
        Box::pin(futures::sink::unfold(packet_tx, |tx, item: Bytes| async move {
            tx.send(item)
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string()))?;
            Ok(tx)
        }));

    let position = proto_position.map(|p| Position { x: p.x, y: p.y, z: p.z }).unwrap_or_default();

    let id = SCANNER_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    let guid = format!("{name}-{id}");

    let device_config = DeviceConfig {
        name: guid.clone(),
        visible: true,
        pose: Pose { position, orientation: Default::default() },
        builtin: true,
        device_info: None,
    };

    let chip = netsim_model::Chip {
        name: name.to_string(),
        manufacturer: "Netsim".to_string(),
        product_name: product_name.to_string(),
        kind: netsim_model::ChipKind::BLUETOOTH,
        variant: Some(netsim_model::ChipVariant::Bluetooth(Box::new(netsim_model::Bluetooth {
            address: "".to_string(),
            mode,
            bt_properties: Default::default(),
            ..Default::default()
        }))),
        ..Default::default()
    };

    let add_chip = DeviceAddChip {
        device_guid: guid,
        packet_stream: None,
        packet_sink: Some(app_sink),
        device_config,
        chip,
    };

    (add_chip, packet_rx)
}

#[derive(Clone)]
pub(crate) struct BleServiceImpl {
    device_client: DeviceClient,
}

impl BleServiceImpl {
    pub(crate) fn new(device_client: DeviceClient) -> Self {
        Self { device_client }
    }
}

impl BleService for BleServiceImpl {
    fn scan(
        &mut self,
        ctx: RpcContext<'_>,
        req: ScanRequest,
        mut stream_sink: ServerStreamingSink<ScanResponse>,
    ) {
        let device_client = self.device_client.clone();

        ctx.spawn(async move {
            let (add_chip, mut packet_rx) = build_ble_chip(
                "BleScanner",
                "Scanner",
                BluetoothMode::Scanner(ScannerParams { active: req.active }),
                req.position.as_ref(),
            );

            if let Err(error) = device_client.add_chip(add_chip).await {
                error!(%error, "Failed to create scanner chip");
                return;
            }

            // Forward packets from mpsc rx -> grpc stream.
            //
            // Note: When the gRPC stream disconnects, the receiver array loop stops
            // and `packet_rx` is dropped. The corresponding `packet_tx` is held by
            // the `packet_sink` inside the `BluetoothActor`. When the actor tries
            // to write to it and fails (broken pipe), the sink task naturally exits,
            // which automatically triggers `BluetoothActor::on_task_closed()`. This
            // lifecycle hook safely removes the corresponding sniffer/scanner
            // chip, and the overarching Device Actor auto-deletes the parent
            // device once empty.
            while let Some(packet) = packet_rx.recv().await {
                let mut resp = ScanResponse::new();
                resp.packet = packet.to_vec();
                if let Err(error) = stream_sink.send((resp, WriteFlags::default())).await {
                    warn!(%error, "Scan stream disconnected or error");
                    break;
                }
            }
            let _ = stream_sink.close().await;
        });
    }

    fn sniff(
        &mut self,
        ctx: RpcContext<'_>,
        req: SniffRequest,
        mut stream_sink: ServerStreamingSink<SniffResponse>,
    ) {
        let device_client = self.device_client.clone();

        ctx.spawn(async move {
            let (add_chip, mut packet_rx) = build_ble_chip(
                "BleSniffer",
                "Sniffer",
                BluetoothMode::Sniffer(SnifferParams::default()),
                req.position.as_ref(),
            );

            if let Err(error) = device_client.add_chip(add_chip).await {
                error!(%error, "Failed to create sniffer chip");
                return;
            }

            // Forward packets from mpsc rx -> grpc stream
            while let Some(packet) = packet_rx.recv().await {
                let mut resp = SniffResponse::new();
                resp.packet = packet.to_vec();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                resp.timestamp = now.as_micros() as i64;
                if let Err(error) = stream_sink.send((resp, WriteFlags::default())).await {
                    warn!(%error, "Sniff stream disconnected or error");
                    break;
                }
            }
            let _ = stream_sink.close().await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_build_ble_chip_scanner_with_position() {
        let proto_pos =
            netsim_proto::model::Position { x: 1.5, y: -2.0, z: 3.25, ..Default::default() };
        let (add_chip, mut packet_rx) = build_ble_chip(
            "BleScanner",
            "Scanner",
            BluetoothMode::Scanner(ScannerParams { active: true }),
            Some(&proto_pos),
        );

        assert!(add_chip.device_guid.starts_with("BleScanner-"));
        assert_eq!(add_chip.device_config.name, add_chip.device_guid);
        assert_eq!(add_chip.device_config.pose.position, Position { x: 1.5, y: -2.0, z: 3.25 });
        assert_eq!(add_chip.chip.name, "BleScanner");
        assert_eq!(add_chip.chip.product_name, "Scanner");

        let is_scanner_mode = match add_chip.chip.variant {
            Some(netsim_model::ChipVariant::Bluetooth(ref bt)) => {
                matches!(bt.mode, BluetoothMode::Scanner(ScannerParams { active: true }))
            }
            _ => false,
        };
        assert!(is_scanner_mode);

        let mut sink = add_chip.packet_sink.expect("packet_sink should be Some");
        sink.send(Bytes::from_static(b"test-packet")).await.expect("send should succeed");
        let packet = packet_rx.recv().await.expect("receive should succeed");
        assert_eq!(packet, Bytes::from_static(b"test-packet"));
    }

    #[tokio::test]
    async fn test_build_ble_chip_sniffer_default_position_and_closed_rx() {
        let (add_chip, packet_rx) = build_ble_chip(
            "BleSniffer",
            "Sniffer",
            BluetoothMode::Sniffer(SnifferParams::default()),
            None,
        );

        assert!(add_chip.device_guid.starts_with("BleSniffer-"));
        assert_eq!(add_chip.device_config.pose.position, Position::default());
        assert_eq!(add_chip.chip.name, "BleSniffer");
        assert_eq!(add_chip.chip.product_name, "Sniffer");

        let is_sniffer_mode = match add_chip.chip.variant {
            Some(netsim_model::ChipVariant::Bluetooth(ref bt)) => {
                matches!(bt.mode, BluetoothMode::Sniffer(_))
            }
            _ => false,
        };
        assert!(is_sniffer_mode);

        drop(packet_rx);
        let mut sink = add_chip.packet_sink.expect("packet_sink should be Some");
        let err = sink
            .send(Bytes::from_static(b"after-drop"))
            .await
            .expect_err("send on closed rx should fail");
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    }
}
