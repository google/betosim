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

//! Netsim daemon cxx libraries.

use crate::bluetooth::chip::{
    AddRustDeviceResult, RustBluetoothChipCallbacks, create_add_rust_device_result,
};
use crate::devices::devices_handler::get_rssi;
use crate::wireless::{
    bluetooth::report_invalid_packet_cxx, handle_request_cxx, handle_response_cxx,
};

#[allow(unsafe_op_in_unsafe_fn)]
#[cxx::bridge(namespace = "netsim::wireless")]
pub mod ffi_wireless {
    extern "Rust" {
        #[cxx_name = HandleRequestCxx]
        fn handle_request_cxx(chip_id: u32, packet: &CxxVector<u8>, packet_type: u8);

        #[cxx_name = HandleResponse]
        fn handle_response_cxx(chip_id: u32, packet: &CxxVector<u8>, packet_type: u8);
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
#[cxx::bridge(namespace = "netsim::transport")]
pub mod ffi_transport {
    unsafe extern "C++" {
        // Grpc client.
        // Expose functions in Cuttlefish only, because it's only used by CVDs and it's
        // unable to pass function pointers on Windows.
        #[cfg(feature = "cuttlefish")]
        include!("backend/grpc_client.h");

        #[allow(dead_code)]
        #[rust_name = stream_packets]
        #[namespace = "netsim::backend::client"]
        #[cfg(feature = "cuttlefish")]
        fn StreamPackets(server: &String) -> u32;

        #[allow(dead_code)]
        #[rust_name = read_packet_response_loop]
        #[namespace = "netsim::backend::client"]
        #[cfg(feature = "cuttlefish")]
        fn ReadPacketResponseLoop(
            stream_id: u32,
            read_fn: fn(stream_id: u32, proto_bytes: &[u8]),
        ) -> bool;

        #[allow(dead_code)]
        #[rust_name = write_packet_request]
        #[cfg(feature = "cuttlefish")]
        #[namespace = "netsim::backend::client"]
        fn WritePacketRequest(stream_id: u32, proto_bytes: &[u8]) -> bool;

    }
}

#[allow(clippy::needless_maybe_sized)]
#[allow(unsafe_op_in_unsafe_fn)]
#[allow(unused_attributes)]
#[cxx::bridge(namespace = "netsim")]
pub mod ffi_bluetooth {
    extern "Rust" {
        // Rust Bluetooth device.
        #[namespace = "netsim::hci::facade"]
        type DynRustBluetoothChipCallbacks;

        #[cxx_name = Tick]
        #[namespace = "netsim::hci::facade"]
        fn tick(dyn_callbacks: &mut DynRustBluetoothChipCallbacks);

        #[cxx_name = ReceiveLinkLayerPacket]
        #[namespace = "netsim::hci::facade"]
        fn receive_link_layer_packet(
            dyn_callbacks: &mut DynRustBluetoothChipCallbacks,
            source_address: String,
            destination_address: String,
            packet_type: u8,
            packet: &[u8],
        );

        // Bluetooth facade.
        #[namespace = "netsim::hci::facade"]
        type AddRustDeviceResult;
        #[cxx_name = "CreateAddRustDeviceResult"]
        #[namespace = "netsim::hci"]
        fn create_add_rust_device_result(
            facade_id: u32,
            rust_chip: UniquePtr<RustBluetoothChip>,
        ) -> Box<AddRustDeviceResult>;

        // Rust Invalid Packet Report
        #[cxx_name = "ReportInvalidPacket"]
        #[namespace = "netsim::hci::facade"]
        fn report_invalid_packet_cxx(
            rootcanal_id: u32,
            reason: i32,
            description: &CxxString,
            packet: &CxxVector<u8>,
        );
    }

    #[allow(dead_code)]
    unsafe extern "C++" {
        // Bluetooth facade.
        include!("hci/hci_packet_hub.h");

        #[rust_name = handle_bt_request]
        #[namespace = "netsim::hci"]
        fn HandleBtRequestCxx(rootcanal_id: u32, packet_type: u8, packet: &Vec<u8>);

        // Rust Bluetooth device.
        include!("hci/rust_device.h");

        #[namespace = "netsim::hci::facade"]
        type RustBluetoothChip;
        #[rust_name = send_link_layer_le_packet]
        #[namespace = "netsim::hci::facade"]
        fn SendLinkLayerLePacket(self: &RustBluetoothChip, packet: &[u8], tx_power: i8);

        include!("hci/bluetooth_facade.h");

        #[rust_name = bluetooth_get_cxx]
        #[namespace = "netsim::hci::facade"]
        pub fn GetCxx(rootcanal_id: u32) -> Vec<u8>;

        #[rust_name = bluetooth_reset]
        #[namespace = "netsim::hci::facade"]
        pub fn Reset(rootcanal_id: u32);

        #[rust_name = bluetooth_remove]
        #[namespace = "netsim::hci::facade"]
        pub fn Remove(rootcanal_id: u32);

        #[rust_name = bluetooth_add]
        #[namespace = "netsim::hci::facade"]
        pub fn Add(chip_id: u32, address: &CxxString, controller_proto_bytes: &[u8]) -> u32;

        /*
        From https://cxx.rs/binding/box.html#restrictions,
        ```
        If T is an opaque Rust type, the Rust type is required to be Sized i.e. size known at compile time. In the future we may introduce support for dynamically sized opaque Rust types.
        ```

        The workaround is using Box<dyn MyData> (fat pointer) as the opaque type.
        Reference:
        - Passing trait objects to C++. https://github.com/dtolnay/cxx/issues/665.
        - Exposing trait methods to C++. https://github.com/dtolnay/cxx/issues/667
                */
        #[rust_name = bluetooth_add_rust_device]
        #[namespace = "netsim::hci::facade"]
        pub fn AddRustDevice(
            chip_id: u32,
            callbacks: Box<DynRustBluetoothChipCallbacks>,
            string_type: &CxxString,
            address: &CxxString,
        ) -> Box<AddRustDeviceResult>;

        /// The provided address must be 6 bytes in length
        #[rust_name = bluetooth_set_rust_device_address]
        #[namespace = "netsim::hci::facade"]
        pub fn SetRustDeviceAddress(rootcanal_id: u32, address: [u8; 6]);

        #[rust_name = bluetooth_remove_rust_device]
        #[namespace = "netsim::hci::facade"]
        pub fn RemoveRustDevice(rootcanal_id: u32);

        #[rust_name = bluetooth_start]
        #[namespace = "netsim::hci::facade"]
        pub fn Start(proto_bytes: &[u8], instance_num: u16);

        #[rust_name = bluetooth_stop]
        #[namespace = "netsim::hci::facade"]
        pub fn Stop();

        #[rust_name = add_device_to_phy]
        #[namespace = "netsim::hci::facade"]
        pub fn AddDeviceToPhy(rootcanal_id: u32, is_low_energy: bool);

        #[rust_name = remove_device_from_phy]
        #[namespace = "netsim::hci::facade"]
        pub fn RemoveDeviceFromPhy(rootcanal_id: u32, is_low_energy: bool);
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
#[cxx::bridge(namespace = "netsim")]
pub mod ffi_util {
    extern "Rust" {
        // Ranging

        #[cxx_name = "GetRssi"]
        fn get_rssi(sender: u32, receiver: u32, link_kind: i32, tx_power: i8) -> i8;
    }

    #[allow(dead_code)]
    unsafe extern "C++" {

        // Crash report.
        include!("util/crash_report.h");

        #[rust_name = set_up_crash_report]
        #[namespace = "netsim"]
        pub fn SetUpCrashReport();
    }
}

// It's required so `RustBluetoothChip` can be sent between threads safely.
// Ref: How to use opaque types in threads? https://github.com/dtolnay/cxx/issues/1175
// SAFETY: Nothing in `RustBluetoothChip` depends on being run on a particular thread.
unsafe impl Send for ffi_bluetooth::RustBluetoothChip {}

type DynRustBluetoothChipCallbacks = Box<dyn RustBluetoothChipCallbacks>;

fn tick(dyn_callbacks: &mut DynRustBluetoothChipCallbacks) {
    (**dyn_callbacks).tick();
}

fn receive_link_layer_packet(
    dyn_callbacks: &mut DynRustBluetoothChipCallbacks,
    source_address: String,
    destination_address: String,
    packet_type: u8,
    packet: &[u8],
) {
    (**dyn_callbacks).receive_link_layer_packet(
        source_address,
        destination_address,
        packet_type,
        packet,
    );
}
