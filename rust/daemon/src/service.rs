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

#![allow(clippy::empty_line_after_doc_comments)]

use crate::bluetooth::advertise_settings as ble_advertise_settings;
use crate::captures::captures_handler::clear_pcap_files;
use crate::http_server::server::run_http_server;
use crate::links::link::LinkManager;
use crate::transport::socket::run_socket_transport;
use crate::websocket_server::run_websocket_server;
use crate::wireless;
use log::{error, info, warn};
use netsim_common::util::zip_artifact::remove_zip_files;
use std::time::Duration;
use std::{env, sync::Arc};

/// Module to control startup, run, and cleanup netsimd services.

type GrpcPort = u32;
type WebPort = Option<u16>;
type WebSocketPort = Option<u16>;

pub struct ServiceParams {
    fd_startup_str: String,
    no_cli_ui: bool,
    no_web_ui: bool,
    hci_port: u16,
    instance_num: u16,
    dev: bool,
    vsock: u16,
}

impl ServiceParams {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fd_startup_str: String,
        no_cli_ui: bool,
        no_web_ui: bool,
        hci_port: u16,
        instance_num: u16,
        dev: bool,
        vsock: u16,
    ) -> Self {
        ServiceParams { fd_startup_str, no_cli_ui, no_web_ui, hci_port, instance_num, dev, vsock }
    }
}

pub struct Service {
    // netsimd states, like device resource.
    service_params: ServiceParams,
    link_manager: Arc<LinkManager>,
    grpc_server: Option<grpcio::Server>,
}

impl Service {
    /// # Safety
    ///
    /// The file descriptors in `service_params.fd_startup_str` must be valid and open, and must
    /// remain so for as long as the `Service` exists.
    pub unsafe fn new(service_params: ServiceParams, link_manager: Arc<LinkManager>) -> Service {
        Service { service_params, link_manager, grpc_server: None }
    }

    /// Remove old artifacts
    pub fn remove_artifacts(&self) {
        // Clear all zip files
        match remove_zip_files() {
            Ok(()) => info!("netsim generated zip files in temp directory has been removed."),
            Err(err) => error!("{err:?}"),
        }

        // Clear all pcap files
        if clear_pcap_files() {
            info!("netsim generated pcap files in temp directory has been removed.");
        }
    }

    /// Runs netsim gRPC server
    fn run_grpc_server(&mut self) -> anyhow::Result<u32> {
        // If NETSIM_GRPC_PORT is set, use the fixed port for grpc server.
        let mut netsim_grpc_port =
            env::var("NETSIM_GRPC_PORT").map(|val| val.parse::<u32>().unwrap_or(0)).unwrap_or(0);
        // Run netsim gRPC server
        let (server, port) = crate::grpc_server::server::start(
            netsim_grpc_port,
            self.service_params.no_cli_ui,
            self.link_manager.clone(),
            self.service_params.vsock,
        )?;
        self.grpc_server = Some(server);
        netsim_grpc_port = port.into();
        Ok(netsim_grpc_port)
    }

    /// Runs netsim web server
    fn run_web_server(&self) -> Option<u16> {
        // If NETSIM_NO_WEB_SERVER is set, don't start http server.
        let no_web_server = env::var("NETSIM_NO_WEB_SERVER").is_ok_and(|v| v == "1");
        match !no_web_server && !self.service_params.no_web_ui {
            true => Some(run_http_server(
                self.service_params.instance_num,
                self.service_params.dev,
                self.link_manager.clone(),
            )),
            false => None,
        }
    }

    /// Runs the netsimd services.
    #[allow(unused_unsafe)]
    pub fn run(&mut self) -> anyhow::Result<(GrpcPort, WebPort, WebSocketPort)> {
        if !self.service_params.fd_startup_str.is_empty() {
            // SAFETY: When the `Service` was constructed by `Service::new` the caller guaranteed
            // that the file descriptors in `service_params.fd_startup_str` would remain valid and
            // open.
            unsafe {
                use crate::transport::fd::run_fd_transport;
                run_fd_transport(&self.service_params.fd_startup_str);
            }
        }

        let grpc_port = match self.run_grpc_server() {
            Ok(port) => port,
            Err(e) => {
                error!("Failed to run netsimd: {e:?}");
                return Err(e);
            }
        };

        // Run frontend web server
        let web_port = self.run_web_server();

        // Run the websocket server.
        let websocket_port = run_websocket_server(self.service_params.instance_num)
            .map_err(|e| warn!("run_websocket_server error: {e}"))
            .ok();

        // Run the socket server.
        run_socket_transport(self.service_params.hci_port);

        Ok((grpc_port, web_port, websocket_port))
    }

    /// Shut down the netsimd services
    pub fn shut_down(&mut self) {
        // TODO: shutdown other services in Rust
        self.grpc_server.as_mut().map(|server| server.shutdown());
        wireless::bluetooth::bluetooth_stop();
        wireless::wifi_manager::wifi_stop();
    }
}

/// Constructing test beacons for dev mode
pub fn new_test_beacon(idx: u32, interval: u64) {
    use crate::devices::devices_handler::create_device;
    use netsim_proto::common::ChipKind;
    use netsim_proto::frontend::CreateDeviceRequest;
    use netsim_proto::model::ChipCreate as ChipCreateProto;
    use netsim_proto::model::DeviceCreate as DeviceCreateProto;
    use netsim_proto::model::chip::ble_beacon::{
        AdvertiseData as AdvertiseDataProto, AdvertiseSettings as AdvertiseSettingsProto,
    };
    use netsim_proto::model::chip_create::{
        BleBeaconCreate as BleBeaconCreateProto, Chip as ChipProto,
    };
    use protobuf::MessageField;

    let beacon_proto = BleBeaconCreateProto {
        address: format!("be:ac:01:be:ef:{idx:02x}"),
        settings: MessageField::some(AdvertiseSettingsProto {
            interval: Some(
                ble_advertise_settings::AdvertiseMode::new(Duration::from_millis(interval))
                    .try_into()
                    .unwrap(),
            ),
            scannable: true,
            ..Default::default()
        }),
        adv_data: MessageField::some(AdvertiseDataProto {
            include_device_name: true,
            ..Default::default()
        }),
        scan_response: MessageField::some(AdvertiseDataProto {
            manufacturer_data: vec![1u8, 2, 3, 4],
            ..Default::default()
        }),
        ..Default::default()
    };

    let chip_proto = ChipCreateProto {
        name: format!("gDevice-bt-beacon-chip-{idx}"),
        kind: ChipKind::BLUETOOTH_BEACON.into(),
        chip: Some(ChipProto::BleBeacon(beacon_proto)),
        ..Default::default()
    };

    let device_proto = DeviceCreateProto {
        name: format!("gDevice-beacon-{idx}"),
        chips: vec![chip_proto],
        ..Default::default()
    };

    let request =
        CreateDeviceRequest { device: MessageField::some(device_proto), ..Default::default() };

    if let Err(err) = create_device(&request) {
        warn!("Failed to create beacon device {idx}: {err}");
    }
}
