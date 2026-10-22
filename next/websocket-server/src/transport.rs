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

use std::{
    collections::HashMap,
    future::ready,
    io,
    net::{SocketAddr, TcpStream},
    sync::Arc,
};

use bytes::Bytes;
use device_actor::DeviceClient;
use device_api::DeviceId;
use futures::{StreamExt, sink::unfold};
use netsim_model::{
    BluetoothUpdate, BtControllerProperties, Chip, ChipError, ChipId, ChipKind, ChipUpdate,
    ChipVariant, ChipVariantUpdate, DeviceInfo, DeviceUpdate, PacketSink, PacketStream, Quirks,
};
use tokio::{sync::mpsc, task::JoinSet};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, info, warn};
use tungstenite::{Message, WebSocket};

use crate::error::ServerError;

/// Configures and adds a virtual radio chip to the simulation.
pub(crate) async fn setup_virtual_chip(
    device_client: &DeviceClient,
    chip_kind: ChipKind,
    mut query_params: HashMap<String, String>,
    addr: SocketAddr,
    stream_rx: mpsc::Receiver<Result<Bytes, io::Error>>,
    sink_tx: mpsc::Sender<Bytes>,
) -> Result<(DeviceId, ChipId), ServerError> {
    let device_guid = query_params
        .remove("device_guid")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| addr.port().to_string());
    let device_name = query_params
        .remove("name")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("websocket-device-{addr}"));
    let device_info = extract_device_info(&device_name, &mut query_params);

    let chip_id = query_params.remove("chip_id").filter(|s| !s.is_empty());
    let chip_name = chip_id.clone().unwrap_or_else(|| format!("websocket-{addr}"));
    let manufacturer = query_params
        .remove("manufacturer")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Google".to_string());
    let product_name = query_params
        .remove("product_name")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Google".to_string());
    let address = query_params
        .remove("address")
        .filter(|s| !s.is_empty())
        .or_else(|| chip_id.filter(|id| id.len() == 17))
        .unwrap_or_default();

    let variant = build_chip_variant(chip_kind, address, device_info.as_ref())
        .map_err(netsim_model::ClientError::from)?;

    let chip = Chip {
        name: chip_name,
        manufacturer,
        product_name,
        kind: chip_kind,
        variant: Some(variant),
        ..Default::default()
    };

    let (packet_stream, packet_sink) = packet_stream_and_sink(stream_rx, sink_tx);
    let request = device_api::DeviceAddChip {
        device_guid,
        packet_stream: Some(packet_stream),
        packet_sink: Some(packet_sink),
        device_config: device_api::DeviceConfig {
            name: device_name,
            visible: true,
            pose: Default::default(),
            builtin: false,
            device_info,
        },
        chip,
    };

    device_client.add_chip_with_id(request).await.map_err(ServerError::DeviceActor)
}

fn extract_device_info(
    device_name: &str,
    query_params: &mut HashMap<String, String>,
) -> Option<DeviceInfo> {
    let mut info = DeviceInfo { name: device_name.to_string(), ..Default::default() };
    let mut has_device_info = false;
    for (key, field) in [
        ("device_kind", &mut info.kind),
        ("version", &mut info.version),
        ("sdk_version", &mut info.sdk_version),
        ("build_id", &mut info.build_id),
        ("variant", &mut info.variant),
        ("arch", &mut info.arch),
        ("avd_path", &mut info.avd_path),
    ] {
        if let Some(val) = query_params.remove(key).filter(|s| !s.is_empty()) {
            *field = val;
            has_device_info = true;
        }
    }

    has_device_info.then_some(info)
}

fn build_chip_variant(
    chip_kind: ChipKind,
    address: String,
    device_info: Option<&DeviceInfo>,
) -> Result<ChipVariant, ChipError> {
    let mut variant = ChipVariant::try_from(chip_kind)?;
    match &mut variant {
        ChipVariant::Bluetooth(bt) => bt.address = address,
        ChipVariant::Uwb(uwb) => uwb.address = address,
        ChipVariant::Cell(cell) => {
            cell.quirks = device_info
                .map(|info| {
                    let is_cuttlefish = info.kind.eq_ignore_ascii_case("CUTTLEFISH");
                    let is_emulator = info.kind.eq_ignore_ascii_case("EMULATOR");
                    let sdk_version = info.sdk_version.parse::<i32>().unwrap_or(0);
                    Quirks {
                        goldfish_ril_37_or_earlier: is_emulator && sdk_version < 38,
                        is_cuttlefish,
                        auto_ctzv: is_cuttlefish || is_emulator,
                        allow_ipv4_for_ipv6: is_cuttlefish || is_emulator,
                        dynamic_slirp_lease: is_emulator,
                    }
                })
                .unwrap_or_default();
        }
        ChipVariant::Wifi(_)
        | ChipVariant::Nfc(_)
        | ChipVariant::CellularData(_)
        | ChipVariant::Ethernet(_) => {}
    }
    Ok(variant)
}

/// Echoes every packet back to the client until either side closes.
///
/// Uses the same stream/sink adapters as a registered chip, so a loopback
/// stream exercises the same plumbing without creating a device.
pub(crate) async fn run_loopback(
    addr: SocketAddr,
    stream_rx: mpsc::Receiver<Result<Bytes, io::Error>>,
    sink_tx: mpsc::Sender<Bytes>,
) {
    let (packet_stream, packet_sink) = packet_stream_and_sink(stream_rx, sink_tx);
    if let Err(error) = packet_stream.map(Ok).forward(packet_sink).await {
        warn!(%error, "WebSocket loopback stream for {addr} failed");
    }
}

/// Adapts the WebSocket transport channels to the device actor's packet types.
fn packet_stream_and_sink(
    stream_rx: mpsc::Receiver<Result<Bytes, io::Error>>,
    sink_tx: mpsc::Sender<Bytes>,
) -> (PacketStream, PacketSink) {
    let packet_stream: PacketStream =
        Box::new(ReceiverStream::new(stream_rx).filter_map(|res| ready(res.ok())));
    let packet_sink: PacketSink =
        Box::pin(unfold(sink_tx, |tx: mpsc::Sender<Bytes>, item: Bytes| async move {
            tx.send(item).await.map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err))?;
            Ok(tx)
        }));
    (packet_stream, packet_sink)
}

/// Context for a device registered with the device actor.
pub(crate) struct RegisteredDevice {
    pub(crate) device_id: DeviceId,
    pub(crate) chip_id: ChipId,
    pub(crate) client: DeviceClient,
}

/// Spawns threads and tasks to manage WebSocket traffic and chip lifecycle.
///
/// `device` is the registered chip to remove on close; `None` for loopback.
pub(crate) async fn run_websocket_transport(
    mut websocket_reader: WebSocket<TcpStream>,
    mut websocket_writer: WebSocket<TcpStream>,
    addr: SocketAddr,
    chip_kind: ChipKind,
    stream_tx: mpsc::Sender<Result<Bytes, io::Error>>,
    mut sink_rx: mpsc::Receiver<Bytes>,
    device: Option<Arc<RegisteredDevice>>,
) {
    let (outbound_msg_tx, mut outbound_msg_rx) = mpsc::unbounded_channel::<Message>();

    // 1. Read Loop: WebSocket -> Packet Stream (HCI traffic from client)
    let outbound_msg_tx_clone = outbound_msg_tx.clone();
    let rt_handle = tokio::runtime::Handle::current();
    // Arc avoids invoking DeviceClient::clone() (which calls inner.clone_box(),
    // panicking on connection-scoped MockActorClient test mocks that only expect
    // a single clone_box() in server::run).
    let reader_device = device.clone();
    let reader_task = tokio::task::spawn_blocking(move || {
        let mut binary_seen = false;
        loop {
            // Ref: RFC 6455, Section 5.2 - Base Framing Protocol
            match websocket_reader.read() {
                Ok(Message::Binary(data)) => {
                    binary_seen = true;
                    if stream_tx.blocking_send(Ok(data)).is_err() {
                        break;
                    }
                }
                Ok(Message::Text(text)) => {
                    if binary_seen {
                        warn!(
                            "Ignoring WebSocket Text message after binary streaming started for {addr}"
                        );
                        continue;
                    }
                    if chip_kind != ChipKind::BLUETOOTH {
                        warn!(
                            "Ignoring WebSocket Text config frame for non-Bluetooth chip {chip_kind:?} from {addr}"
                        );
                        continue;
                    }
                    let bt_props = match parse_ws_bt_properties(text.as_str()) {
                        Ok(props) if !props.is_empty() => props,
                        Ok(_) => {
                            warn!(
                                "Ignoring WebSocket Text config frame with empty or unrecognized bt_properties for {addr}"
                            );
                            continue;
                        }
                        Err(error) => {
                            warn!(%error, "Failed to parse WebSocket Text config frame for {addr}");
                            continue;
                        }
                    };
                    if let Some(device) = &reader_device {
                        let update = DeviceUpdate {
                            chips: Some(vec![ChipUpdate {
                                id: None,
                                variant: Some(ChipVariantUpdate::Bluetooth(BluetoothUpdate {
                                    bt_properties: Some(bt_props),
                                    ..Default::default()
                                })),
                                ..Default::default()
                            }]),
                            ..Default::default()
                        };
                        // Blocking the spawn_blocking thread here is intentional and deadlock-free:
                        // DeviceActor::handle_update and BluetoothActor::handle_update only mutate
                        // actor state and call rootcanal.set_properties without awaiting this WebSocket
                        // reader task, and completing the update before reading the next frame ensures
                        // RootCanal controller properties are applied prior to the first binary HCI packet.
                        if let Err(error) =
                            rt_handle.block_on(device.client.update(device.device_id, update))
                        {
                            warn!(%error, "Failed to apply WebSocket chip config for {addr}");
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    let _ = outbound_msg_tx_clone.send(Message::Pong(data));
                }
                Ok(Message::Close(close_frame)) => {
                    let _ = outbound_msg_tx_clone.send(Message::Close(close_frame));
                    break;
                }
                Ok(_) => {} // Ignore Pong, Frame
                Err(error) => {
                    warn!(%error, "WebSocket read error for {addr}");
                    break;
                }
            }
        }
        info!("WebSocket reader loop ended for {addr}");
    });

    // 2. Write Loop: ws_msg_rx -> WebSocket (HCI traffic to client)
    let writer_task = tokio::task::spawn_blocking(move || {
        while let Some(msg) = outbound_msg_rx.blocking_recv() {
            let is_close = matches!(msg, Message::Close(_));
            if let Err(error) = websocket_writer.send(msg) {
                warn!(%error, "WebSocket write error for {addr}");
                break;
            }
            if is_close {
                break;
            }
        }
        let _ = websocket_writer.flush();
        let _ = websocket_writer.get_ref().shutdown(std::net::Shutdown::Read);
    });

    // 3. Forward Task: Packet Sink -> WebSocket (Async to Sync bridge)
    tokio::spawn(async move {
        while let Some(packet) = sink_rx.recv().await {
            if outbound_msg_tx.send(Message::Binary(packet)).is_err() {
                break;
            }
        }
        let _ = outbound_msg_tx.send(Message::Close(None));
    });

    // 4. Cleanup Task: Triggered when WebSocket reader or writer closes
    JoinSet::from_iter([reader_task, writer_task]).join_next().await;
    let Some(device) = device else {
        info!("WebSocket loopback stream for {addr} closed");
        return;
    };
    let RegisteredDevice { device_id, chip_id, client } = &*device;
    info!("Cleaning up WebSocket chip {chip_id} on device {device_id} for {addr}");
    if let Err(error) = client.notify_chip_removed(*device_id, *chip_id).await {
        debug!(%error, "WebSocket chip {chip_id} on device {device_id} already removed");
    }
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum BtPropsOrEnvelope {
    Envelope { bt_properties: BtControllerProperties },
    Direct(BtControllerProperties),
}

fn parse_ws_bt_properties(text: &str) -> Result<BtControllerProperties, serde_json::Error> {
    serde_json::from_str::<BtPropsOrEnvelope>(text).map(|res| match res {
        BtPropsOrEnvelope::Envelope { bt_properties } => bt_properties,
        BtPropsOrEnvelope::Direct(bt_properties) => bt_properties,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_chip_variant_rejects_unspecified() {
        assert!(matches!(
            build_chip_variant(ChipKind::UNSPECIFIED, String::new(), None),
            Err(ChipError::UnsupportedChipKind(ChipKind::UNSPECIFIED))
        ));
    }

    #[test]
    fn test_build_chip_variant_sets_address_and_cellular_quirks() {
        let bt = build_chip_variant(ChipKind::BLUETOOTH, "00:11:22:33:44:55".to_string(), None)
            .expect("bluetooth variant should build");
        match bt {
            ChipVariant::Bluetooth(bt) => assert_eq!(bt.address, "00:11:22:33:44:55"),
            _ => panic!("expected bluetooth variant"),
        }

        let uwb = build_chip_variant(ChipKind::UWB, "00:11:22:33".to_string(), None)
            .expect("uwb variant should build");
        match uwb {
            ChipVariant::Uwb(uwb) => assert_eq!(uwb.address, "00:11:22:33"),
            _ => panic!("expected uwb variant"),
        }

        let emulator_info_37 = DeviceInfo {
            kind: "EMULATOR".to_string(),
            sdk_version: "37".to_string(),
            ..Default::default()
        };
        let cell_37 =
            build_chip_variant(ChipKind::CELLULAR, String::new(), Some(&emulator_info_37))
                .expect("cell variant should build");
        match cell_37 {
            ChipVariant::Cell(cell) => {
                assert!(cell.quirks.goldfish_ril_37_or_earlier);
                assert!(!cell.quirks.is_cuttlefish);
                assert!(cell.quirks.auto_ctzv);
                assert!(cell.quirks.allow_ipv4_for_ipv6);
                assert!(cell.quirks.dynamic_slirp_lease);
            }
            _ => panic!("expected cell variant"),
        }

        let emulator_info_38 = DeviceInfo {
            kind: "EMULATOR".to_string(),
            sdk_version: "38".to_string(),
            ..Default::default()
        };
        let cell_38 =
            build_chip_variant(ChipKind::CELLULAR, String::new(), Some(&emulator_info_38))
                .expect("cell variant should build");
        match cell_38 {
            ChipVariant::Cell(cell) => {
                assert!(!cell.quirks.goldfish_ril_37_or_earlier);
                assert!(!cell.quirks.is_cuttlefish);
                assert!(cell.quirks.auto_ctzv);
                assert!(cell.quirks.allow_ipv4_for_ipv6);
                assert!(cell.quirks.dynamic_slirp_lease);
            }
            _ => panic!("expected cell variant"),
        }

        let cuttlefish_info = DeviceInfo {
            kind: "CUTTLEFISH".to_string(),
            sdk_version: "35".to_string(),
            ..Default::default()
        };
        let cell_cvd =
            build_chip_variant(ChipKind::CELLULAR, String::new(), Some(&cuttlefish_info))
                .expect("cell variant should build");
        match cell_cvd {
            ChipVariant::Cell(cell) => {
                assert!(!cell.quirks.goldfish_ril_37_or_earlier);
                assert!(cell.quirks.is_cuttlefish);
                assert!(cell.quirks.auto_ctzv);
                assert!(cell.quirks.allow_ipv4_for_ipv6);
                assert!(!cell.quirks.dynamic_slirp_lease);
            }
            _ => panic!("expected cell variant"),
        }

        let cell_default = build_chip_variant(ChipKind::CELLULAR, String::new(), None)
            .expect("cell variant should build");
        match cell_default {
            ChipVariant::Cell(cell) => {
                assert!(!cell.quirks.goldfish_ril_37_or_earlier);
                assert!(!cell.quirks.is_cuttlefish);
                assert!(!cell.quirks.auto_ctzv);
                assert!(!cell.quirks.allow_ipv4_for_ipv6);
                assert!(!cell.quirks.dynamic_slirp_lease);
            }
            _ => panic!("expected cell variant"),
        }

        let ethernet = build_chip_variant(ChipKind::ETHERNET, String::new(), None)
            .expect("ethernet variant should build");
        assert!(matches!(ethernet, ChipVariant::Ethernet(_)));
    }

    #[test]
    fn test_parse_ws_bt_properties_preset_direct() {
        let json = r#"{"preset": "laird_bl654"}"#;
        let props = parse_ws_bt_properties(json).expect("should parse direct preset");
        assert_eq!(props.preset.as_deref(), Some("laird_bl654"));
        assert!(!props.is_empty());
    }

    #[test]
    fn test_parse_ws_bt_properties_preset_envelope() {
        let json = r#"{"bt_properties": {"preset": "laird_bl654", "strict": true}}"#;
        let props = parse_ws_bt_properties(json).expect("should parse enveloped preset");
        assert_eq!(props.preset.as_deref(), Some("laird_bl654"));
        assert_eq!(props.strict, Some(true));
        assert!(!props.is_empty());
    }

    #[test]
    fn test_parse_ws_bt_properties_features() {
        let json = r#"{"features": {"le_connected_isochronous_stream": false}}"#;
        let props = parse_ws_bt_properties(json).expect("should parse features");
        assert_eq!(
            props.features.as_ref().and_then(|f| f.le_connected_isochronous_stream),
            Some(false)
        );
        assert!(!props.is_empty());
    }

    #[test]
    fn test_parse_ws_bt_properties_empty() {
        let json = "{}";
        let props = parse_ws_bt_properties(json).expect("should parse empty object");
        assert!(props.is_empty());
    }

    #[test]
    fn test_parse_ws_bt_properties_nested_empty_substructs_are_empty() {
        let empty_json = r#"{"features":{},"quirks":{},"vendor":{}}"#;
        let props = parse_ws_bt_properties(empty_json).expect("should parse empty substructs");
        assert!(props.is_empty());

        let with_feature = r#"{"features":{"le_2m_phy":true}}"#;
        assert!(!parse_ws_bt_properties(with_feature).unwrap().is_empty());

        let with_quirk = r#"{"quirks":{"has_default_random_address":true}}"#;
        assert!(!parse_ws_bt_properties(with_quirk).unwrap().is_empty());

        let with_vendor = r#"{"vendor":{"csr":true}}"#;
        assert!(!parse_ws_bt_properties(with_vendor).unwrap().is_empty());

        let with_strict = r#"{"strict":true}"#;
        assert!(!parse_ws_bt_properties(with_strict).unwrap().is_empty());

        let with_preset = r#"{"preset":"default"}"#;
        assert!(!parse_ws_bt_properties(with_preset).unwrap().is_empty());
    }

    #[test]
    fn test_parse_ws_bt_properties_invalid() {
        assert!(parse_ws_bt_properties("not json").is_err());
    }
}
