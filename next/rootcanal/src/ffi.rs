// Copyright 2025 The Android Open Source Project
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

//! Bindings for rootcanal/models/controller/ffi.h

#![allow(non_camel_case_types)]

/// A C-compatible unsigned 8-bit integer.
type uint8_t = u8;
/// A C-compatible size type.
pub(crate) type size_t = usize;

use std::{
    option::Option,
    os::raw::{c_char, c_int, c_void},
};

/// Callback signature for rootcanal log interception.
type FfiLogCallback =
    extern "C" fn(level: c_int, file: *const c_char, line: c_int, message: *const c_char);

unsafe extern "C" {
    /// Creates a new Bluetooth controller.
    ///
    /// # Safety
    /// * `address` must be a valid, non-null pointer to a contiguous array of
    ///   exactly 6 bytes (`[u8; 6]`).
    /// * If `proto_bytes` is not null, `proto_len` must accurately reflect the
    ///   size of the readable memory it points to, and it must contain a valid
    ///   serialized Protobuf configuration.
    /// * The caller must ensure that the function pointers (`send_hci`,
    ///   `send_ll`, `invalid_packet_handler`, `ranging_estimator`) are valid
    ///   C-ABI function pointers if they are provided (not `None`).
    /// * `cookie` can be any pointer (including null) but it will be blindly
    ///   passed back to the provided function pointers. The caller is
    ///   responsible for ensuring it is valid when those callbacks are invoked.
    pub(crate) fn ffi_controller_new(
        address: *const uint8_t,
        send_hci: Option<
            unsafe extern "C" fn(
                cookie: *mut c_void,
                idc: c_int,
                data: *const uint8_t,
                data_len: size_t,
            ),
        >,
        send_ll: Option<
            unsafe extern "C" fn(
                cookie: *mut c_void,
                data: *const uint8_t,
                data_len: size_t,
                phy: c_int,
                tx_power: c_int,
            ),
        >,
        invalid_packet_handler: Option<
            unsafe extern "C" fn(
                cookie: *mut c_void,
                reason: c_int,
                message: *const c_char,
                data: *const uint8_t,
                data_len: size_t,
            ),
        >,
        ranging_estimator: Option<
            unsafe extern "C" fn(
                cookie: *mut c_void,
                source_addr: *const u8,
                destination_addr: *const u8,
            ) -> u32,
        >,
        cookie: *mut c_void,
        proto_bytes: *const uint8_t,
        proto_len: size_t,
    ) -> *mut c_void;

    /// Deletes a Bluetooth controller.
    pub(crate) fn ffi_controller_delete(controller: *mut c_void);

    /// Receives an HCI packet from the host.
    pub(crate) fn ffi_controller_receive_hci(
        controller: *mut c_void,
        idc: c_int,
        data: *const uint8_t,
        data_len: size_t,
    );

    /// Receives a link layer packet from a peer.
    pub(crate) fn ffi_controller_receive_ll(
        controller: *mut c_void,
        data: *const uint8_t,
        data_len: size_t,
        phy: c_int,
        rssi: c_int,
    );

    /// Reconfigures the controller with new properties.
    pub(crate) fn ffi_controller_set_properties(
        controller: *mut c_void,
        proto_bytes: *const uint8_t,
        proto_len: size_t,
    ) -> bool;

    /// Advances the controller's state by one tick.
    pub(crate) fn ffi_controller_tick(controller: *mut c_void);

    /// Returns true if the controller has a connection to the given address.
    ///
    /// # Safety
    /// * `controller` must be a valid, non-null pointer returned by
    ///   `ffi_controller_new` that hasn't been deleted.
    /// * `source_addr` and `destination_addr` must be valid, non-null pointers
    ///   to contiguous arrays of exactly 6 bytes (`[u8; 6]`).
    pub(crate) fn ffi_controller_has_le_connection(
        controller: *mut c_void,
        source_addr: *const uint8_t,
        destination_addr: *const uint8_t,
    ) -> bool;

    /// Registers a log callback for rootcanal logs.
    ///
    /// # Safety
    /// When specified, `callback` must point to a thread-safe `extern "C"`
    /// function that expects a valid null-terminated C string.
    pub(crate) fn ffi_set_log_callback(callback: Option<FfiLogCallback>);
}
