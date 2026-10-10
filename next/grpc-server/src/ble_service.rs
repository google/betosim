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
            let (packet_tx, mut packet_rx) = mpsc::channel::<Bytes>(100);

            let app_sink: netsim_model::PacketSink =
                Box::pin(futures::sink::unfold(packet_tx, |tx, item: Bytes| async move {
                    tx.send(item).await.map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string())
                    })?;
                    Ok(tx)
                }));

            let position = req
                .position
                .as_ref()
                .map(|p| Position { x: p.x, y: p.y, z: p.z })
                .unwrap_or_default();

            let id = SCANNER_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
            let guid = format!("BleScanner-{}", id);

            let device_config = DeviceConfig {
                name: guid.clone(),
                visible: true,
                pose: Pose { position, orientation: Default::default() },
                builtin: true,
                device_info: None,
            };

            let chip = netsim_model::Chip {
                name: "BleScanner".to_string(),
                manufacturer: "Netsim".to_string(),
                product_name: "Scanner".to_string(),
                kind: netsim_model::ChipKind::BLUETOOTH,
                variant: Some(netsim_model::ChipVariant::Bluetooth(Box::new(
                    netsim_model::Bluetooth {
                        address: "".to_string(),
                        mode: BluetoothMode::Scanner(ScannerParams { active: req.active }),
                        bt_properties: Default::default(),
                        ..Default::default()
                    },
                ))),
                ..Default::default()
            };

            let add_chip = DeviceAddChip {
                device_guid: guid,
                packet_stream: None,
                packet_sink: Some(app_sink),
                device_config,
                chip,
            };

            if let Err(e) = device_client.add_chip(add_chip).await {
                error!("Failed to create scanner chip: {}", e);
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
                if let Err(e) = stream_sink.send((resp, WriteFlags::default())).await {
                    warn!("Scan stream disconnected or error: {}", e);
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
            let (packet_tx, mut packet_rx) = mpsc::channel::<Bytes>(100);

            let app_sink: netsim_model::PacketSink =
                Box::pin(futures::sink::unfold(packet_tx, |tx, item: Bytes| async move {
                    tx.send(item).await.map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::BrokenPipe, e.to_string())
                    })?;
                    Ok(tx)
                }));

            let position = req
                .position
                .as_ref()
                .map(|p| Position { x: p.x, y: p.y, z: p.z })
                .unwrap_or_default();

            let id = SCANNER_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
            let guid = format!("BleSniffer-{}", id);

            let device_config = DeviceConfig {
                name: guid.clone(),
                visible: true,
                pose: Pose { position, orientation: Default::default() },
                builtin: true,
                device_info: None,
            };

            let chip = netsim_model::Chip {
                name: "BleSniffer".to_string(),
                manufacturer: "Netsim".to_string(),
                product_name: "Sniffer".to_string(),
                kind: netsim_model::ChipKind::BLUETOOTH,
                variant: Some(netsim_model::ChipVariant::Bluetooth(Box::new(
                    netsim_model::Bluetooth {
                        address: "".to_string(),
                        mode: BluetoothMode::Sniffer(SnifferParams::default()),
                        bt_properties: Default::default(),
                        ..Default::default()
                    },
                ))),
                ..Default::default()
            };

            let add_chip = DeviceAddChip {
                device_guid: guid,
                packet_stream: None,
                packet_sink: Some(app_sink),
                device_config,
                chip,
            };

            if let Err(e) = device_client.add_chip(add_chip).await {
                error!("Failed to create sniffer chip: {}", e);
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
                if let Err(e) = stream_sink.send((resp, WriteFlags::default())).await {
                    warn!("Sniff stream disconnected or error: {}", e);
                    break;
                }
            }
            let _ = stream_sink.close().await;
        });
    }
}
