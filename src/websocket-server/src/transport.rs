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
    let explicit_address =
        query_params.remove("address").filter(|s| !s.is_empty()).unwrap_or_default();
    // Legacy clients pass the MAC as a 17-char chip_id; honor it as-is when no
    // explicit address is given, but never persist it as an explicit override.
    let legacy_address = chip_id.filter(|id| id.len() == 17);
    let address = match legacy_address {
        Some(legacy) if explicit_address.is_empty() => legacy,
        _ => {
            resolve_bluetooth_address(
                chip_kind,
                &device_name,
                device_info.as_ref(),
                &explicit_address,
            )
            .await
        }
    };

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

/// Resolves the Bluetooth MAC once per connection, mirroring the gRPC/fd path in
/// netsimd. Unnamed devices (default `websocket-device-{addr}` name) have no
/// stable identity across reconnects, so they only register an explicit address
/// to avoid collisions instead of being keyed by name.
async fn resolve_bluetooth_address(
    chip_kind: ChipKind,
    device_name: &str,
    device_info: Option<&DeviceInfo>,
    explicit_address: &str,
) -> String {
    if chip_kind != ChipKind::BLUETOOTH {
        return explicit_address.to_string();
    }
    let avd_path = device_info.map(|info| info.avd_path.as_str()).unwrap_or_default();
    let is_unnamed = device_name.starts_with("websocket-device-");
    if avd_path.is_empty() && is_unnamed && explicit_address.is_empty() {
        return String::new();
    }
    let key = if is_unnamed { "" } else { device_name };
    common::util::bluetooth_mac::resolve_bluetooth_mac_for_device(avd_path, key, explicit_address)
        .await
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
    if let Some(val) = query_params.remove("timezone").filter(|s| !s.is_empty()) {
        info.timezone = Some(val);
        has_device_info = true;
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
            cell.timezone_override = device_info
                .and_then(|info| info.timezone.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            cell.quirks = Quirks::from_device_info(device_info);
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
    // `packet_stream` yields only `Ok` items and `packet_sink` returns only `BrokenPipe` when
    // `sink_rx` closes, so any error here is expected teardown.
    if let Err(error) = packet_stream.map(Ok).forward(packet_sink).await {
        debug!(%error, "WebSocket loopback stream for {addr} closed");
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

/// Runs `f` on Tokio's blocking pool, forwarding the active test `tracing` dispatch.
pub(crate) fn spawn_blocking_traced<F, R>(f: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    #[cfg(test)]
    {
        let dispatch = tracing::dispatcher::get_default(|d| d.clone());
        tokio::task::spawn_blocking(move || {
            let _guard = tracing::dispatcher::set_default(&dispatch);
            f()
        })
    }
    #[cfg(not(test))]
    {
        tokio::task::spawn_blocking(f)
    }
}

pub(crate) fn is_daemon_shutdown_client_error(
    device_client: &DeviceClient,
    error: &netsim_model::ClientError,
) -> bool {
    device_client.is_shutting_down()
        && matches!(error, netsim_model::ClientError::Framework(_))
        && device_actor::DeviceError::downcast_from(error).is_none()
}

pub(crate) fn is_expected_io_close(io_err: &io::Error) -> bool {
    use io::ErrorKind::*;
    matches!(io_err.kind(), BrokenPipe | ConnectionReset | ConnectionAborted | NotConnected)
}

fn is_expected_read_close(error: &tungstenite::Error) -> bool {
    use tungstenite::{Error::*, error::ProtocolError::ResetWithoutClosingHandshake};
    match error {
        ConnectionClosed | AlreadyClosed | Protocol(ResetWithoutClosingHandshake) => true,
        Io(e) => e.kind() == io::ErrorKind::UnexpectedEof || is_expected_io_close(e),
        _ => false,
    }
}

pub(crate) fn is_expected_write_close(error: &tungstenite::Error) -> bool {
    use tungstenite::Error::*;
    matches!(error, ConnectionClosed | AlreadyClosed)
        || matches!(error, Io(e) if is_expected_io_close(e))
}

/// Spawns threads and tasks to manage WebSocket traffic and chip lifecycle.
///
/// `device` is the registered chip to remove on close; `None` for loopback.
pub(crate) async fn run_websocket_transport(
    websocket_reader: WebSocket<TcpStream>,
    websocket_writer: WebSocket<TcpStream>,
    addr: SocketAddr,
    chip_kind: ChipKind,
    stream_tx: mpsc::Sender<Result<Bytes, io::Error>>,
    sink_rx: mpsc::Receiver<Bytes>,
    device: Option<Arc<RegisteredDevice>>,
) {
    let _ = run_transport_inner(
        websocket_reader,
        websocket_writer,
        addr,
        chip_kind,
        stream_tx,
        sink_rx,
        device,
    )
    .await;
}

async fn run_transport_inner(
    mut websocket_reader: WebSocket<TcpStream>,
    mut websocket_writer: WebSocket<TcpStream>,
    addr: SocketAddr,
    chip_kind: ChipKind,
    stream_tx: mpsc::Sender<Result<Bytes, io::Error>>,
    mut sink_rx: mpsc::Receiver<Bytes>,
    device: Option<Arc<RegisteredDevice>>,
) -> JoinSet<Result<(), tokio::task::JoinError>> {
    // TODO(b/586144449): Drain both transport tasks with bounded close-flush/read timeouts and
    // single Arc<TcpStream>.
    let (outbound_msg_tx, mut outbound_msg_rx) = mpsc::unbounded_channel::<Message>();

    // 1. Read Loop: WebSocket -> Packet Stream (HCI traffic from client)
    let outbound_msg_tx_clone = outbound_msg_tx.clone();
    let rt_handle = tokio::runtime::Handle::current();
    // Arc avoids invoking DeviceClient::clone() (which calls inner.clone_box(),
    // panicking on connection-scoped MockActorClient test mocks that only expect
    // a single clone_box() in server::run).
    let reader_device = device.clone();
    let reader_task = spawn_blocking_traced(move || {
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
                            code = crate::err::Code::TextFrameAfterBinary as u32,
                            "Ignoring WebSocket Text message after binary streaming started for {addr}"
                        );
                        continue;
                    }
                    if chip_kind != ChipKind::BLUETOOTH {
                        warn!(
                            code = crate::err::Code::TextFrameNonBluetooth as u32,
                            "Ignoring WebSocket Text config frame for non-Bluetooth chip {chip_kind:?} from {addr}"
                        );
                        continue;
                    }
                    let bt_props = match parse_ws_bt_properties(text.as_str()) {
                        Ok(props) if !props.is_empty() => props,
                        Ok(_) => {
                            warn!(
                                code = crate::err::Code::TextFrameEmptyBtProps as u32,
                                "Ignoring WebSocket Text config frame with empty or unrecognized bt_properties for {addr}"
                            );
                            continue;
                        }
                        Err(error) => {
                            warn!(
                                code = crate::err::Code::TextFrameParseFailed as u32,
                                %error,
                                "Failed to parse WebSocket Text config frame for {addr}"
                            );
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
                            if is_daemon_shutdown_client_error(&device.client, &error) {
                                debug!(%error, "Skipping chip config during shutdown for {addr}");
                            } else {
                                warn!(
                                    code = crate::err::Code::ApplyBtConfigFailed as u32,
                                    %error,
                                    "Failed to apply WebSocket chip config for {addr}"
                                );
                            }
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
                    if is_expected_read_close(&error) {
                        debug!(%error, "WebSocket reader closed for {addr}");
                    } else {
                        warn!(
                            code = crate::err::Code::TransportReadFailed as u32,
                            %error,
                            "WebSocket read error for {addr}"
                        );
                    }
                    break;
                }
            }
        }
        info!("WebSocket reader loop ended for {addr}");
    });

    // 2. Write Loop: ws_msg_rx -> WebSocket (HCI traffic to client)
    let writer_task = spawn_blocking_traced(move || {
        run_ws_writer_loop(&mut websocket_writer, addr, &mut outbound_msg_rx);
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
    let mut tasks = JoinSet::from_iter([reader_task, writer_task]);
    tasks.join_next().await;
    let Some(device) = device else {
        info!("WebSocket loopback stream for {addr} closed");
        return tasks;
    };
    let RegisteredDevice { device_id, chip_id, client } = &*device;
    info!("Cleaning up WebSocket chip {chip_id} on device {device_id} for {addr}");
    if let Err(error) = client.notify_chip_removed(*device_id, *chip_id).await {
        debug!(%error, "WebSocket chip {chip_id} on device {device_id} already removed");
    }
    tasks
}

fn run_ws_writer_loop<W: io::Read + io::Write>(
    websocket_writer: &mut WebSocket<W>,
    addr: SocketAddr,
    outbound_msg_rx: &mut mpsc::UnboundedReceiver<Message>,
) {
    while let Some(msg) = outbound_msg_rx.blocking_recv() {
        let is_close = matches!(msg, Message::Close(_));
        if let Err(error) = websocket_writer.send(msg) {
            if is_close || is_expected_write_close(&error) {
                debug!(%error, "WebSocket writer closed for {addr}");
            } else {
                warn!(
                    code = crate::err::Code::TransportWriteFailed as u32,
                    %error,
                    "WebSocket write error for {addr}"
                );
            }
            break;
        }
        if is_close {
            break;
        }
    }
    let _ = websocket_writer.flush();
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum BtPropsOrEnvelope {
    Envelope { bt_properties: BtControllerProperties },
    Direct(BtControllerProperties),
}

fn parse_ws_bt_properties(text: &str) -> Result<BtControllerProperties, serde_json::Error> {
    serde_json::from_str::<BtPropsOrEnvelope>(text).map(|res| match res {
        BtPropsOrEnvelope::Envelope { bt_properties }
        | BtPropsOrEnvelope::Direct(bt_properties) => bt_properties,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use actor_framework::{FrameworkError, MockActorClient};
    use device_actor::{DeviceActor, DeviceError};
    use tokio::net::{TcpListener, TcpStream as AsyncTcpStream};
    use tungstenite::protocol::Role;

    use super::*;
    use crate::err::{Code, EXPECTED_IO_CLOSE_KINDS, ErrWriteStream, assert_error_stats_async};

    #[tokio::test]
    async fn test_resolve_bluetooth_address_named_device_is_stable() {
        let first =
            resolve_bluetooth_address(ChipKind::BLUETOOTH, "ws-named-stable", None, "").await;
        assert!(first.starts_with("BB:BB:BB:"), "expected generated MAC, got '{first}'");
        let again =
            resolve_bluetooth_address(ChipKind::BLUETOOTH, "ws-named-stable", None, "").await;
        assert_eq!(first, again);
    }

    #[tokio::test]
    async fn test_resolve_bluetooth_address_unnamed_device() {
        // No identity and no address: nothing to resolve.
        let none =
            resolve_bluetooth_address(ChipKind::BLUETOOTH, "websocket-device-1.2.3.4:1", None, "")
                .await;
        assert!(none.is_empty());
        // Explicit address is normalized and registered.
        let explicit = resolve_bluetooth_address(
            ChipKind::BLUETOOTH,
            "websocket-device-1.2.3.4:2",
            None,
            "bb-bb-bb-00-02-aa",
        )
        .await;
        assert_eq!(explicit, "BB:BB:BB:00:02:AA");
    }

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

        let emulator_info_38_1 = DeviceInfo {
            kind: "EMULATOR".to_string(),
            sdk_version: "38.1".to_string(),
            ..Default::default()
        };
        let cell_38_1 =
            build_chip_variant(ChipKind::CELLULAR, String::new(), Some(&emulator_info_38_1))
                .expect("cell variant should build");
        let ChipVariant::Cell(cell) = cell_38_1 else { panic!("expected cell variant") };
        assert!(!cell.quirks.goldfish_ril_37_or_earlier);

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

    const TRANSPORT_TEST_TIMEOUT: Duration = Duration::from_secs(5);
    // RFC 6455 Section 5.1: unmasked client binary frame (FIN|Binary, len=1, 0xAA).
    const UNMASKED_CLIENT_BINARY_FRAME: &[u8] = &[0x82, 0x01, 0xAA];

    async fn run_ws<R: Send + 'static>(
        chip_kind: ChipKind,
        device: Option<Arc<RegisteredDevice>>,
        drop_sink: bool,
        expected: Option<Code>,
        client_fn: impl FnOnce(TcpStream) -> R + Send + 'static,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (c, a) = tokio::join!(AsyncTcpStream::connect(addr), listener.accept());
        let client_std = c.unwrap().into_std().unwrap();
        client_std.set_nonblocking(false).unwrap();
        client_std.set_read_timeout(Some(TRANSPORT_TEST_TIMEOUT)).unwrap();
        client_std.set_write_timeout(Some(TRANSPORT_TEST_TIMEOUT)).unwrap();
        let (server_stream, client_addr) = a.unwrap();
        let server_std = server_stream.into_std().unwrap();
        server_std.set_nonblocking(false).unwrap();
        let writer =
            WebSocket::from_raw_socket(server_std.try_clone().unwrap(), Role::Server, None);
        let reader = WebSocket::from_raw_socket(server_std, Role::Server, None);
        let (stream_tx, stream_rx) = mpsc::channel(4);
        let (sink_tx, sink_rx) = mpsc::channel(4);
        let keep_sink = (!drop_sink).then_some(sink_tx);
        let fut = async move {
            let client_task = tokio::task::spawn_blocking(move || client_fn(client_std));
            let transport_task = run_transport_inner(
                reader,
                writer,
                client_addr,
                chip_kind,
                stream_tx,
                sink_rx,
                device,
            );
            let done = async {
                let (mut tasks, res) = tokio::join!(transport_task, client_task);
                res.unwrap();
                drop((keep_sink, stream_rx));
                while let Some(res) = tasks.join_next().await {
                    res.unwrap().unwrap();
                }
            };
            tokio::time::timeout(TRANSPORT_TEST_TIMEOUT, done).await.unwrap();
        };
        assert_error_stats_async(fut, expected).await;
    }

    fn send_ws(client_std: TcpStream, messages: Vec<Message>) {
        let mut ws = WebSocket::from_raw_socket(client_std, Role::Client, None);
        messages.into_iter().for_each(|m| ws.send(m).unwrap());
        let _ = ws.send(Message::Close(None));
    }

    #[tokio::test]
    async fn test_run_websocket_transport_records_text_frame_codes() {
        use ChipKind::{BLUETOOTH, WIFI};
        use Code::*;
        let bin = Message::Binary(Bytes::from_static(&[0x01]));
        for (chip_kind, messages, expected) in [
            (BLUETOOTH, vec![bin, Message::text("{}")], TextFrameAfterBinary),
            (WIFI, vec![Message::text("{}")], TextFrameNonBluetooth),
            (BLUETOOTH, vec![Message::text("{}")], TextFrameEmptyBtProps),
            (BLUETOOTH, vec![Message::text("{invalid")], TextFrameParseFailed),
        ] {
            run_ws(chip_kind, None, false, Some(expected), |s| send_ws(s, messages)).await;
        }
    }

    async fn assert_bt_config_err(shut_down: bool, is_device_err: bool, expected: Option<Code>) {
        let mut mock = MockActorClient::<DeviceActor>::new();
        mock.expect_shutdown().returning(|| Ok(()));
        mock.expect_update().returning(move |_, _| {
            Err(if is_device_err {
                FrameworkError::ServiceError(DeviceError::DeviceNotFound("1".into()))
            } else {
                FrameworkError::ActorClosed(Box::new(io::Error::other("closed")))
            })
        });
        mock.expect_perform_action().returning(|_, _| Ok(device_api::DeviceActionResult::Success));
        let device = Arc::new(RegisteredDevice {
            device_id: DeviceId(1),
            chip_id: ChipId(2),
            client: DeviceClient::new(Box::new(mock)),
        });
        if shut_down {
            device.client.shutdown().await.unwrap();
        }
        run_ws(ChipKind::BLUETOOTH, Some(device), false, expected, |s| {
            send_ws(s, vec![Message::text(r#"{"preset":"laird_bl654"}"#)])
        })
        .await;
    }

    #[tokio::test]
    async fn test_run_websocket_transport_records_apply_bt_config_failed() {
        assert_bt_config_err(false, true, Some(Code::ApplyBtConfigFailed)).await;
    }

    #[tokio::test]
    async fn test_run_websocket_transport_shutdown_update_records_no_error_stat() {
        assert_bt_config_err(true, false, None).await;
    }

    #[tokio::test]
    async fn test_run_websocket_transport_unexpected_actor_closed_records_error_stat() {
        assert_bt_config_err(false, false, Some(Code::ApplyBtConfigFailed)).await;
    }

    #[tokio::test]
    async fn test_run_websocket_transport_shutdown_device_error_records_error_stat() {
        assert_bt_config_err(true, true, Some(Code::ApplyBtConfigFailed)).await;
    }

    #[tokio::test]
    async fn test_run_websocket_transport_records_read_failed() {
        use ChipKind::BLUETOOTH;
        let send_unmasked = |mut s: TcpStream| {
            io::Write::write_all(&mut s, UNMASKED_CLIENT_BINARY_FRAME).unwrap();
            s
        };
        run_ws(BLUETOOTH, None, false, Some(Code::TransportReadFailed), send_unmasked).await;
    }

    #[test]
    fn test_is_expected_read_close_io_variants() {
        let io_err = |k| tungstenite::Error::Io(io::Error::from(k));
        for k in EXPECTED_IO_CLOSE_KINDS.into_iter().chain([io::ErrorKind::UnexpectedEof]) {
            assert!(is_expected_read_close(&io_err(k)), "{k:?}");
        }
        assert!(!is_expected_read_close(&io_err(io::ErrorKind::Other)));
    }

    #[tokio::test]
    async fn test_run_websocket_transport_records_write_failed_and_expected_close() {
        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        for (msg, kind, exp) in crate::err::write_error_cases(Code::TransportWriteFailed) {
            let mut w = WebSocket::from_raw_socket(ErrWriteStream(kind), Role::Server, None);
            let (tx, mut rx) = mpsc::unbounded_channel();
            tx.send(msg).unwrap();
            drop(tx);
            let fut = async move {
                spawn_blocking_traced(move || run_ws_writer_loop(&mut w, addr, &mut rx))
                    .await
                    .unwrap();
            };
            assert_error_stats_async(fut, exp).await;
        }
    }

    #[tokio::test]
    async fn test_run_websocket_transport_tcp_drop_records_no_error_stats() {
        run_ws(ChipKind::BLUETOOTH, None, false, None, drop).await;
    }

    #[tokio::test]
    async fn test_run_websocket_transport_clean_close_records_no_error_stats() {
        run_ws(ChipKind::BLUETOOTH, None, false, None, |s| {
            send_ws(s, vec![Message::Binary(Bytes::from_static(&[1, 2]))]);
        })
        .await;
    }

    #[tokio::test]
    async fn test_run_websocket_transport_sink_drop_records_no_error_stats() {
        run_ws(ChipKind::BLUETOOTH, None, true, None, |s| {
            let mut ws = WebSocket::from_raw_socket(s, Role::Client, None);
            assert!(matches!(ws.read(), Ok(Message::Close(_))));
        })
        .await;
    }

    #[tokio::test]
    async fn test_run_loopback_teardown_records_no_error_stats() {
        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let (stream_tx, stream_rx) = mpsc::channel(4);
        let (sink_tx, sink_rx) = mpsc::channel(4);
        drop(sink_rx);
        stream_tx.send(Ok(Bytes::from_static(&[0x01]))).await.unwrap();
        let fut = assert_error_stats_async(run_loopback(addr, stream_rx, sink_tx), None);
        tokio::time::timeout(TRANSPORT_TEST_TIMEOUT, fut).await.unwrap();
    }
}
