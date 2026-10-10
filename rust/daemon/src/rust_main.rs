// Copyright 2023 The Android Open Source Project
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

use clap::Parser;
use grpcio::{ChannelBuilder, Deadline, EnvBuilder};
use log::{error, info, warn};
use netsim_common::system::netsimd_temp_dir;
use netsim_common::util::ini_file::{create_ini, get_server_address, remove_ini};
use netsim_common::util::os_utils::{
    get_hci_port, get_instance, get_instance_name, redirect_std_stream,
};
use netsim_common::util::zip_artifact::zip_artifacts;
use netsim_proto::frontend_grpc::FrontendServiceClient;

use crate::captures::capture::spawn_capture_event_subscriber;
use crate::config_file;
use crate::devices::devices_handler::{DeviceManager, spawn_shutdown_publisher};
use crate::events::{Event, Events, ShutDown};
use crate::links::link::{ANY_CHIP, LinkManager, PhyKind};
use crate::session::Session;
use crate::version::get_version;
use crate::wireless;

use netsim_common::util::netsim_logger;

use crate::args::NetsimdArgs;
use crate::ffi::ffi_util;
use crate::service::{Service, ServiceParams, new_test_beacon};
use netsim_proto::config::{Bluetooth as BluetoothConfig, Capture, Config};
use std::env;
use std::ffi::{c_char, c_int};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

/// Wireless network simulator for android (and other) emulated devices.
///
/// # Safety
///
/// The file descriptors passed in `NetsimdArgs::fd_startup_str` must remain valid and open for as
/// long as the program runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rust_main(argc: c_int, argv: *const *const c_char) {
    // enable Rust backtrace by setting env RUST_BACKTRACE=full
    // SAFETY: Single-threaded main function.
    // Note: set_var and remove_var have always been unsound when called in
    // a multi-threaded context. They are marked unsafe in edition 2024.
    // See https://doc.rust-lang.org/edition-guide/rust-2024/newly-unsafe-functions.html#stdenvset_var-remove_var
    unsafe { env::set_var("RUST_BACKTRACE", "full") };
    ffi_util::set_up_crash_report();
    let netsimd_args = get_netsimd_args(argc, argv);
    netsim_logger::init("netsimd", netsimd_args.verbose);
    run_netsimd_with_args(netsimd_args);
}

#[allow(unused)]
fn get_netsimd_args(argc: c_int, argv: *const *const c_char) -> NetsimdArgs {
    let env_args_or_err = env::var("NETSIM_ARGS");

    #[cfg(feature = "cuttlefish")]
    {
        // TODO: Use NetsimdArgs::parse() after netsimd binary is built with netsimd.rs.
        // In linux arm64 in aosp-main, it can't access CLI arguments by std::env::args() with netsimd.cc wrapper.
        let mut argv: Vec<_> = (0..argc)
            .map(|i|
                // SAFETY: argc and argv will remain valid as long as the program runs.
                unsafe {
                    std::ffi::CStr::from_ptr(*argv.add(i as usize)).to_str().unwrap().to_owned()
                })
            .collect();
        if let Ok(env_args) = env_args_or_err {
            env_args.split(' ').for_each(|arg| argv.push(arg.to_string()));
        }
        NetsimdArgs::parse_from(argv)
    }
    #[cfg(not(feature = "cuttlefish"))]
    {
        let mut argv = env::args().collect::<Vec<String>>();
        if let Ok(env_args) = env_args_or_err {
            env_args.split(' ').for_each(|arg| argv.push(arg.to_string()));
        }
        NetsimdArgs::parse_from(argv)
    }
}

fn run_netsimd_with_args(args: NetsimdArgs) {
    // Log version and terminate netsimd
    if args.version {
        println!("Netsimd Version: {}", get_version());
        return;
    }

    // Log where netsim artifacts are located
    info!("netsim artifacts path: {:?}", netsimd_temp_dir());

    // Log all args
    info!("{args:#?}");

    if !args.logtostderr {
        if let Err(err) =
            redirect_std_stream(&get_instance_name(args.instance, args.connector_instance))
        {
            error!("{err:?}");
        }
        // Duplicating the previous two logs to be included in netsim_stderr.log
        info!("netsim artifacts path: {:?}", netsimd_temp_dir());
        info!("{args:#?}");
    }

    match args.connector_instance {
        #[cfg(feature = "cuttlefish")]
        Some(connector_instance) => run_netsimd_connector(args, connector_instance),
        _ => run_netsimd_primary(args),
    }
}

/// Forwards packets to another netsim daemon.
#[cfg(feature = "cuttlefish")]
fn run_netsimd_connector(args: NetsimdArgs, instance: u16) {
    if args.fd_startup_str.is_none() {
        error!("Failed to start netsimd forwarder, missing `-s` arg");
        return;
    }
    let fd_startup = args.fd_startup_str.unwrap();

    let mut server: Option<String> = None;
    // Attempts multiple time for fetching netsim.ini
    for second in [1, 2, 4, 8, 0] {
        server = get_server_address(instance);
        if server.is_some() {
            break;
        } else {
            warn!("Unable to find ini file for instance {instance}, retrying");
            std::thread::sleep(std::time::Duration::from_secs(second));
        }
    }
    if server.is_none() {
        error!("Failed to run netsimd connector");
        return;
    }
    let server = server.unwrap();
    // TODO: Make this function returns Result to use `?` instead of unwrap().
    info!("Starting in Connector mode to {}", server.as_str());
    crate::transport::fd::run_fd_connector(&fd_startup, server.as_str())
        .map_err(|e| error!("Failed to run fd connector: {e}"))
        .unwrap();
}

// loop until ShutDown event is received, then log and return.
fn main_loop(events_rx: Receiver<Event>) {
    loop {
        // events_rx.recv() will wait until the event is received.
        // TODO(b/305536480): Remove built-in devices during shutdown.
        if let Ok(Event::ShutDown(ShutDown { reason })) = events_rx.recv() {
            info!("Netsim is shutdown: {reason}");
            return;
        }
    }
}

// Disambiguate config and command line args and store merged setting in config
fn disambiguate_args(args: &mut NetsimdArgs, config: &mut Config) {
    // Command line override config file arguments

    // Currently capture cannot be specified off explicitly with command line.
    // Enable capture if enabled by command line arg
    if args.pcap {
        match config.capture.as_mut() {
            Some(capture) => {
                capture.enabled = Some(true);
            }
            None => {
                let mut capture = Capture::new();
                capture.enabled = Some(true);
                config.capture = Some(capture).into();
            }
        }
    }

    // Ensure Bluetooth config is initialized
    let bt_config = match config.bluetooth.as_mut() {
        Some(existing_bt_config) => existing_bt_config,
        None => {
            config.bluetooth = Some(BluetoothConfig::new()).into();
            config.bluetooth.as_mut().unwrap()
        }
    };

    // Set disable_address_reuse as needed
    if args.disable_address_reuse {
        bt_config.disable_address_reuse = Some(true);
    }

    // Determine test beacons configuration, default true for cuttlefish
    // TODO: remove default for cuttlefish by adding flag to tests
    bt_config.test_beacons = match (args.test_beacons, args.no_test_beacons) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        (false, false) => match bt_config.test_beacons {
            Some(test_beacons) => Some(test_beacons),
            None => Some(cfg!(feature = "cuttlefish")),
        },
        (true, true) => panic!("unexpected flag combination"),
    };
}

/// Parses a single "PHY_KIND=RSSI_VALUE" string and sets the global RSSI.
fn parse_and_set_rssi(link_manager: &Arc<LinkManager>, rssi_str: &str) -> Result<(), String> {
    let parts: Vec<&str> = rssi_str.split(':').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid RSSI default format: '{rssi_str}'. Expected PHY_KIND:RSSI_VALUE"
        ));
    }

    let phy_kind_input_str = parts[0].trim();
    let rssi_input_str = parts[1].trim();
    let phy_kind_str = phy_kind_input_str.to_uppercase();

    // Match the uppercased input string against a list of known aliases and full names
    // for PhyKind. This is case-insensitive due to the to_uppercase() call.
    let phy_kind = match phy_kind_str.as_str() {
        "BLE" | "BT_LE" | "BLUETOOTH_LOW_ENERGY" => Ok(PhyKind::BluetoothLowEnergy),
        "CLASSIC" | "BT_CLASSIC" | "BLUETOOTH_CLASSIC" => Ok(PhyKind::BluetoothClassic),
        "WIFI" => Ok(PhyKind::Wifi),
        "UWB" => Ok(PhyKind::Uwb),
        "WIFIRTT" | "WIFI_RTT" => Ok(PhyKind::WifiRtt),
        "NONE" => Ok(PhyKind::None),
        _ => Err(format!("Invalid or unhandled PhyKind string: '{phy_kind_input_str}'")),
    }?;

    let rssi_value = rssi_input_str.parse::<i8>().map_err(|e| {
        format!(
            "Invalid RSSI for {}: '{}'. Expected i8 ({} to {}). Error: {}",
            phy_kind_input_str,
            rssi_input_str,
            i8::MIN,
            i8::MAX,
            e
        )
    })?;
    info!("Setting global RSSI default: {phy_kind:?} = {rssi_value}");

    link_manager.set_rssi(ANY_CHIP, ANY_CHIP, phy_kind, rssi_value);
    Ok(())
}

/// Processes the `rssi` command line arguments and configures the LinkManager.
fn process_rssi_arg(rssi_opt: &Option<Vec<String>>, link_manager: &Arc<LinkManager>) {
    if let Some(rssi) = rssi_opt {
        for rssi_str in rssi {
            if let Err(e) = parse_and_set_rssi(link_manager, rssi_str) {
                panic!("{}", e);
            }
        }
    }
}

fn run_netsimd_primary(mut args: NetsimdArgs) {
    info!(
        "Netsim Version: {}, OS: {}, Arch: {}",
        get_version(),
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    let fd_startup_str = args.fd_startup_str.clone().unwrap_or_default();
    let instance_num = get_instance(args.instance);
    let hci_port: u16 =
        get_hci_port(args.hci_port.unwrap_or_default(), instance_num - 1).try_into().unwrap();

    #[cfg(feature = "cuttlefish")]
    if fd_startup_str.is_empty() {
        warn!("Warning: netsimd startup flag -s is empty, waiting for gRPC connections.");
    }

    if is_netsimd_alive(instance_num) {
        warn!("Failed to start netsim daemon because a netsim daemon is already running");
        return;
    }

    // Load config file
    let mut config = Config::new();
    if let Some(ref filename) = args.config {
        match config_file::new_from_file(filename) {
            Ok(config_from_file) => {
                config = config_from_file;
            }
            Err(e) => {
                error!("Skipping config in {filename}: {e:?}");
            }
        }
    }
    // Disambiguate conflicts between cmdline args and config file
    disambiguate_args(&mut args, &mut config);

    // Print config file settings
    info!("{config:#?}");

    if let Some(host_dns) = args.host_dns {
        config.wifi.mut_or_insert_default().slirp_options.mut_or_insert_default().host_dns =
            host_dns;
    }

    if let Some(http_proxy) = args.http_proxy {
        config.wifi.mut_or_insert_default().slirp_options.mut_or_insert_default().http_proxy =
            http_proxy;
    }

    // Create all Event Receivers before any events are posted
    let events = Events::new();
    let capture_events_rx = events.subscribe();
    let device_events_rx = events.subscribe();
    let main_events_rx = events.subscribe();
    let session_events_rx = events.subscribe();
    let link_manager = Arc::new(LinkManager::new());
    process_rssi_arg(&args.rssi, &link_manager);
    DeviceManager::init(events.clone(), link_manager.clone());

    // Start radio facades
    wireless::bluetooth::bluetooth_start(&config.bluetooth, instance_num);
    let wifi_stats = wireless::wifi_manager::wifi_start(
        &config.wifi,
        args.forward_host_mdns,
        args.wifi,
        args.wifi_tap,
        Arc::new(args.debug),
    );
    wireless::uwb::uwb_start();

    // Instantiate ServiceParams
    let service_params = ServiceParams::new(
        fd_startup_str,
        args.no_cli_ui,
        args.no_web_ui,
        hci_port,
        instance_num,
        args.dev,
        args.vsock.unwrap_or_default(),
    );

    // SAFETY: The caller guaranteed that the file descriptors in `fd_startup_str` would remain
    // valid and open for as long as the program runs.
    let mut service = unsafe { Service::new(service_params, link_manager) };

    // Run all netsimd services (grpc, socket, web)
    match service.run() {
        Err(e) => {
            error!("service.run() -> Err({e:?})");
            error!("Failed to run netsimd services, exiting...");
            service.shut_down();
            return;
        }
        Ok((grpc_port, web_port, websocket_port)) => {
            // If create_ini fails, check if there is another netsimd instance.
            // If there isn't another netsimd instance, remove_ini and create_ini once more.
            for _ in 0..2 {
                match create_ini(instance_num, grpc_port, web_port, websocket_port) {
                    Err(e) => {
                        warn!("create_ini error with {e:?}");
                        // Continue if the address overlaps to support Oxygen CF Boot.
                        // The pre-warmed device may leave stale netsim ini with the same grpc port.
                        if let Some(address) = get_server_address(instance_num) {
                            // If the address matches, break the loop and continue running netsimd.
                            if address == format!("localhost:{grpc_port}") {
                                info!("Reusing existing netsim ini with grpc_port: {grpc_port}");
                                break;
                            }
                        }
                        // Checkes if a different netsimd instance exists
                        if is_netsimd_alive(instance_num) {
                            warn!("netsimd already running, exiting...");
                            service.shut_down();
                            return;
                        } else {
                            info!("Removing stale netsim ini");
                            if let Err(e) = remove_ini(instance_num) {
                                error!("{e:?}");
                            }
                        }
                    }
                    _ => {
                        break;
                    }
                }
            }
        }
    }

    // Gets rid of old artifacts (pcap and zip files)
    service.remove_artifacts();

    // Start Session Event listener
    let mut session = Session::new();
    session.start(session_events_rx, wifi_stats.clone());

    // Pass all event receivers to each modules
    let capture = config.capture.enabled.unwrap_or_default();
    spawn_capture_event_subscriber(capture_events_rx, capture);

    if !args.no_shutdown {
        spawn_shutdown_publisher(device_events_rx, events);
    }

    // Create test beacons if required
    if config.bluetooth.test_beacons == Some(true) {
        new_test_beacon(1, 1000);
        new_test_beacon(2, 1000);
    }

    // Runs a synchronous main loop
    main_loop(main_events_rx);

    // Gracefully shutdown netsimd services
    service.shut_down();

    // write out session stats
    let _ = session.stop(wifi_stats);

    // zip all artifacts
    if let Err(err) = zip_artifacts() {
        error!("Failed to zip artifacts: {err:?}");
    }

    // Once shutdown is complete, delete the netsim ini file
    match remove_ini(instance_num) {
        Ok(_) => info!("Removed netsim ini file"),
        Err(e) => error!("Failed to remove netsim ini file: {e:?}"),
    }
}

fn is_netsimd_alive(instance_num: u16) -> bool {
    for i in 0..2 {
        match get_server_address(instance_num) {
            Some(address) => {
                // Check if grpc server has started
                let channel = ChannelBuilder::new(std::sync::Arc::new(EnvBuilder::new().build()))
                    .connect(&address);
                let client = FrontendServiceClient::new(channel);
                let deadline = Deadline::from(std::time::Duration::from_secs(1));
                return futures::executor::block_on(
                    client.client.channel().wait_for_connected(deadline),
                );
            }
            None => {
                if i == 1 {
                    warn!("get_server_address({instance_num}) returned None.");
                    break;
                }
                info!("get_server_address({instance_num}) returned None. Retrying...");
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    }
    false
}
