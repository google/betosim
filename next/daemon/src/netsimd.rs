// Copyright 2023-2025 The Android Open Source Project
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
    env, io,
    path::PathBuf,
    sync::{Arc, OnceLock, atomic::AtomicU32},
    time::Duration,
};

use capture_actor::CaptureClient;
use common::{
    system::netsimd_temp_dir,
    util::{
        net::bind_tcp_loopback,
        os_utils::{
            get_discovery_directory, get_hci_port, get_instance, get_instance_name,
            redirect_std_stream,
        },
        zip_artifact::{remove_old_artifacts_in_dir, zip_artifacts_in_dir},
    },
};
use device_actor::DeviceClient;
use device_api::{DeviceAddChip, DeviceConfig};
use futures::{FutureExt, SinkExt, StreamExt, pin_mut};
use grpc_server::PacketStreamerService;
use link_actor::LinkClient;
use netsim_model::{
    BluetoothMode, ChipClient, ChipInfo, ChipKind, DeviceParams, PacketSink as ApiPacketSink,
    PacketStream as ApiPacketStream, Pose, set_if_some,
};
use packet_stream::{
    StreamAddress, Streams,
    transport::traits::{PacketSink, PacketStream},
};
use slirp_actor::SlirpClient;
use tokio::{
    signal::{self},
    sync::mpsc,
    task::JoinSet,
};
use tracing::{debug, error, info, warn};
#[cfg(feature = "cuttlefish")]
use {
    packet_stream::{
        ChipInfo as PsChipInfo, DualFdConfig, DualFdListener, InitInfo as PsInitInfo,
        transport::traits::TransportListener,
    },
    tokio::{net::TcpStream, select},
    tokio_util::codec::{Framed, LengthDelimitedCodec},
};

use crate::{
    args::Args,
    ini_file::{
        FileIdentity, IniFile, IniFileAccess, IniFileInitialized, IniFileUninitialized,
        NetsimConfig,
    },
    logger,
    version::{get_version, strip_version_suffix},
};

const MAX_INIT_RETRIES: i32 = 100;
const RETRY_DELAY: Duration = Duration::from_millis(100);
/// Number of gRPC connection attempts before treating a lock owner as unresponsive.
const LIVENESS_PROBE_ATTEMPTS: u32 = 2;
/// Per-attempt deadline for [`is_netsimd_alive`] gRPC connection probes.
const LIVENESS_PROBE_TIMEOUT: Duration = Duration::from_millis(400);
/// Maximum time to wait for `DeviceActor` and secondary actors to shut down.
const ACTOR_TEARDOWN_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum time to wait for session artifact zipping while `netsim.ini.lock` is held.
const ARTIFACT_ZIP_TIMEOUT: Duration = Duration::from_secs(3);
/// Hard deadline for daemon teardown before forcing process exit to release `netsim.ini.lock`.
const TEARDOWN_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, PartialEq)]
pub enum RunResult {
    ExitedNormally,
    InitializationError(String),
}

fn init_error<E: std::fmt::Display>(e: E) -> RunResult {
    RunResult::InitializationError(e.to_string())
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum StartUpMode {
    Owner(NetsimDaemon, IniFileInitialized),
    Client(NetsimConfig),
}

/// Echoes every packet back to its sender (`startup.Chip.loopback`).
///
/// No device or chip is created, so transport tests and benchmarks measure
/// only the packet-stream path: transport framing plus stream plumbing.
async fn run_loopback(stream: PacketStream, sink: PacketSink, device_name: &str) {
    info!("Loopback packet stream for {device_name}");
    if let Err(e) = stream.forward(sink).await {
        warn!("Loopback packet stream for {device_name} failed: {e}");
    }
    info!("Loopback packet stream for {device_name} closed");
}

async fn handle_new_connection(
    device_client: DeviceClient,
    _capture_client: CaptureClient,
    _next_chip_id: Arc<AtomicU32>,
    stream: PacketStream,
    sink: PacketSink,
    chip_info: ChipInfo,
    device_guid: String,
) {
    let device_name = chip_info.device_name();
    info!("Handling new connection for {chip_info}");
    let device_config = DeviceConfig {
        name: device_name.clone(),
        visible: true,
        pose: Pose { position: Default::default(), orientation: Default::default() },
        builtin: false,
        device_info: chip_info.device_info.clone().map(Into::into),
    };

    let mut chip = match chip_info.chip {
        Some(chip) => chip,
        None => {
            warn!("ChipInfo missing 'chip' field for {}. Dropping connection.", chip_info.name);
            return;
        }
    };

    if chip.loopback {
        run_loopback(stream, sink, &device_name).await;
        return;
    }

    if chip.kind == ChipKind::BLUETOOTH
        && let Some(device_info) = chip_info.device_info.as_ref().filter(|d| !d.avd_path.is_empty())
    {
        chip.address =
            crate::avd_config::resolve_bluetooth_mac(&device_info.avd_path, &chip.address).await;
    }

    if chip.address.is_empty() && chip.id.len() == 17 {
        chip.address = chip.id.clone();
    }

    let chip_variant = match chip.kind {
        ChipKind::BLUETOOTH => {
            Some(netsim_model::ChipVariant::Bluetooth(Box::new(netsim_model::Bluetooth {
                address: chip.address.clone(),
                bt_properties: Default::default(),
                mode: BluetoothMode::Device(DeviceParams {}),
                ..Default::default()
            })))
        }
        ChipKind::UWB => Some(netsim_model::ChipVariant::Uwb(netsim_model::Uwb::default())),
        ChipKind::WIFI => {
            Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi { radio: Default::default() }))
        }
        ChipKind::CELLULAR => {
            let is_cuttlefish =
                chip_info.device_info.as_ref().is_some_and(|d| d.kind == "CUTTLEFISH");
            let is_emulator = chip_info.device_info.as_ref().is_some_and(|d| d.kind == "EMULATOR");
            let sdk_version = chip_info
                .device_info
                .as_ref()
                .and_then(|d| d.sdk_version.parse::<i32>().ok())
                .unwrap_or(0);

            let network_configs = if is_cuttlefish {
                crate::cuttlefish::resolve_cuttlefish_network_config(&device_name)
                    .await
                    .into_iter()
                    .collect()
            } else {
                Vec::new()
            };

            let goldfish_ril_37_or_earlier = is_emulator && sdk_version < 38;
            // Emulators rely on automatic NITZ time zone reporting from the modem
            // without requiring explicit AT+CTZV activation.
            let auto_ctzv = is_cuttlefish || is_emulator;
            // Cuttlefish host TAP and Goldfish SLIRP only provision IPv4 cellular routing;
            // fall back to IPv4 when an IPv6 PDP context is requested on emulators.
            let allow_ipv4_for_ipv6 = is_cuttlefish || is_emulator;

            Some(netsim_model::ChipVariant::Cell(netsim_model::Cell {
                sim_type: chip.sim_type,
                sim_profile: chip.sim_profile.clone(),
                quirks: netsim_model::Quirks {
                    goldfish_ril_37_or_earlier,
                    is_cuttlefish,
                    auto_ctzv,
                    allow_ipv4_for_ipv6,
                    dynamic_slirp_lease: is_emulator,
                },
                network_configs,
                ..Default::default()
            }))
        }
        ChipKind::NFC => Some(netsim_model::ChipVariant::Nfc(netsim_model::Nfc::default())),
        ChipKind::ETHERNET | ChipKind::CELLULAR_DATA => None,
        kind => {
            error!("Unsupported chip kind: {kind}");
            return;
        }
    };

    let chip_instance = netsim_model::Chip {
        id: 0,
        kind: chip.kind,
        name: chip.name.clone(),
        manufacturer: chip.manufacturer.clone(),
        product_name: chip.product_name.clone(),
        variant: chip_variant,
        ..Default::default()
    };

    // Convert packet_stream types to netsim_model types
    let api_stream: ApiPacketStream = Box::new(stream.filter_map(|item| {
        Box::pin(async move {
            match item {
                Ok(bytes) => Some(bytes),
                Err(e) => {
                    error!("Error in packet stream: {e}");
                    None
                }
            }
        })
    }));

    let api_sink: ApiPacketSink = Box::pin(sink.sink_map_err(io::Error::other));

    let request = DeviceAddChip {
        device_guid,
        packet_stream: Some(api_stream),
        packet_sink: Some(api_sink),
        device_config,
        chip: chip_instance,
    };

    if let Err(e) = device_client.add_chip(request).await {
        error!("Failed to register stream for {}: {e}", chip_info.name);
    }
}

fn get_env_port_impl<F>(var_name: &str, get_var: F) -> Option<u16>
where
    F: Fn(&str) -> Result<String, std::env::VarError>,
{
    get_var(var_name).ok().and_then(|val| {
        let trimmed = val.trim();
        match trimmed.parse::<u16>() {
            Ok(port) => Some(port),
            Err(e) => {
                warn!("Environment variable {var_name} value '{val}' could not be parsed as a port: {e}");
                None
            }
        }
    })
}
/// Resolves a port number, prioritizing the command line argument (`arg_port`)
/// over the environment variable (`env_var`).
///
/// Returns `None` if neither the argument is provided nor the environment
/// variable is set/valid.
fn resolve_port_with_env<F>(arg_port: Option<u16>, env_var: &str, get_var: F) -> Option<u16>
where
    F: Fn(&str) -> Result<String, std::env::VarError>,
{
    arg_port.or_else(|| get_env_port_impl(env_var, get_var))
}

/// Checks whether the netsimd gRPC server on `grpc_port` is responsive,
/// matching legacy `rust_main.rs::is_netsimd_alive`.
async fn is_netsimd_alive(grpc_port: u16) -> bool {
    static PROBE_ENV: OnceLock<Arc<grpcio::Environment>> = OnceLock::new();
    if grpc_port == 0 {
        return false;
    }
    let addr = format!("localhost:{grpc_port}");
    let env = PROBE_ENV.get_or_init(|| Arc::new(grpcio::Environment::new(1))).clone();
    for attempt in 1..=LIVENESS_PROBE_ATTEMPTS {
        let channel = grpcio::ChannelBuilder::new(env.clone()).connect(&addr);
        let deadline = grpcio::Deadline::from(LIVENESS_PROBE_TIMEOUT);
        if channel.wait_for_connected(deadline).await {
            return true;
        }
        debug!("is_netsimd_alive attempt {attempt}/{LIVENESS_PROBE_ATTEMPTS} failed for {addr}");
    }
    false
}

/// Runs `zip_fn` on Tokio's blocking pool and waits at most `timeout_duration`
/// for it. Returns `true` when `zip_fn` returned before the deadline (a zip
/// error is logged but still counts, because the task is finished) and `false`
/// when it timed out or panicked. On timeout the blocking task is detached via
/// `runtime.shutdown_background()` as `run()` exits, so callers must not assume
/// the artifact directory is quiescent afterwards.
async fn run_zip_with_timeout<F, E>(timeout_duration: Duration, zip_fn: F) -> bool
where
    F: FnOnce() -> Result<(), E> + Send + 'static,
    E: std::fmt::Debug + Send + 'static,
{
    let Ok(join_result) =
        tokio::time::timeout(timeout_duration, tokio::task::spawn_blocking(zip_fn)).await
    else {
        warn!("Artifact zip exceeded {timeout_duration:?}; leaving artifacts unzipped");
        return false;
    };
    match join_result {
        Ok(Ok(())) => true,
        Ok(Err(err)) => {
            error!("Failed to zip artifacts: {err:?}");
            true
        }
        Err(err) => {
            error!("Artifact zip task panicked: {err}");
            false
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn setup_grpc_listener(
    streams: &mut Streams,
    listener_addresses: &mut HashMap<String, StreamAddress>,
    requested_port: u16,
    #[cfg(unix)] grpc_uds_path: Option<String>,
    enable_cli_ui: bool,
    device_client: DeviceClient,
    link_client: LinkClient,
    ap_client: ap_actor::ApClient,
    cell_client: cell_actor::CellClient,
    nfc_client: nfc_actor::NfcClient,
    wifi_client: wifi_actor::WifiClient,
    capture_client: CaptureClient,
    version: String,
    frontend_stats: Arc<netsim_model::FrontendStats>,
) -> Result<(u16, grpcio::Server), RunResult> {
    // Create a channel to bridge PacketStreamerService connections to Streams
    let (new_connection_tx, new_connection_rx) = mpsc::channel(100);
    let packet_streamer_service = PacketStreamerService::new(new_connection_tx);

    // Start the gRPC server
    let (server, port) = grpc_server::start(
        "localhost",
        requested_port.into(),
        #[cfg(unix)]
        grpc_uds_path.clone(),
        enable_cli_ui,
        device_client,
        link_client,
        ap_client,
        cell_client,
        nfc_client,
        wifi_client,
        capture_client,
        packet_streamer_service,
        version,
        frontend_stats,
    )
    .map_err(|e| init_error(format!("Failed to start gRPC server: {}", e)))?;

    let listener = grpc_server::ChannelTransportListener {
        rx: new_connection_rx,
        local_addr: StreamAddress::Grpc(std::net::SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
            port,
        )),
    };

    let _ = streams.add_listener("netsim_grpc", Box::new(listener));

    info!("gRPC port: {}", port);
    listener_addresses.insert(
        "netsim_grpc".to_string(),
        StreamAddress::Grpc(std::net::SocketAddr::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
            port,
        )),
    );
    #[cfg(unix)]
    if let Some(ref uds_path) = grpc_uds_path {
        listener_addresses.insert(
            "netsim_uds".to_string(),
            StreamAddress::Uds(std::path::PathBuf::from(uds_path)),
        );
    }

    Ok((port, server))
}

/// The main daemon for netsim.
///
/// This struct manages the lifecycle of various servers (Bluetooth, Device),
/// and handles incoming connections using the `packet_stream` crate.
#[derive(Debug)]
pub struct NetsimDaemon {
    /// Set of spawned Tokio tasks, including the chip and device servers.
    join_set: JoinSet<()>,
    /// Manages the network listeners (UDS, gRPC) for incoming connections.
    streams: Streams,
    /// Path to the discovery INI file.
    ini_path: PathBuf,
    /// The platform file identity of the `.lock` file held by this daemon.
    ini_lock_inode: Option<FileIdentity>,
    /// Client for interacting with the Device Service.
    device_client: DeviceClient,
    /// Client for interacting with the Capture Service.
    capture_client: CaptureClient,
    /// Shared counter for generating ChipIds.
    next_chip_id: Arc<AtomicU32>,
    /// Addresses of the active listeners.
    listener_addresses: HashMap<String, StreamAddress>,
    /// Command line arguments passed to the daemon.
    args: Args,
    /// The gRPC server instance (kept alive).
    _grpc_server: Option<grpcio::Server>,
    /// The port the HTTP /v1 API is listening on.
    http_port: Option<u16>,
    /// The HTTP listener task, aborted when the daemon is dropped so the HTTP
    /// port does not outlive it.
    http_task: tokio::task::JoinHandle<()>,
    /// The DeviceActor task handle.
    device_task: tokio::task::JoinHandle<()>,
    /// The CaptureActor task handle.
    capture_task: tokio::task::JoinHandle<()>,
    /// Directory for session artifacts (pcaps, stats, zip archives).
    artifact_dir: PathBuf,
    link_client: Box<dyn link_api::LinkClient>,

    slirp_client: SlirpClient,
    chip_clients: HashMap<ChipKind, Box<dyn ChipClient>>,
}

impl Drop for NetsimDaemon {
    fn drop(&mut self) {
        // The HTTP listener task owns its TCP listener. Aborting it here
        // releases the port, so restarting the daemon in the same process does
        // not fail with "Address already in use".
        self.http_task.abort();
        self.device_task.abort();
        self.capture_task.abort();
    }
}

impl NetsimDaemon {
    /// Returns a reference to the CaptureClient.
    pub fn capture_client(&self) -> &CaptureClient {
        &self.capture_client
    }

    /// Returns a clone of the DeviceClient.
    pub fn device_client(&self) -> DeviceClient {
        self.device_client.clone()
    }

    /// Creates a new `NetsimDaemon` instance or returns config for forwarding.
    ///
    /// Returns:
    /// - `Ok(StartUpMode::Owner)`: Daemon instance, lock acquired.
    /// - `Ok(StartUpMode::Client)`: Config of running daemon, lock not
    ///   acquired.
    /// - `Err(RunResult::InitializationError)`: Fatal error.
    #[allow(clippy::new_ret_no_self)]
    pub async fn new() -> Result<StartUpMode, RunResult> {
        let discovery_dir = get_discovery_directory();
        Self::new_with_dirs(discovery_dir, Args::parse()).await
    }

    /// Creates a new `NetsimDaemon` instance with custom directories.
    pub async fn new_with_dirs(
        discovery_dir: PathBuf,
        mut args: Args,
    ) -> Result<StartUpMode, RunResult> {
        args.sanitize();
        if args.version {
            #[allow(clippy::print_stdout)]
            {
                println!("Netsim version: {}", get_version());
            }
            return Err(RunResult::ExitedNormally);
        }

        logger::init("netsim", args.verbose);

        #[cfg(feature = "cuttlefish")]
        if let Some(connector_instance) = args.connector_instance {
            Self::redirect_daemon_logs(&args, Some(connector_instance));
            info!("netsim startup (Connector mode)");
            return match Self::run_netsimd_connector(args, connector_instance).await {
                Ok(()) => Err(RunResult::ExitedNormally),
                Err(err) => Err(RunResult::InitializationError(err)),
            };
        }

        info!("netsim startup");

        // enable Rust backtrace by setting env RUST_BACKTRACE=full
        // SAFETY: Single-threaded initialization code. Caller must guarantee
        // this.
        unsafe {
            env::set_var("RUST_BACKTRACE", "full");
        }

        // Log where netsim artifacts are located
        info!("Artifacts: {:?}", netsimd_temp_dir());
        // Log all args
        info!("{args:#?}");

        // Resolve TAP configuration early to validate permissions/availability.
        #[cfg(all(target_os = "linux", not(feature = "cuttlefish")))]
        let wifi_tap = args.wifi.wifi_tap.clone().or_else(|| {
            if args.wifi.wifi_cvd_tap { Some("cvd-etap-%02d".to_string()) } else { None }
        });

        // Pre-check TAP permissions if configured.
        // We do this BEFORE redirection so the user can see the error in the
        // console.
        #[cfg(all(target_os = "linux", not(feature = "cuttlefish")))]
        if let Some(ref tap_config) = wifi_tap
            && let Err(e) = wifi_actor::TapGateway::preflight_check(tap_config)
        {
            return Err(RunResult::InitializationError(format!("TAP configuration failed: {}", e)));
        }

        info!(
            "Netsim Version: {}, OS: {}, Arch: {}",
            get_version(),
            std::env::consts::OS,
            std::env::consts::ARCH
        );

        let mut attempts = 0;
        let mut stale_eviction_attempted = false;
        // Support Cuttlefish multi-instance by using instance-specific INI
        // files.
        let instance_num = get_instance(args.instance);

        loop {
            let ini_file =
                IniFile::new_for_dir(discovery_dir.clone(), instance_num).map_err(init_error)?;

            // Attempt to acquire the singleton lock for the netsim daemon.
            // The lock is managed by direct file locking on the netsim.ini
            // file.
            match ini_file.try_acquire().map_err(init_error)? {
                // This instance is the Writer (the primary daemon).
                IniFileAccess::Writer(uninitialized_guard) => {
                    return Self::initialize_primary_daemon(uninitialized_guard, args).await;
                }
                // This instance is a Reader, another daemon is already running.
                IniFileAccess::Reader(config) => {
                    if is_netsimd_alive(config.grpc_port).await {
                        info!("Lock held by another process. Reading config: {:?}", config);
                        return Ok(StartUpMode::Client(config));
                    }
                    if stale_eviction_attempted {
                        return Err(init_error(format!(
                            "Existing netsimd (pid {:?}) on port {} is unresponsive and stale lock could not be evicted",
                            config.pid, config.grpc_port
                        )));
                    }
                    warn!(
                        "Existing netsimd (pid {:?}) on port {} is unresponsive; removing stale netsim.ini and starting new daemon",
                        config.pid, config.grpc_port
                    );
                    if IniFile::evict_stale_lock(&discovery_dir, instance_num, &config)
                        .map_err(init_error)?
                    {
                        stale_eviction_attempted = true;
                        continue;
                    }
                    // Nothing was evicted: another process holds
                    // `netsim.ini.init.lock` (evicting the same owner,
                    // initializing as the new owner, or briefly inspecting
                    // the discovery files) or the files changed since
                    // `try_acquire`. Re-observe after the `Initializing`
                    // back-off instead of spending the single eviction
                    // attempt on a no-op.
                    info!("Stale lock eviction skipped; re-checking discovery files");
                }
                // Another process holds `netsim.ini.init.lock` or has not
                // published `netsim.ini` yet.
                IniFileAccess::Initializing => {}
            }

            if attempts >= MAX_INIT_RETRIES {
                return Err(init_error("Timed out waiting for daemon to initialize"));
            }
            attempts += 1;
            tokio::time::sleep(RETRY_DELAY).await;
        }
    }

    fn redirect_daemon_logs(args: &Args, connector_instance: Option<u16>) {
        if !args.logtostderr {
            if let Err(err) =
                redirect_std_stream(&get_instance_name(args.instance, connector_instance))
            {
                error!("{err:?}");
            }

            // Duplicating the previous two logs to be included in
            // netsim_stderr.log
            info!("netsim artifacts path: {:?}", netsimd_temp_dir());
            info!("{args:#?}");
        }
    }

    async fn initialize_primary_daemon(
        ini_guard: IniFileUninitialized,
        args: Args,
    ) -> Result<StartUpMode, RunResult> {
        // Redirect stdout/stderr ONLY after acquiring `netsim.ini.lock` so a
        // newly spawned instance waiting on `.lock` does not truncate the
        // active/draining primary daemon's logs.
        Self::redirect_daemon_logs(&args, None);
        info!("Acquired lock (Owner)");
        info!("INI file path: {}", ini_guard.path().display());

        let artifact_dir = if cfg!(feature = "testing") {
            ini_guard.path().parent().map(|p| p.to_path_buf()).unwrap_or_else(netsimd_temp_dir)
        } else {
            netsimd_temp_dir()
        };

        // Gets rid of old artifacts (pcap and zip files)
        remove_old_artifacts_in_dir(&artifact_dir);

        // Initialize listeners (UDS, gRPC).
        let mut listener_addresses = HashMap::new();
        let mut streams = Streams::new();

        #[cfg(all(target_os = "linux", feature = "cuttlefish"))]
        if let Some(fd_startup_str) = &args.fd_startup_str {
            Self::init_dualfd_listener(fd_startup_str, &mut streams).await;
        }

        // Setup Link Server
        let (link_runner, link_client) = link_actor::new();

        // Setup Device Server Channel
        let (device_runner, device_client) = device_actor::new();

        // Setup Slirp Actor
        let slirp_backend = if args.no_slirp_native {
            slirp_actor::SlirpBackend::CFfi
        } else if args.slirp_native {
            slirp_actor::SlirpBackend::Native
        } else {
            Default::default()
        };
        let (slirp_runner, slirp_client) = slirp_actor::new();
        let slirp_actor_state = slirp_actor::SlirpActor::new_with_backend(
            Default::default(),
            args.http_proxy.clone(),
            args.host_dns.clone(),
            slirp_backend,
        )
        .await;

        // Setup Cell Server
        let (cell_runner, cell_client) = cell_actor::new();
        let cell_actor_state =
            cell_actor::CellActor::new(device_client.clone(), slirp_client.clone());

        // Setup Capture Server
        let (capture_runner, capture_client) = capture_actor::new();

        // Coordinate IDs across all actors
        let next_chip_id = Arc::new(AtomicU32::new(0));

        let frontend_stats = Arc::new(netsim_model::FrontendStats::default());

        let shared_keys = Arc::new(ap_actor::SharedKeyStore::new());
        let (ap_runner, ap_client) = ap_actor::new();
        let ap_actor_state = ap_actor::ApActor::new(shared_keys.clone());

        let (wifi_runner, wifi_client, wifi_actor_state, eth_runner, eth_client, eth_actor_state) = {
            // Setup Wifi Actor
            let (wifi_runner, wifi_client) = wifi_actor::new();
            // Initialize wifi_tap configuration.
            // If --wifi-cvd-tap is set, it implies explicit "cvd-etap-%02d"
            // pattern for pooling. If --wifi-tap is set, it
            // overrides everything.
            let wifi_tap = {
                #[cfg(target_os = "linux")]
                {
                    args.wifi.wifi_tap.clone().or_else(|| {
                        if args.wifi.wifi_cvd_tap {
                            Some("cvd-etap-%02d".to_string())
                        } else {
                            None
                        }
                    })
                }
                #[cfg(not(target_os = "linux"))]
                {
                    None
                }
            };

            // TAP preflight check is now done in `new_with_dirs` before lock
            // acquisition.

            let wifi_actor_state = wifi_actor::WifiActor::new(
                Some(Arc::new(ap_client.clone())),
                Some(slirp_client.clone()),
                device_client.clone(),
                wifi_tap,
                shared_keys,
                Arc::new(wifi_actor::SystemClock),
                args.forward_host_mdns,
            );

            // Setup Ethernet Actor
            let (eth_runner, eth_client) = ethernet_actor::new();
            let eth_actor_state =
                ethernet_actor::EthernetActor::new(slirp_client.clone(), device_client.clone());

            (wifi_runner, wifi_client, wifi_actor_state, eth_runner, eth_client, eth_actor_state)
        };

        // Setup NFC Server
        let (nfc_runner, nfc_client) = nfc_actor::new();
        let mut nfc_actor_state =
            nfc_actor::NfcActor::new(device_client.clone(), nfc_client.stats.clone());
        nfc_actor_state.start_casimir();

        // gRPC port is determined after the listener starts.
        let resolved_grpc_port =
            resolve_port_with_env(args.grpc_port, "NETSIM_GRPC_PORT", |name| std::env::var(name))
                .unwrap_or(0);
        let (actual_grpc_port, grpc_server) = setup_grpc_listener(
            &mut streams,
            &mut listener_addresses,
            resolved_grpc_port,
            #[cfg(unix)]
            args.grpc_uds_path.clone(),
            !args.no_cli_ui,
            device_client.clone(),
            link_client.clone(),
            ap_client.clone(),
            cell_client.clone(),
            nfc_client.clone(),
            wifi_client.clone(),
            capture_client.clone(),
            get_version(),
            frontend_stats.clone(),
        )?;

        // HTTP server for the /v1 API
        let resolved_http_port =
            resolve_port_with_env(args.http_port, "NETSIM_HTTP_PORT", |name| std::env::var(name))
                .unwrap_or(0);
        let http_listener = bind_tcp_loopback(resolved_http_port).map_err(|e| {
            init_error(format!("Failed to bind requested HTTP port {resolved_http_port}: {e}"))
        })?;
        let actual_http_port = http_listener.local_addr().map_err(init_error)?.port();

        // HCI TCP socket server
        let instance_num = get_instance(args.instance);
        let is_explicit_hci = args.hci_port.is_some() || std::env::var("NETSIM_HCI_PORT").is_ok();
        let resolved_hci_port =
            resolve_port_with_env(args.hci_port, "NETSIM_HCI_PORT", |name| std::env::var(name))
                .unwrap_or_else(|| get_hci_port(0, instance_num - 1) as u16);
        let mut actual_hci_port = None;
        match bind_tcp_loopback(resolved_hci_port) {
            Ok(listener) => {
                actual_hci_port = Some(listener.local_addr().map_err(init_error)?.port());
                tokio::spawn(hci_server::run(listener, device_client.clone()));
            }
            Err(e) => {
                if is_explicit_hci {
                    return Err(init_error(format!(
                        "Failed to bind requested HCI socket server port {resolved_hci_port}: {e}"
                    )));
                }
                warn!("Failed to start HCI socket server: {e}");
            }
        }

        // WebSocket server
        let mut actual_ws_port = None;
        let resolved_ws_port_opt =
            resolve_port_with_env(args.ws_port, "NETSIM_WS_PORT", |name| std::env::var(name));
        let websocket_port = resolved_ws_port_opt.map(|p| p + instance_num - 1);
        if let Some(ws_port) = websocket_port {
            match bind_tcp_loopback(ws_port) {
                Ok(listener) => {
                    actual_ws_port = Some(listener.local_addr().map_err(init_error)?.port());
                    tokio::spawn(websocket_server::server::run(listener, device_client.clone()));
                }
                Err(e) => {
                    return Err(init_error(format!(
                        "Failed to bind requested WebSocket server port {ws_port}: {e}"
                    )));
                }
            }
        }

        // Rootcanal legacy control server
        #[cfg(feature = "cuttlefish")]
        let actual_test_addr = {
            let mut addr = None;
            let is_explicit_test = args.test_port.is_some();
            let test_port = args.test_port.unwrap_or_else(|| 7500 + instance_num - 1);
            match bind_tcp_loopback(test_port) {
                Ok(listener) => {
                    let local_addr = listener.local_addr().map_err(init_error)?;
                    addr = Some(local_addr);
                    // Spawn legacy Rootcanal control server for host-side test
                    // runners (like pts-bot/mmi2grpc).
                    tokio::spawn(rootcanal_server::run(listener, device_client.clone()));
                }
                Err(e) => {
                    if is_explicit_test {
                        return Err(init_error(format!(
                            "Failed to bind requested Rootcanal control server port {test_port}: {e}"
                        )));
                    }
                    error!("Failed to bind Rootcanal control server: {e}");
                }
            }
            addr
        };

        let mut actual_tcp_port = None;
        let resolved_tcp_port_opt =
            resolve_port_with_env(args.tcp_port, "NETSIM_TCP_PORT", |name| std::env::var(name));
        let should_start_tcp =
            if cfg!(feature = "cuttlefish") { true } else { resolved_tcp_port_opt.is_some() };

        if should_start_tcp {
            let tcp_port = resolved_tcp_port_opt.unwrap_or(0);
            if let Err(e) = streams
                .start_listener(
                    "tcp",
                    packet_stream::transport::TransportType::tcp("127.0.0.1", tcp_port),
                )
                .await
            {
                error!("Failed to start TCP forwarder listener: {e}");
            } else if let Some(packet_stream::StreamAddress::Tcp(socket_addr)) =
                streams.listener_address("tcp")
            {
                actual_tcp_port = Some(socket_addr.port());
            }
        }
        // Write the current daemon's information to the INI file.
        // Clients will use this to connect.
        let mut ini_data = HashMap::from([
            ("pid".to_string(), std::process::id().to_string()),
            ("grpc.port".to_string(), actual_grpc_port.to_string()),
        ]);
        if let Some(port) = actual_hci_port {
            ini_data.insert("hci.port".to_string(), port.to_string());
        }
        #[cfg(feature = "cuttlefish")]
        if let Some(addr) = actual_test_addr {
            listener_addresses.insert("netsim_rootcanal".to_string(), StreamAddress::Tcp(addr));
            ini_data.insert("test.port".to_string(), addr.port().to_string());
        }
        if let Some(port) = actual_tcp_port {
            ini_data.insert("tcp.port".to_string(), port.to_string());
        }
        if let Some(ws_port) = actual_ws_port {
            ini_data.insert("ws.port".to_string(), ws_port.to_string());
        }
        ini_data.insert("http.port".to_string(), actual_http_port.to_string());
        if let Some(StreamAddress::Uds(path)) = listener_addresses.get("netsim_uds") {
            ini_data.insert("uds.path".to_string(), path.to_string_lossy().to_string());
        }

        // Setup Bluetooth Server
        let (bt_runner, bt_client) = bluetooth_actor::new();
        let bt_actor_state =
            bluetooth_actor::BluetoothActor::new(device_client.clone(), args.disable_address_reuse);

        // Setup Uwb Server
        let (uwb_runner, uwb_client) = uwb_actor::new();
        let uwb_actor = uwb_actor::UwbActor::new(device_client.clone());

        // Prepare chip clients map for DeviceServer
        let mut chip_clients: HashMap<ChipKind, Box<dyn ChipClient>> = HashMap::new();
        chip_clients.insert(ChipKind::BLUETOOTH, Box::new(bt_client.clone()));
        chip_clients.insert(ChipKind::WIFI, Box::new(wifi_client.clone()));
        chip_clients.insert(ChipKind::ETHERNET, Box::new(eth_client.clone()));
        chip_clients.insert(ChipKind::CELLULAR_DATA, Box::new(eth_client.clone()));
        chip_clients.insert(ChipKind::UWB, Box::new(uwb_client.clone()));
        chip_clients.insert(ChipKind::CELLULAR, Box::new(cell_client.clone()));
        chip_clients.insert(ChipKind::NFC, Box::new(nfc_client.clone()));
        // Note: ApClient is NOT added to chip_clients as it is now an
        // independent specialist.

        // Setup Link Actor State
        // Create a new map for LinkActor.
        // We need to inject ChipClients into LinkActor so it can propagate link
        // changes (like RSSI updates) to the underlying radio actors
        // (e.g., BluetoothActor).
        let link_chip_clients = chip_clients.iter().map(|(&k, v)| (k, v.clone())).collect();
        let link_actor_state = link_actor::LinkActor::new(link_chip_clients);

        let (startup_timeout, idle_timeout) = if args.no_shutdown {
            (None, None)
        } else {
            (
                Some(args.startup_timeout.map_or(Duration::from_secs(15), Duration::from_millis)),
                Some(
                    args.idle_shutdown_timeout
                        .map_or(Duration::from_secs(0), Duration::from_millis),
                ),
            )
        };

        let stats_path = if cfg!(feature = "testing") {
            Some(artifact_dir.join("netsim_session_stats.json"))
        } else {
            None
        };

        let mut device_actor_state = device_actor::DeviceActor::new(
            chip_clients.clone(),
            next_chip_id.clone(),
            Some(Arc::new(capture_client.clone())),
            Box::new(link_client.clone()),
            startup_timeout,
            idle_timeout,
            strip_version_suffix(&get_version()).to_owned(),
            stats_path,
            None,
            frontend_stats.clone(),
        );

        // Spawn server tasks
        let mut join_set = JoinSet::new();
        join_set.spawn(bt_runner.run(bt_actor_state));
        join_set.spawn(wifi_runner.run(wifi_actor_state));
        join_set.spawn(eth_runner.run(eth_actor_state));
        join_set.spawn(ap_runner.run(ap_actor_state));
        join_set.spawn(slirp_runner.run(slirp_actor_state));
        join_set.spawn(cell_runner.run(cell_actor_state));
        join_set.spawn(link_runner.run(link_actor_state));
        join_set.spawn(uwb_runner.run(uwb_actor));
        join_set.spawn(nfc_runner.run(nfc_actor_state));

        // Spawn CaptureActor separately so shutdown can deterministically await flush
        let capture_dir =
            if cfg!(feature = "testing") { Some(artifact_dir.join("pcaps")) } else { None };
        let capture_task = tokio::spawn(
            capture_runner.run(capture_actor::CaptureActor::new(args.pcap, capture_dir)),
        );

        // Spawn DeviceActor separately
        device_actor_state.set_self_client(device_client.clone());
        let device_task = tokio::spawn(device_runner.run(device_actor_state));

        // Create Default AP
        {
            let mut ap_config = ap_actor::ApConfig::default();
            if let Some(ssid) = &args.wifi.wifi_ssid {
                ap_config.ssid = ssid.clone();
            }
            set_if_some!(ap_config.wpa_passphrase, args.wifi.wifi_password.clone(), Some);
            set_if_some!(ap_config.channel, args.wifi.wifi_channel);
            set_if_some!(ap_config.beacon_interval, args.wifi.wifi_beacon_interval);
            set_if_some!(ap_config.hw_mode, args.wifi.wifi_mode, Into::into);

            if let Err(e) = ap_client.create_ap(Some(0), ap_config).await {
                warn!("Failed to create default AP: {e}");
            }
        }

        // Create test beacons if required
        let test_beacons = match (args.test_beacons, args.no_test_beacons) {
            (true, false) => true,
            (false, true) => false,
            (false, false) => cfg!(feature = "cuttlefish"),
            (true, true) => {
                return Err(init_error(
                    "unexpected flag combination: cannot set both --test-beacons and --no-test-beacons",
                ));
            }
        };

        if test_beacons {
            crate::test_beacons::create_test_beacons(&device_client).await;
        }

        // Write netsim.ini atomically ONLY after DeviceActor, Default AP, and test beacons
        // are running so external launchers cannot connect before actors are ready.
        let initialized_guard = ini_guard.write(&ini_data).map_err(init_error)?;
        let ini_path = initialized_guard.path().to_path_buf();
        let ini_lock_inode = initialized_guard.lock_inode();
        info!("Wrote to INI file {}", ini_path.display());

        // Clone chip_clients for NetsimDaemon
        let daemon_chip_clients = chip_clients.iter().map(|(k, v)| (*k, v.clone_box())).collect();

        let http_task = tokio::spawn(netsim_rest_api::server::run(
            http_listener,
            device_client.clone(),
            Arc::new(link_client.clone()),
            ap_client.clone(),
            get_version(),
        ));

        Ok(StartUpMode::Owner(
            NetsimDaemon {
                device_client,
                capture_client,
                next_chip_id,
                join_set,
                streams,
                listener_addresses,
                args,
                _grpc_server: Some(grpc_server),
                http_port: Some(actual_http_port),
                http_task,
                device_task,
                capture_task,
                artifact_dir,
                ini_path,
                ini_lock_inode,
                link_client: Box::new(link_client),
                slirp_client,
                chip_clients: daemon_chip_clients,
            },
            initialized_guard,
        ))
    }
    /// Gets the gRPC port, if the server is running.
    pub fn grpc_port(&self) -> Option<u16> {
        self.listener_addresses.get("netsim_grpc").and_then(|addr| match addr {
            StreamAddress::Tcp(socket_addr) | StreamAddress::Grpc(socket_addr) => {
                Some(socket_addr.port())
            }
            _ => None,
        })
    }

    /// Gets the HTTP /v1 API port, if the server is running.
    pub fn http_port(&self) -> Option<u16> {
        self.http_port
    }

    /// Gets the Rootcanal test port, if the server is running.
    #[cfg(feature = "cuttlefish")]
    pub fn test_port(&self) -> Option<u16> {
        self.listener_addresses.get("netsim_rootcanal").and_then(|addr| match addr {
            StreamAddress::Tcp(socket_addr) => Some(socket_addr.port()),
            _ => None,
        })
    }

    async fn shutdown_actors(&mut self, grpc_shutdown: Option<grpcio::ShutdownFuture>) {
        info!("Graceful shutdown requested for all actors");

        let link_fut = self.link_client.shutdown();
        let slirp_fut = async {
            if let Err(e) = self.slirp_client.shutdown().await {
                warn!("SlirpActor shutdown error: {}", e);
            }
        };
        let chips_fut = futures::future::join_all(self.chip_clients.values().map(|c| c.shutdown()));

        // Execute all shutdown dispatches concurrently
        let (link_res, _, chips_res) = tokio::join!(link_fut, slirp_fut, chips_fut);

        if let Err(e) = link_res {
            warn!("LinkActor shutdown error: {}", e);
        }
        for res in chips_res {
            if let Err(e) = res {
                warn!("ChipActor shutdown error: {}", e);
            }
        }
        // Shut down CaptureActor after packet producers have stopped, and await
        // task completion so all buffered PCAP data is flushed and file handles
        // are closed before zipping.
        if !self.capture_task.is_finished() {
            if let Err(e) = self.capture_client.shutdown().await {
                warn!("CaptureActor shutdown error: {}", e);
            }
            let _ = (&mut self.capture_task).await;
        }
        if let Some(fut) = grpc_shutdown {
            let _ = fut.await;
        }
    }

    fn handle_device_actor_completion(&self, result: Result<(), tokio::task::JoinError>) {
        info!("DeviceActor exited. Shutting down daemon.");
        if let Err(e) = result {
            error!("DeviceActor panicked: {}", e);
        }
    }

    fn handle_secondary_task_completion(
        &mut self,
        result: Option<Result<(), tokio::task::JoinError>>,
    ) -> bool {
        match result {
            Some(Ok(_)) => {
                info!("A secondary server task completed. Shutting down.");
                true
            }
            Some(Err(e)) => {
                error!("A secondary server task panicked: {}", e);
                true
            }
            None => {
                // Should not happen as we have multiple tasks
                true
            }
        }
    }

    #[cfg(all(target_os = "linux", feature = "cuttlefish"))]
    async fn init_dualfd_listener(fd_startup_str: &str, streams: &mut Streams) {
        if fd_startup_str.is_empty() {
            return;
        }
        match serde_json::from_str::<DualFdConfig>(fd_startup_str) {
            Ok(config) => match DualFdListener::new(config).await {
                Ok(listener) => {
                    if let Err(e) = streams.add_listener("netsim_dualfd", Box::new(listener)) {
                        error!("Failed to add DualFdListener: {}", e);
                    } else {
                        info!("Added DualFdListener");
                    }
                }
                Err(e) => {
                    error!("Failed to create DualFdListener: {}", e);
                }
            },
            Err(e) => {
                error!("Failed to parse fd_startup_str JSON: {}", e);
            }
        }
    }

    /// Runs the main event loop for the daemon, then tears it down while
    /// `ini_guard` keeps `netsim.ini.lock` held; the guard is released only
    /// after `self` (actors, gRPC server, C++ FFI state) has been dropped.
    pub async fn run_daemon(mut self, ini_guard: IniFileInitialized) -> RunResult {
        info!("Netsimd started {}", if self.args.no_shutdown { "--no-shutdown" } else { "" });

        let shutdown_signal = shutdown_signal();
        pin_mut!(shutdown_signal);

        loop {
            tokio::select! {
                // Branch 1: Handle incoming gRPC/UDS streams (New Clients)
                accept_result = self.streams.accept_any() => {
                    match accept_result {
                        Ok((listener_name, (stream, sink, chip_info, guid))) => {
                            info!("Accepted connection on {}:", listener_name);
                            // Spawning the handler ensures the main loop isn't blocked
                            let device_client = self.device_client.clone();
                            let capture_client = self.capture_client.clone();
                            let next_chip_id = self.next_chip_id.clone();
                            // Spawn connection handling to avoid blocking the main loop
                            // handle_new_connection performs async operations (like device_client.add_chip)
                            // which could delay accepting other connections if awaited directly.
                            tokio::spawn(handle_new_connection(device_client, capture_client, next_chip_id, stream, sink, chip_info, guid));
                        }
                        Err(e) => {
                            error!("Error accepting connection: {}. Stopping new connections.", e);
                        }
                    }
                }

                // Branch 2: Wait for DeviceActor to complete (primary shutdown signal)
                device_result = &mut self.device_task => {
                    self.handle_device_actor_completion(device_result);
                    break;
                }

                // Branch 3: Wait for a task in the JoinSet or CaptureActor to complete (abnormal shutdown)
                join_result = self.join_set.join_next() => {
                    if self.handle_secondary_task_completion(join_result) {
                        break;
                    }
                }
                capture_result = &mut self.capture_task => {
                    if self.handle_secondary_task_completion(Some(capture_result)) {
                        break;
                    }
                }
                http_result = &mut self.http_task => {
                    if self.handle_secondary_task_completion(Some(http_result)) {
                        break;
                    }
                }

                // Branch 4: Graceful shutdown
                () = &mut shutdown_signal => {
                    info!("Shutting down gracefully...");
                    break;
                }
            }
        }
        // Hard watchdog armed with a disarm channel: if actor teardown,
        // zipping, or a C++ FFI destructor (`Drop` of `NetsimDaemon`) hangs
        // past `TEARDOWN_WATCHDOG_TIMEOUT` while `netsim.ini.lock` is still
        // held, force-exit so the OS releases `.lock`. Dropping `watchdog_tx`
        // at the end of `run_daemon` disconnects the channel so the blocking
        // task exits immediately on normal shutdown.
        let (watchdog_tx, watchdog_rx) = std::sync::mpsc::channel::<()>();
        tokio::task::spawn_blocking(move || {
            if matches!(
                watchdog_rx.recv_timeout(TEARDOWN_WATCHDOG_TIMEOUT),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                error!(
                    "Teardown watchdog fired after {TEARDOWN_WATCHDOG_TIMEOUT:?}; exiting without releasing resources gracefully"
                );
                std::process::exit(2);
            }
        });

        IniFileInitialized::unlink_discovery_file_if_owner(&self.ini_path, self.ini_lock_inode);
        let grpc_shutdown = self._grpc_server.as_mut().map(|s| {
            let fut = s.shutdown();
            s.cancel_all_calls();
            fut
        });
        // Shut down DeviceActor sequentially before secondary actors so it can
        // query active chip stats during on_shutdown() without racing against
        // chip actor termination.
        let teardown = async {
            if !self.device_task.is_finished() {
                let _ = self.device_client.shutdown().await;
                let _ = (&mut self.device_task).await;
            }
            self.shutdown_actors(grpc_shutdown).await;
            self.join_set.shutdown().await;
        };
        if tokio::time::timeout(ACTOR_TEARDOWN_TIMEOUT, teardown).await.is_err() {
            warn!(
                "Actor teardown exceeded {ACTOR_TEARDOWN_TIMEOUT:?} timeout; continuing shutdown"
            );
        }
        info!("NetsimDaemon main loop exited.");
        // Zip session artifacts under `ARTIFACT_ZIP_TIMEOUT` while `.lock` is
        // still held so a successor cannot call `remove_old_artifacts_in_dir()`
        // mid-zip.
        let artifact_dir = self.artifact_dir.clone();
        let _ =
            run_zip_with_timeout(ARTIFACT_ZIP_TIMEOUT, move || zip_artifacts_in_dir(&artifact_dir))
                .await;
        // Drop `self` (C++ FFI / gRPC destructors) and then the discovery lock
        // while the teardown watchdog is still armed, then disarm it.
        drop(self);
        drop(ini_guard);
        drop(watchdog_tx);
        RunResult::ExitedNormally
    }
}

/// Listens for process shutdown signals to enable graceful termination.
///
/// - On Unix: listens for `SIGINT` (Ctrl+C) and `SIGTERM`.
/// - On Windows: listens for Ctrl+C and `CTRL_CLOSE_EVENT`.
///
/// If it fails to listen for a signal, that error is logged and ignored.
async fn shutdown_signal() {
    let ctrl_c = signal::ctrl_c().fuse();
    pin_mut!(ctrl_c);

    let mut terminate = {
        #[cfg(unix)]
        {
            match signal::unix::signal(signal::unix::SignalKind::terminate()) {
                Ok(sig) => Some(sig),
                Err(err) => {
                    warn!("Failed to listen for terminate signal: {err}");
                    None
                }
            }
        }
        #[cfg(target_os = "windows")]
        {
            match signal::windows::ctrl_close() {
                Ok(sig) => Some(sig),
                Err(err) => {
                    warn!("Failed to listen for ctrl_close signal: {err}");
                    None
                }
            }
        }
        #[cfg(not(any(unix, target_os = "windows")))]
        {
            None
        }
    };
    let terminate_fut = async {
        if let Some(ref mut sig) = terminate {
            sig.recv().await;
        } else {
            // Signal failed to init or unsupported platform
            futures::future::pending::<Option<()>>().await;
        }
    };
    pin_mut!(terminate_fut);

    loop {
        tokio::select! {
            res = &mut ctrl_c => {
                if let Err(err) = res {
                    warn!("Failed to listen for ctrl+c signal: {err}");
                } else {
                    info!("Ctrl+C received");
                    break;
                }
            }
            _ = &mut terminate_fut => {
                info!("Termination signal received");
                break;
            }
        }
    }
}

pub fn run() -> RunResult {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            return RunResult::InitializationError(format!("Failed to start Tokio runtime: {err}"));
        }
    };
    let result = runtime.block_on(run_async());
    // Detach any wedged worker or timed-out `spawn_blocking` task immediately
    // once `run_async()` returns (`Runtime::drop` would block indefinitely, and
    // waiting after `ini_guard` is dropped would keep `.lock`-less cleanup alive).
    runtime.shutdown_background();
    result
}

async fn run_async() -> RunResult {
    match NetsimDaemon::new().await {
        Ok(StartUpMode::Owner(daemon, ini_guard)) => daemon.run_daemon(ini_guard).await,
        Ok(StartUpMode::Client(config)) => {
            // If we are just a client, we shouldn't necessarily fail, but if
            // the user expected to start a NEW daemon, they might
            // be confused. For now, valid behavior is to print info
            // and exit normally (acting as a client/discovery).
            info!("Another netsimd is running. Will use its config: {:?}", config);
            info!("Target gRPC port: {}", config.grpc_port);
            RunResult::ExitedNormally
        }
        Err(e) => e,
    }
}

#[cfg(feature = "cuttlefish")]
const RETRY_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(feature = "cuttlefish")]
#[derive(Clone, Debug, PartialEq, Eq)]
enum ConnectorTarget {
    WebSocket(String),
    Tcp(String),
}

#[cfg(feature = "cuttlefish")]
impl NetsimDaemon {
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
        S: futures::Stream<Item = io::Result<bytes::Bytes>> + Unpin,
        K: futures::Sink<bytes::Bytes, Error = io::Error> + Unpin,
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
                    Err(BridgeError::Guest(msg)) => {
                        error!("Upstream bridge failed for chip {chip_id} (Guest): {msg}");
                        true
                    }
                    Err(BridgeError::Remote(msg)) => {
                        error!(
                            "Upstream bridge failed for chip {chip_id} ({transport_name}): {msg}"
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
                    Err(BridgeError::Guest(msg)) => {
                        error!("Downstream bridge failed for chip {chip_id} (Guest): {msg}");
                        true
                    }
                    Err(BridgeError::Remote(msg)) => {
                        error!(
                            "Downstream bridge failed for chip {chip_id} ({transport_name}): {msg}"
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
                    let path = websocket_server::client::build_packet_stream_path(
                        &chip_info,
                        &device_guid,
                    );
                    match websocket_server::client::connect(server_addr, &path).await {
                        Ok((ws_stream, ws_sink)) => {
                            info!(
                                "Connected WebSocket forwarder to primary daemon for chip {chip_id}"
                            );
                            Self::bridge_packets(
                                &mut packet_stream,
                                &mut packet_sink,
                                ws_stream,
                                ws_sink,
                                &chip_id,
                                "WebSocket",
                            )
                            .await
                        }
                        Err(e) => {
                            warn!(
                                "Failed to connect to primary daemon WebSocket {server_addr}{path}: {e}"
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

                        let mut framed_tcp =
                            Framed::new(tcp_stream_raw, LengthDelimitedCodec::new());

                        let init_json = match serde_json::to_vec(&init_info) {
                            Ok(json) => json,
                            Err(e) => {
                                error!("Failed to serialize InitInfo: {e}");
                                break;
                            }
                        };

                        if let Err(e) = framed_tcp.send(bytes::Bytes::from(init_json)).await {
                            error!("Failed to send InitInfo: {e}");
                            false
                        } else {
                            info!("Sent initial ChipInfo to primary daemon for chip {chip_id}");

                            let (tcp_sink, tcp_stream) = framed_tcp.split();
                            let tcp_stream = tcp_stream.map(|res| res.map(bytes::BytesMut::freeze));
                            Self::bridge_packets(
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
                    Err(e) => {
                        warn!("Failed to connect to primary daemon TCP port: {e}");
                        false
                    }
                },
            };

            if is_guest_disconnected {
                info!("Guest radio transport disconnected permanently for chip {chip_id}");
                break;
            }

            info!(
                "Forwarder stream disconnected, re-establishing session in 1s for chip {chip_id}..."
            );
            tokio::time::sleep(RETRY_INTERVAL).await;
        }
        info!("Duplex bridge closed for chip {chip_id}");
    }

    /// Orchestrates the Cuttlefish packet forwarder connector daemon.
    async fn run_netsimd_connector(args: Args, connector_instance: u16) -> Result<(), String> {
        let Some(fd_startup_str) = args.fd_startup_str else {
            return Err("Failed to start netsimd forwarder, missing `-s` arg".to_string());
        };

        let dual_fd_config: DualFdConfig = serde_json::from_str(&fd_startup_str)
            .map_err(|e| format!("Could not parse startup info JSON: {e}"))?;

        let Some(target) = Self::resolve_connector_target(
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
            .await
            .map_err(|e| format!("Failed to create DualFdListener: {e}"))?;

        loop {
            tokio::select! {
                res = listener.accept() => {
                    match res {
                        Ok((packet_stream, packet_sink, chip_info, guid)) => {
                            join_set.spawn(Self::run_chip_bridge(
                                packet_stream,
                                packet_sink,
                                chip_info,
                                guid,
                                target.clone(),
                            ));
                        }
                        Err(e) => {
                            error!("Error accepting connection: {e}");
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_env_port_impl() {
        let mock_env = |name: &str| match name {
            "VALID" => Ok("1234".to_string()),
            "WHITESPACE" => Ok("  5678  ".to_string()),
            "INVALID" => Ok("not_a_port".to_string()),
            "EMPTY" => Ok("".to_string()),
            "OVERFLOW" => Ok("999999".to_string()),
            _ => Err(std::env::VarError::NotPresent),
        };

        assert_eq!(get_env_port_impl("VALID", mock_env), Some(1234));
        assert_eq!(get_env_port_impl("WHITESPACE", mock_env), Some(5678));
        assert_eq!(get_env_port_impl("INVALID", mock_env), None);
        assert_eq!(get_env_port_impl("EMPTY", mock_env), None);
        assert_eq!(get_env_port_impl("OVERFLOW", mock_env), None);
        assert_eq!(get_env_port_impl("NOT_PRESENT", mock_env), None);
    }

    #[test]
    fn test_resolve_port_with_env() {
        let mock_env = |name: &str| match name {
            "NETSIM_PORT" => Ok("1234".to_string()),
            "INVALID_PORT" => Ok("invalid".to_string()),
            _ => Err(std::env::VarError::NotPresent),
        };

        // Case 1: CLI arg is Some, should ignore environment variable
        assert_eq!(resolve_port_with_env(Some(5678), "NETSIM_PORT", mock_env), Some(5678));
        assert_eq!(resolve_port_with_env(Some(5678), "NOT_PRESENT", mock_env), Some(5678));

        // Case 2: CLI arg is None, environment variable is present and valid
        assert_eq!(resolve_port_with_env(None, "NETSIM_PORT", mock_env), Some(1234));

        // Case 3: CLI arg is None, environment variable is invalid/malformed
        assert_eq!(resolve_port_with_env(None, "INVALID_PORT", mock_env), None);

        // Case 4: CLI arg is None, environment variable is not present
        assert_eq!(resolve_port_with_env(None, "NOT_PRESENT", mock_env), None);
    }

    #[tokio::test]
    async fn test_handle_new_connection_missing_chip() {
        let (_runner, device_client) = device_actor::new();
        let (_capture_runner, capture_client) = capture_actor::new();
        let stream: PacketStream = Box::pin(futures::stream::empty());
        let sink: PacketSink =
            Box::pin(futures::sink::drain().sink_map_err(|never| match never {}));
        let chip_info = ChipInfo { name: "test-device".to_string(), chip: None, device_info: None };
        handle_new_connection(
            device_client,
            capture_client,
            Arc::new(AtomicU32::new(1)),
            stream,
            sink,
            chip_info,
            "guid-1".to_string(),
        )
        .await;
    }

    #[tokio::test]
    async fn test_handle_new_connection_unsupported_kind() {
        let (_runner, device_client) = device_actor::new();
        let (_capture_runner, capture_client) = capture_actor::new();
        let stream: PacketStream = Box::pin(futures::stream::empty());
        let sink: PacketSink =
            Box::pin(futures::sink::drain().sink_map_err(|never| match never {}));
        let chip_info = ChipInfo {
            name: "test-device".to_string(),
            chip: Some(packet_stream::Chip::new(ChipKind::UNSPECIFIED, "unsupported")),
            device_info: None,
        };
        handle_new_connection(
            device_client,
            capture_client,
            Arc::new(AtomicU32::new(1)),
            stream,
            sink,
            chip_info,
            "guid-2".to_string(),
        )
        .await;
    }

    #[tokio::test]
    async fn test_handle_new_connection_add_chip_error() {
        let (runner, device_client) = device_actor::new();
        let (_capture_runner, capture_client) = capture_actor::new();
        // Dropping runner ensures device_client.add_chip fails with Send error
        drop(runner);
        let stream: PacketStream = Box::pin(futures::stream::empty());
        let sink: PacketSink =
            Box::pin(futures::sink::drain().sink_map_err(|never| match never {}));
        let chip_info = ChipInfo::new("Pixel 9", ChipKind::BLUETOOTH);
        handle_new_connection(
            device_client,
            capture_client,
            Arc::new(AtomicU32::new(1)),
            stream,
            sink,
            chip_info,
            "guid-3".to_string(),
        )
        .await;
    }

    #[tokio::test]
    async fn test_is_netsimd_alive_rejects_unresponsive_port() {
        assert!(!is_netsimd_alive(0).await);

        // A listener that never completes the HTTP/2 handshake models a
        // wedged netsimd that still holds its port open.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let dead_port = listener.local_addr().unwrap().port();

        let probe_start = std::time::Instant::now();
        assert!(!is_netsimd_alive(dead_port).await);
        // Two 400ms attempts must stay well below the Initializing retry budget.
        assert!(probe_start.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn test_run_zip_with_timeout_bounds_a_stalled_zip() {
        assert!(run_zip_with_timeout(Duration::from_secs(1), || Ok::<(), io::Error>(())).await);
        // A zip error is logged but still counts as finished.
        assert!(
            run_zip_with_timeout(Duration::from_secs(1), || Err(io::Error::other("zip failed")))
                .await
        );
        // A panicking blocking task resolves `JoinError` to `false` without aborting teardown.
        assert!(
            !run_zip_with_timeout(Duration::from_secs(1), || -> Result<(), io::Error> {
                panic!("zip panicked");
            })
            .await
        );

        // A zip task that never returns must not block teardown.
        let (unblock_zip_tx, unblock_zip_rx) = std::sync::mpsc::channel::<()>();
        let start = std::time::Instant::now();
        let finished_in_time = run_zip_with_timeout(Duration::from_millis(100), move || {
            let _ = unblock_zip_rx.recv();
            Ok::<(), io::Error>(())
        })
        .await;
        assert!(!finished_in_time);
        assert!(start.elapsed() < Duration::from_secs(1));
        drop(unblock_zip_tx);
    }

    #[cfg(feature = "cuttlefish")]
    #[test]
    fn test_resolve_connector_target() {
        assert_eq!(
            NetsimDaemon::resolve_connector_target(
                Some("localhost:7681".to_string()),
                Some("localhost:5678".to_string()),
            ),
            Some(ConnectorTarget::WebSocket("localhost:7681".to_string()))
        );
        assert_eq!(
            NetsimDaemon::resolve_connector_target(None, Some("localhost:5678".to_string())),
            Some(ConnectorTarget::Tcp("localhost:5678".to_string()))
        );
        assert_eq!(NetsimDaemon::resolve_connector_target(None, None), None);
    }

    #[cfg(feature = "cuttlefish")]
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
            Box::pin(futures::sink::unfold(guest_out_tx, |tx, item: bytes::Bytes| async move {
                tx.send(item)
                    .await
                    .map_err(|e| packet_stream::PacketStreamError::Io(io::Error::other(e)))?;
                Ok(tx)
            }));

        let mut chip_info = ChipInfo::new("cvd-1", ChipKind::BLUETOOTH);
        if let Some(chip) = &mut chip_info.chip {
            chip.loopback = true;
        }

        let bridge = tokio::spawn(NetsimDaemon::run_chip_bridge(
            stream,
            sink,
            chip_info,
            "dualfd-cvd-1".to_string(),
            ConnectorTarget::WebSocket(ws_addr),
        ));

        let payload = bytes::Bytes::from_static(&[0x01, 0x03, 0x0c, 0x00]);
        guest_in_tx.send(payload.clone()).await.unwrap();
        assert_eq!(guest_out_rx.recv().await.unwrap(), payload);

        drop(guest_in_tx);
        bridge.await.unwrap();
    }
}
