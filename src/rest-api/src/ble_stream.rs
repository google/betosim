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

//! Live BLE scan (`GET /v1/ble/scan`) and sniff (`GET /v1/ble/sniff`) over WebSocket.

use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use device_actor::DeviceClient;
use http_body_util::Full;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use netsim_model::{
    Bluetooth, BluetoothMode, Chip, ChipKind, ChipVariant, DeviceAddChip, DeviceConfig, PacketSink,
    Pose, Position, ScannerParams, SnifferParams,
};
use tokio::sync::mpsc;
use tracing::info;

use crate::{
    capture_stream::{Handshake, forward, switching_protocols},
    server::{client_error_status, error_response},
};

static STREAM_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BleStreamKind {
    Scan,
    Sniff,
}

fn parse_f32(name: &str, value: &str) -> Result<f32, String> {
    value.parse::<f32>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("Invalid {name}"))
}

fn parse_query(query: Option<&str>, kind: BleStreamKind) -> Result<(Position, bool), String> {
    let mut pos = Position::default();
    let mut active = false;
    for (key, value) in form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        match key.as_ref() {
            "x" => pos.x = parse_f32("x", &value)?,
            "y" => pos.y = parse_f32("y", &value)?,
            "z" => pos.z = parse_f32("z", &value)?,
            "active" if kind == BleStreamKind::Scan => {
                active =
                    value.parse::<bool>().map_err(|e| format!("Invalid active ({value}): {e}"))?;
            }
            other => return Err(format!("Unknown query parameter: {other}")),
        }
    }
    Ok((pos, active))
}

pub(super) async fn upgrade(
    device_client: &DeviceClient,
    mut req: Request<hyper::body::Incoming>,
    kind: BleStreamKind,
) -> Response<Full<Bytes>> {
    let (position, active) = match parse_query(req.uri().query(), kind) {
        Ok(parsed) => parsed,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, &msg),
    };
    let resp = match switching_protocols(req.headers()) {
        Handshake::Upgrade(resp) => resp,
        Handshake::Reject(resp) => return resp,
    };

    let (chip_name, product_name, mode) = match kind {
        BleStreamKind::Scan => {
            ("BleScanner", "Scanner", BluetoothMode::Scanner(ScannerParams { active }))
        }
        BleStreamKind::Sniff => {
            ("BleSniffer", "Sniffer", BluetoothMode::Sniffer(SnifferParams::default()))
        }
    };

    let (packet_tx, packet_rx) = mpsc::channel::<Bytes>(100);
    let app_sink: PacketSink =
        Box::pin(futures::sink::unfold(packet_tx, |tx, item: Bytes| async move {
            tx.send(item)
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))?;
            Ok(tx)
        }));

    let id = STREAM_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    let name = format!("{chip_name}-{id}");
    let add_chip = DeviceAddChip {
        device_guid: format!("{chip_name}-rest-{id}"),
        packet_stream: None,
        packet_sink: Some(app_sink),
        device_config: DeviceConfig {
            name: name.clone(),
            visible: true,
            pose: Pose { position, orientation: Default::default() },
            builtin: true,
            device_info: None,
        },
        chip: Chip {
            name: chip_name.to_string(),
            manufacturer: "Netsim".to_string(),
            product_name: product_name.to_string(),
            kind: ChipKind::BLUETOOTH,
            variant: Some(ChipVariant::Bluetooth(Box::new(Bluetooth {
                address: String::new(),
                mode,
                ..Default::default()
            }))),
            ..Default::default()
        },
    };

    let device_id = match device_client.add_chip(add_chip).await {
        Ok(device_id) => device_id,
        Err(err) => return error_response(client_error_status(&err), &err.to_string()),
    };

    let cleanup_client = device_client.clone();
    let on_upgrade = hyper::upgrade::on(&mut req);
    tokio::spawn(async move {
        if let Ok(upgraded) = on_upgrade.await {
            info!("BLE stream opened for {name} (device {device_id})");
            forward(TokioIo::new(upgraded), packet_rx).await;
            info!("BLE stream closed for {name} (device {device_id})");
        }
        let _ = cleanup_client.delete(device_id).await;
    });

    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_defaults_and_rejects_invalid() {
        assert_eq!(parse_query(None, BleStreamKind::Scan).unwrap(), (Position::default(), false));
        assert_eq!(
            parse_query(Some("x=1.5&y=-2&z=3.25&active=true"), BleStreamKind::Scan).unwrap(),
            (Position { x: 1.5, y: -2.0, z: 3.25 }, true)
        );
        for bad in ["x=abc", "y=NaN", "z=inf", "active=yes", "foo=1"] {
            assert!(parse_query(Some(bad), BleStreamKind::Scan).is_err(), "{bad}");
        }
        assert!(parse_query(Some("active=true"), BleStreamKind::Sniff).is_err());
    }
}
