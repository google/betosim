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

//! Cuttlefish (AOSP Soong) platform initialization policy.

use std::{collections::HashMap, io, time::Duration};

use bytes::Bytes;
use common::util::net::bind_tcp_loopback_with_ephemeral_fallback;
use device_actor::DeviceClient;
use futures::{SinkExt, StreamExt};
use packet_stream::{
    ChipInfo as PsChipInfo, DualFdConfig, DualFdListener, InitInfo as PsInitInfo, StreamAddress,
    Streams,
    transport::traits::{PacketSink, PacketStream, TransportListener},
};
use tokio::{
    net::TcpStream,
    select,
    task::{JoinHandle, JoinSet},
};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{error, info, warn};

use crate::{args::Args, netsimd::RunResult};

const DEFAULT_ROOTCANAL_TEST_PORT: u16 = 7500;
const RETRY_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConnectorTarget {
    WebSocket(String),
    Tcp(String),
}

#[derive(Debug, Default)]
pub(crate) struct PlatformTasks {
    test_task: Option<JoinHandle<()>>,
}

impl PlatformTasks {
    pub(crate) fn abort(&self) {
        if let Some(task) = &self.test_task {
            task.abort();
        }
    }
}

impl Drop for PlatformTasks {
    fn drop(&mut self) {
        self.abort();
    }
}

pub(crate) fn preflight_check(_args: &Args) -> Result<(), RunResult> {
    Ok(())
}

pub(crate) fn init_transport_listeners(
    args: &Args,
    streams: &mut Streams,
) -> std::future::Ready<()> {
    #[cfg(target_os = "linux")]
    if let Some(fd_startup_str) = &args.fd_startup_str {
        init_dualfd_listener(fd_startup_str, streams);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (args, streams);
    std::future::ready(())
}

#[cfg(target_os = "linux")]
fn init_dualfd_listener(fd_startup_str: &str, streams: &mut Streams) {
    if fd_startup_str.is_empty() {
        return;
    }
    match serde_json::from_str::<DualFdConfig>(fd_startup_str) {
        Ok(config) => match DualFdListener::new(config) {
            Ok(listener) => {
                if let Err(error) = streams.add_listener("netsim_dualfd", Box::new(listener)) {
                    error!(%error, "Failed to add DualFdListener");
                } else {
                    info!("Added DualFdListener");
                }
            }
            Err(error) => {
                error!(%error, "Failed to create DualFdListener");
            }
        },
        Err(error) => {
            error!(%error, "Failed to parse fd_startup_str JSON");
        }
    }
}

pub(crate) fn start_servers(
    args: &Args,
    instance_num: u16,
    device_client: &DeviceClient,
    listener_addresses: &mut HashMap<String, StreamAddress>,
    ini_data: &mut HashMap<String, String>,
) -> Result<PlatformTasks, RunResult> {
    let mut test_task = None;
    let is_explicit_test = args.test_port.is_some();
    let test_port =
        args.test_port.unwrap_or_else(|| DEFAULT_ROOTCANAL_TEST_PORT + instance_num - 1);
    if let Some(listener) = bind_tcp_loopback_with_ephemeral_fallback(
        test_port,
        is_explicit_test,
        "Rootcanal control server",
    )
    .map_err(|error| {
        crate::netsimd::init_error(format!(
            "Failed to bind requested Rootcanal control server port {test_port}: {error}"
        ))
    })? {
        let local_addr = listener.local_addr().map_err(crate::netsimd::init_error)?;
        listener_addresses.insert("netsim_rootcanal".to_string(), StreamAddress::Tcp(local_addr));
        ini_data.insert("test.port".to_string(), local_addr.port().to_string());
        // Spawn legacy Rootcanal control server for host-side test
        // runners (like pts-bot/mmi2grpc).
        test_task = Some(tokio::spawn(rootcanal_server::run(listener, device_client.clone())));
    }
    Ok(PlatformTasks { test_task })
}

#[must_use]
pub(crate) const fn should_start_tcp_listener(_resolved_tcp_port: Option<u16>) -> bool {
    true
}

#[must_use]
pub(crate) const fn default_test_beacons() -> bool {
    true
}

/// Runs connector/forwarder mode on Cuttlefish if requested by `--connector_instance`.
///
/// Connector/forwarder mode is Cuttlefish-only and is selected when
/// `--connector_instance` is passed.
///
/// Returns:
/// - `None`: Not running in connector mode; the caller should continue normal
///   `NetsimDaemon` startup.
/// - `Some(result)`: Connector mode ran to completion; the caller must return
///   `result` immediately without starting the primary daemon.
///
/// As a side effect on Cuttlefish, this redirects daemon logs for the connector
/// instance before establishing packet bridges.
pub(crate) async fn run_connector_mode_if_requested(args: &Args) -> Option<RunResult> {
    let connector_instance = args.connector_instance?;
    crate::netsimd::NetsimDaemon::redirect_daemon_logs(args, Some(connector_instance));
    info!("netsim startup (Connector mode)");
    Some(match run_netsimd_connector(args, connector_instance).await {
        Ok(()) => RunResult::ExitedNormally,
        Err(err) => RunResult::InitializationError(err),
    })
}

fn resolve_connector_target(
    ws_addr: Option<String>,
    tcp_addr: Option<String>,
) -> Option<ConnectorTarget> {
    ws_addr.map(ConnectorTarget::WebSocket).or_else(|| tcp_addr.map(ConnectorTarget::Tcp))
}

async fn bridge_packets<S, K>(
    packet_stream: &mut PacketStream,
    packet_sink: &mut PacketSink,
    mut remote_stream: S,
    mut remote_sink: K,
    chip_id: &str,
    transport_name: &str,
) -> bool
where
    S: futures::Stream<Item = io::Result<Bytes>> + Unpin,
    K: futures::Sink<Bytes, Error = io::Error> + Unpin,
{
    enum BridgeError {
        Guest(String),
        Remote(String),
    }

    select! {
        res = async {
            while let Some(packet) = packet_stream.next().await {
                let packet =
                    packet.map_err(|e| BridgeError::Guest(format!("read error: {e}")))?;
                remote_sink
                    .send(packet)
                    .await
                    .map_err(|e| BridgeError::Remote(format!("write error: {e}")))?;
            }
            Ok::<(), BridgeError>(())
        } => {
            match res {
                Err(BridgeError::Guest(error)) => {
                    error!(
                        %error,
                        %chip_id,
                        "Upstream bridge failed for chip (Guest)"
                    );
                    true
                }
                Err(BridgeError::Remote(error)) => {
                    error!(
                        %error,
                        %chip_id,
                        %transport_name,
                        "Upstream bridge failed for chip"
                    );
                    false
                }
                Ok(()) => true, // Guest FD reader hit EOF, break loop permanently!
            }
        }
        res = async {
            while let Some(packet) = remote_stream.next().await {
                let packet =
                    packet.map_err(|e| BridgeError::Remote(format!("read error: {e}")))?;
                packet_sink
                    .send(packet)
                    .await
                    .map_err(|e| BridgeError::Guest(format!("write error: {e}")))?;
            }
            Ok::<(), BridgeError>(())
        } => {
            match res {
                Err(BridgeError::Guest(error)) => {
                    error!(
                        %error,
                        %chip_id,
                        "Downstream bridge failed for chip (Guest)"
                    );
                    true
                }
                Err(BridgeError::Remote(error)) => {
                    error!(
                        %error,
                        %chip_id,
                        %transport_name,
                        "Downstream bridge failed for chip"
                    );
                    false
                }
                Ok(()) => false, // Remote stream disconnected, need to reconnect
            }
        }
    }
}

async fn run_chip_bridge(
    mut packet_stream: PacketStream,
    mut packet_sink: PacketSink,
    chip_info: PsChipInfo,
    device_guid: String,
    target: ConnectorTarget,
) {
    let chip_id = chip_info.chip.as_ref().map_or("Unknown".to_string(), |c| c.id.clone());

    loop {
        let is_guest_disconnected = match &target {
            ConnectorTarget::WebSocket(server_addr) => {
                let path =
                    websocket_server::client::build_packet_stream_path(&chip_info, &device_guid);
                match websocket_server::client::connect(server_addr, &path).await {
                    Ok((ws_stream, ws_sink)) => {
                        info!(
                            %chip_id,
                            "Connected WebSocket forwarder to primary daemon"
                        );
                        bridge_packets(
                            &mut packet_stream,
                            &mut packet_sink,
                            ws_stream,
                            ws_sink,
                            &chip_id,
                            "WebSocket",
                        )
                        .await
                    }
                    Err(error) => {
                        warn!(
                            %error,
                            "Failed to connect to primary daemon WebSocket {server_addr}{path}"
                        );
                        false
                    }
                }
            }
            ConnectorTarget::Tcp(server_addr) => match TcpStream::connect(server_addr).await {
                Ok(tcp_stream_raw) => {
                    let init_info = PsInitInfo {
                        chip_info: chip_info.clone(),
                        transport_type: "TCP Forwarder".to_string(),
                    };

                    let mut framed_tcp = Framed::new(tcp_stream_raw, LengthDelimitedCodec::new());

                    let init_json = match serde_json::to_vec(&init_info) {
                        Ok(json) => json,
                        Err(error) => {
                            error!(%error, "Failed to serialize InitInfo");
                            break;
                        }
                    };

                    if let Err(error) = framed_tcp.send(Bytes::from(init_json)).await {
                        error!(%error, "Failed to send InitInfo");
                        false
                    } else {
                        info!("Sent initial ChipInfo to primary daemon for chip {chip_id}");

                        let (tcp_sink, tcp_stream) = framed_tcp.split();
                        let tcp_stream = tcp_stream.map(|res| res.map(bytes::BytesMut::freeze));
                        bridge_packets(
                            &mut packet_stream,
                            &mut packet_sink,
                            tcp_stream,
                            tcp_sink,
                            &chip_id,
                            "TCP",
                        )
                        .await
                    }
                }
                Err(error) => {
                    warn!(%error, "Failed to connect to primary daemon TCP port");
                    false
                }
            },
        };

        if is_guest_disconnected {
            info!("Guest radio transport disconnected permanently for chip {chip_id}");
            break;
        }

        info!(
            %chip_id,
            "Forwarder stream disconnected, re-establishing session in 1s..."
        );
        tokio::time::sleep(RETRY_INTERVAL).await;
    }
    info!("Duplex bridge closed for chip {chip_id}");
}

/// Orchestrates the Cuttlefish packet forwarder connector daemon.
async fn run_netsimd_connector(args: &Args, connector_instance: u16) -> Result<(), String> {
    let Some(fd_startup_str) = args.fd_startup_str.as_deref() else {
        return Err("Failed to start netsimd forwarder, missing `-s` arg".to_string());
    };

    let dual_fd_config: DualFdConfig = serde_json::from_str(fd_startup_str)
        .map_err(|e| format!("Could not parse startup info JSON: {e}"))?;

    let Some(target) = resolve_connector_target(
        common::util::ini_file::get_ws_server_address(connector_instance),
        common::util::ini_file::get_tcp_server_address(connector_instance),
    ) else {
        return Err(format!(
            "No primary netsimd ws or tcp port found for instance {connector_instance}"
        ));
    };

    info!("Starting in Connector mode to {target:?}");

    let mut join_set = JoinSet::new();

    let mut listener = DualFdListener::new(dual_fd_config)
        .map_err(|e| format!("Failed to create DualFdListener: {e}"))?;

    loop {
        tokio::select! {
            res = listener.accept() => {
                match res {
                    Ok((packet_stream, packet_sink, chip_info, guid)) => {
                        join_set.spawn(run_chip_bridge(
                            packet_stream,
                            packet_sink,
                            chip_info,
                            guid,
                            target.clone(),
                        ));
                    }
                    Err(error) => {
                        error!(%error, "Error accepting connection");
                        break;
                    }
                }
            }
            res = join_set.join_next(), if !join_set.is_empty() => {
                if let Some(res) = res {
                    if let Err(e) = res {
                        return Err(format!("Bridge task panicked: {e}"));
                    }
                    return Err("A guest radio transport disconnected".to_string());
                }
            }
        }
    }

    info!("Connector forwarder disconnected unexpectedly");
    Err("Connector forwarder disconnected unexpectedly from guest FDs".to_string())
}

#[cfg(test)]
mod tests {
    use common::util::net::bind_tcp_loopback;
    use netsim_model::{ChipInfo, ChipKind};
    use tokio::sync::mpsc;

    use super::*;

    #[test]
    fn test_cuttlefish_defaults_enable_tcp_listener_and_test_beacons() {
        assert!(should_start_tcp_listener(None));
        assert!(should_start_tcp_listener(Some(8080)));
        assert!(default_test_beacons());
    }

    #[test]
    fn test_cuttlefish_preflight_check_succeeds() {
        let args = Args::default();
        assert!(preflight_check(&args).is_ok());
    }

    #[tokio::test]
    async fn test_cuttlefish_start_servers_binds_rootcanal_and_aborts_task() {
        let args = Args { test_port: Some(0), ..Default::default() };
        let (_runner, device_client) = device_actor::new();
        let mut listener_addresses = HashMap::new();
        let mut ini_data = HashMap::new();

        let tasks = start_servers(&args, 1, &device_client, &mut listener_addresses, &mut ini_data)
            .expect("start_servers with ephemeral port should succeed");

        let rootcanal_addr = listener_addresses
            .get("netsim_rootcanal")
            .expect("netsim_rootcanal listener must be registered");
        let bound_port = match rootcanal_addr {
            StreamAddress::Tcp(addr) => addr.port(),
            _ => panic!("Expected TCP address"),
        };
        assert_ne!(bound_port, 0);
        assert_eq!(ini_data.get("test.port"), Some(&bound_port.to_string()));

        tasks.abort();
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn test_cuttlefish_init_transport_listeners_ignores_empty_and_malformed_json() {
        let mut streams = Streams::new();
        let args_empty = Args { fd_startup_str: Some(String::new()), ..Default::default() };
        init_transport_listeners(&args_empty, &mut streams).await;
        assert!(streams.listener_address("netsim_dualfd").is_none());

        let args_malformed =
            Args { fd_startup_str: Some("invalid json".to_string()), ..Default::default() };
        init_transport_listeners(&args_malformed, &mut streams).await;
        assert!(streams.listener_address("netsim_dualfd").is_none());
    }

    #[test]
    fn test_resolve_connector_target() {
        assert_eq!(
            resolve_connector_target(
                Some("localhost:7681".to_string()),
                Some("localhost:5678".to_string()),
            ),
            Some(ConnectorTarget::WebSocket("localhost:7681".to_string()))
        );
        assert_eq!(
            resolve_connector_target(None, Some("localhost:5678".to_string())),
            Some(ConnectorTarget::Tcp("localhost:5678".to_string()))
        );
        assert_eq!(resolve_connector_target(None, None), None);
    }

    #[tokio::test]
    async fn test_run_chip_bridge_websocket_loopback() {
        let listener = bind_tcp_loopback(0).unwrap();
        let ws_addr = listener.local_addr().unwrap().to_string();
        let (_runner, device_client) = device_actor::new();
        tokio::spawn(websocket_server::server::run(listener, device_client));

        let (guest_in_tx, guest_in_rx) = mpsc::channel(4);
        let (guest_out_tx, mut guest_out_rx) = mpsc::channel(4);
        let stream: PacketStream =
            Box::pin(futures::stream::unfold(guest_in_rx, |mut rx| async move {
                rx.recv().await.map(|item| (Ok(item), rx))
            }));
        let sink: PacketSink =
            Box::pin(futures::sink::unfold(guest_out_tx, |tx, item: Bytes| async move {
                tx.send(item)
                    .await
                    .map_err(|e| packet_stream::PacketStreamError::Io(io::Error::other(e)))?;
                Ok(tx)
            }));

        let mut chip_info = ChipInfo::new("cvd-1", ChipKind::BLUETOOTH);
        if let Some(chip) = &mut chip_info.chip {
            chip.loopback = true;
        }

        let bridge = tokio::spawn(run_chip_bridge(
            stream,
            sink,
            chip_info,
            "dualfd-cvd-1".to_string(),
            ConnectorTarget::WebSocket(ws_addr),
        ));

        let payload = Bytes::from_static(&[0x01, 0x03, 0x0c, 0x00]);
        guest_in_tx.send(payload.clone()).await.unwrap();
        assert_eq!(guest_out_rx.recv().await.unwrap(), payload);

        drop(guest_in_tx);
        bridge.await.unwrap();
    }

    #[tokio::test]
    async fn test_cuttlefish_run_connector_mode_if_requested_missing_fd_startup_str() {
        let args = Args { connector_instance: Some(1), logtostderr: true, ..Default::default() };
        let result = run_connector_mode_if_requested(&args).await;
        assert!(matches!(result, Some(RunResult::InitializationError(_))));
    }
}
