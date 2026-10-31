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

//! BDD Actor World harness for `netsim_rest_api` Tier 2 integration tests.
//!
//! Spins up an ephemeral in-process `netsim_rest_api::server::run` listener on
//! `127.0.0.1:0` backed by real in-memory `DeviceActor`, `ApActor`, and
//! `CaptureActor` instances.

use std::{
    collections::HashMap,
    fmt::Write as _,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, atomic::AtomicU32},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ap_actor::ApClient;
use capture_actor::CaptureActor;
use device_actor::{DeviceActor, DeviceClient};
use netsim_model::{
    ApCreate, BluetoothCreate, BluetoothMode, CellCreate, Chip, ChipClient, ChipCreateVariant,
    ChipError, ChipId, ChipKind, ChipRequest, ChipVariant, Controller, DeviceChipCreate,
    DeviceConfig, DeviceCreate, DeviceId, DeviceParams, FrontendStats, MODEM_STATE_IDLE,
    Orientation, Pose, Position, RadioChipClient, WifiCreate,
};
use netsim_rest_api::{ErrorResponse, NetsimRestClient};
use serde::de::DeserializeOwned;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

use crate::server_test::MockLinkClient;

const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Parsed raw HTTP/1.1 response from the in-process REST server.
#[derive(Debug, Clone)]
pub struct RawHttpResponse {
    pub status_code: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub body_bytes: Vec<u8>,
}

impl RawHttpResponse {
    /// Asserts that `Content-Type: application/json` and
    /// `X-Content-Type-Options: nosniff` headers are present.
    pub fn assert_json_headers(&self) {
        assert_eq!(
            self.headers.get("content-type").map(String::as_str),
            Some("application/json"),
            "Expected Content-Type: application/json in headers: {:?}",
            self.headers
        );
        assert_eq!(
            self.headers.get("x-content-type-options").map(String::as_str),
            Some("nosniff"),
            "Expected X-Content-Type-Options: nosniff in headers: {:?}",
            self.headers
        );
    }

    /// Asserts that `Content-Type: application/vnd.tcpdump.pcap` and
    /// `X-Content-Type-Options: nosniff` headers are present.
    pub fn assert_pcap_headers(&self) {
        assert_eq!(
            self.headers.get("content-type").map(String::as_str),
            Some("application/vnd.tcpdump.pcap"),
            "Expected Content-Type: application/vnd.tcpdump.pcap in headers: {:?}",
            self.headers
        );
        assert_eq!(
            self.headers.get("x-content-type-options").map(String::as_str),
            Some("nosniff"),
            "Expected X-Content-Type-Options: nosniff in headers: {:?}",
            self.headers
        );
    }

    /// Asserts `204 No Content` headers (`X-Content-Type-Options: nosniff`, no
    /// `Content-Type`, and an empty body).
    pub fn assert_no_content_headers(&self) {
        assert!(
            !self.headers.contains_key("content-type"),
            "204 No Content must not include Content-Type: {:?}",
            self.headers
        );
        assert_eq!(
            self.headers.get("x-content-type-options").map(String::as_str),
            Some("nosniff"),
            "Expected X-Content-Type-Options: nosniff in headers: {:?}",
            self.headers
        );
        assert!(self.body.is_empty(), "204 No Content body must be empty, got: {:?}", self.body);
    }

    /// Deserializes the response body into `T`.
    pub fn parse_json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|e| panic!("Failed to deserialize HTTP JSON body {:?}: {e}", self.body))
    }

    /// Deserializes the response body into a dynamic `serde_json::Value`.
    pub fn parse_json_value(&self) -> serde_json::Value {
        self.parse_json()
    }

    /// Deserializes the response body as an [`ErrorResponse`].
    pub fn parse_error(&self) -> ErrorResponse {
        self.parse_json()
    }
}

fn spawn_chip_actor() -> (Box<dyn ChipClient>, tokio::task::JoinHandle<()>) {
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    let task = tokio::spawn(async move {
        let mut chips: HashMap<ChipId, Chip> = HashMap::new();
        let mut initial_chips: HashMap<ChipId, Chip> = HashMap::new();
        while let Some(req) = rx.recv().await {
            match req {
                ChipRequest::Create { id, mut params, respond_to } => {
                    params.chip.id = id.0;
                    params.chip.enabled = true;
                    if let Some(ChipVariant::Cell(cell)) = &mut params.chip.variant {
                        if cell.radio.state.is_none() {
                            cell.radio.state = Some(true);
                        }
                        if cell.state.is_empty() {
                            cell.state = MODEM_STATE_IDLE.to_string();
                        }
                    }
                    initial_chips.insert(id, params.chip.clone());
                    chips.insert(id, params.chip);
                    let _ = respond_to.send(Ok(()));
                }
                ChipRequest::Read { id, respond_to } => {
                    let res = chips.get(&id).cloned().ok_or(ChipError::ChipNotFound(id));
                    let _ = respond_to.send(res);
                }
                ChipRequest::Update { id, patch, respond_to } => {
                    if let Some(chip) = chips.get_mut(&id) {
                        patch.apply(chip);
                        let _ = respond_to.send(Ok(chip.clone()));
                    } else {
                        let _ = respond_to.send(Err(ChipError::ChipNotFound(id)));
                    }
                }
                ChipRequest::Delete { id, respond_to } => {
                    chips.remove(&id);
                    initial_chips.remove(&id);
                    let _ = respond_to.send(Ok(()));
                }
                ChipRequest::Reset { id, respond_to } => {
                    if let Some(initial) = initial_chips.get(&id).cloned() {
                        chips.insert(id, initial.clone());
                        let _ = respond_to.send(Ok(initial));
                    } else {
                        let _ = respond_to.send(Err(ChipError::ChipNotFound(id)));
                    }
                }
                ChipRequest::GetStatistics { respond_to } => {
                    let _ = respond_to.send(Ok(Vec::new().into_boxed_slice()));
                }
                ChipRequest::GetCountForTesting { respond_to } => {
                    let _ = respond_to.send(Ok(chips.len()));
                }
                ChipRequest::Shutdown => break,
            }
        }
    });
    (Box::new(RadioChipClient::new(tx)), task)
}

fn temp_stats_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    let unique = format!(
        "netsim_rest_bdd_stats_{}_{:?}_{}.json",
        std::process::id(),
        std::thread::current().id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
    );
    path.push(unique);
    path
}

/// BDD World running an ephemeral `netsim_rest_api::server::run` listener
/// backed by real `DeviceActor`, `ApActor`, and `CaptureActor` instances.
pub struct World {
    pub addr: SocketAddr,
    pub rest_client: NetsimRestClient,
    pub device_client: DeviceClient,
    stats_path: PathBuf,
    pcap_dir: PathBuf,
    _tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for World {
    fn drop(&mut self) {
        for task in &self._tasks {
            task.abort();
        }
        let _ = std::fs::remove_file(&self.stats_path);
        let mut tmp = self.stats_path.clone();
        if let Some(name) = tmp.file_name() {
            let mut s = name.to_os_string();
            s.push(".tmp");
            tmp.set_file_name(s);
            let _ = std::fs::remove_file(tmp);
        }
        let _ = std::fs::remove_dir_all(&self.pcap_dir);
    }
}

impl World {
    /// Starts the actors, seeds the default AP (`id: 0`), and binds the REST
    /// server on `127.0.0.1:0`.
    pub async fn new() -> Self {
        let mut tasks = Vec::new();
        let mut chip_clients: HashMap<ChipKind, Box<dyn ChipClient>> = HashMap::new();
        for kind in [ChipKind::WIFI, ChipKind::BLUETOOTH, ChipKind::CELLULAR] {
            let (client, task) = spawn_chip_actor();
            chip_clients.insert(kind, client);
            tasks.push(task);
        }

        let stats_path = temp_stats_path();
        let pcap_dir = stats_path.with_extension("pcaps");
        let (capture_runner, capture_client) = capture_actor::new();
        tasks.push(tokio::spawn(
            capture_runner.run(CaptureActor::new(false, Some(pcap_dir.clone()))),
        ));

        let (device_runner, device_client) = device_actor::new();
        let mut device_actor_inst = DeviceActor::new(
            chip_clients,
            Arc::new(AtomicU32::new(1000)),
            Some(Arc::new(capture_client.clone())),
            Box::new(MockLinkClient),
            None,
            None,
            "0.3.92".to_string(),
            Some(stats_path.clone()),
            None,
            Arc::new(FrontendStats::default()),
        );
        device_actor_inst.set_self_client(device_client.clone());
        tasks.push(tokio::spawn(device_runner.run(device_actor_inst)));

        let shared_keys = Arc::new(ap_actor::SharedKeyStore::new());
        let ap_actor_inst = ap_actor::ApActor::new(shared_keys);
        let (ap_runner, ap_client_base) = actor_framework::ResourceActor::new(32);
        let ap_client = ApClient::new(ap_client_base);
        tasks.push(tokio::spawn(ap_runner.run(ap_actor_inst)));
        // Seed the default AP (id: 0) as netsimd does on startup.
        ap_client
            .create_ap(Some(0), ap_actor::ApConfig::default())
            .await
            .expect("Failed to seed default AP 0");

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tasks.push(tokio::spawn(netsim_rest_api::server::run(
            listener,
            device_client.clone(),
            Arc::new(MockLinkClient),
            ap_client.clone(),
            capture_client.clone(),
            "0.3.92".to_string(),
        )));

        let rest_client = NetsimRestClient::from_addr(addr.to_string());
        Self { addr, rest_client, device_client, stats_path, pcap_dir, _tasks: tasks }
    }

    /// BDD Step: Given a dynamic Wi-Fi device at `position`.
    pub async fn given_wifi_device(&self, name: &str, position: Position) -> DeviceId {
        self.device_client
            .create_device(DeviceCreate {
                device_config: DeviceConfig::new(
                    name,
                    true,
                    Pose { position, orientation: Orientation::default() },
                    false,
                ),
                chip: DeviceChipCreate::new(
                    "wifi-chip",
                    "TestMfg",
                    "TestProduct",
                    ChipCreateVariant::Wifi(WifiCreate::default()),
                ),
            })
            .await
            .expect("Failed to create Wi-Fi device")
    }

    /// BDD Step: Given a dynamic Bluetooth device at `position`.
    pub async fn given_bluetooth_device(&self, name: &str, position: Position) -> DeviceId {
        self.device_client
            .create_device(DeviceCreate {
                device_config: DeviceConfig::new(
                    name,
                    true,
                    Pose { position, orientation: Orientation::default() },
                    false,
                ),
                chip: DeviceChipCreate::new(
                    "bt-chip",
                    "TestMfg",
                    "TestProduct",
                    ChipCreateVariant::Bluetooth(BluetoothCreate {
                        address: "02:15:b2:00:00:01".to_string(),
                        bt_properties: Controller::default(),
                        mode: BluetoothMode::Device(DeviceParams::default()),
                    }),
                ),
            })
            .await
            .expect("Failed to create Bluetooth device")
    }

    /// BDD Step: Given a dynamic Cellular device at `position`.
    pub async fn given_cellular_device(&self, name: &str, position: Position) -> DeviceId {
        self.device_client
            .create_device(DeviceCreate {
                device_config: DeviceConfig::new(
                    name,
                    true,
                    Pose { position, orientation: Orientation::default() },
                    false,
                ),
                chip: DeviceChipCreate::new(
                    "cell-chip",
                    "TestMfg",
                    "TestProduct",
                    ChipCreateVariant::Cell(CellCreate::default()),
                ),
            })
            .await
            .expect("Failed to create Cellular device")
    }

    /// BDD Step: Given a dynamic Access Point created via REST.
    pub async fn given_dynamic_ap(&self, ssid: &str) -> u32 {
        self.rest_client
            .create_ap(&ApCreate { ssid: ssid.to_string(), ..Default::default() })
            .send()
            .await
            .expect("Failed to create dynamic AP")
            .id
    }

    /// BDD Step: When a client sends an HTTP/1.1 request with optional
    /// `Content-Type` and `body` over the TCP socket.
    pub async fn when_http_request(
        &self,
        method: &str,
        path: &str,
        content_type: Option<&str>,
        body: &[u8],
    ) -> RawHttpResponse {
        self.when_http_request_with_content_length(method, path, content_type, body.len(), body)
            .await
    }

    /// BDD Step: When a client sends an HTTP/1.1 request with an explicit
    /// `declared_content_length` (useful for testing `413 Payload Too Large`
    /// without transmitting a full multi-megabyte buffer).
    pub async fn when_http_request_with_content_length(
        &self,
        method: &str,
        path: &str,
        content_type: Option<&str>,
        declared_content_length: usize,
        body: &[u8],
    ) -> RawHttpResponse {
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\n", self.addr);
        if let Some(ct) = content_type {
            let _ = write!(head, "Content-Type: {ct}\r\n");
        }
        if declared_content_length > 0 || content_type.is_some() {
            let _ = write!(head, "Content-Length: {declared_content_length}\r\n");
        }
        head.push_str("Connection: close\r\n\r\n");

        let raw_bytes = timeout(IO_TIMEOUT, async {
            let mut stream = TcpStream::connect(self.addr).await.unwrap();
            stream.write_all(head.as_bytes()).await.unwrap();
            if !body.is_empty() {
                // Ignore write error if the server rejects early (e.g. 413 on declared length)
                let _ = stream.write_all(body).await;
            }
            let mut buf = Vec::new();
            if let Err(e) = stream.read_to_end(&mut buf).await {
                assert!(!buf.is_empty(), "Failed to read HTTP response: {e}");
            }
            buf
        })
        .await
        .expect("HTTP request timed out");

        let sep = raw_bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("Missing HTTP header/body separator");
        let head_part = std::str::from_utf8(&raw_bytes[..sep]).expect("Non-UTF8 HTTP headers");
        let body_bytes = raw_bytes[sep + 4..].to_vec();
        let body = String::from_utf8_lossy(&body_bytes).into_owned();
        let mut lines = head_part.lines();
        let status_line = lines.next().unwrap_or_default();
        let status_code = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .expect("Invalid HTTP status line");
        let mut headers = HashMap::new();
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
        }
        RawHttpResponse { status_code, headers, body, body_bytes }
    }
}
