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

//! This module defines the `Controller` struct, which represents a Bluetooth
//! controller.

/// A unique ID for the Controller
pub type Id = u32;

use std::{
    ffi::{c_int, c_void},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use bytes::{BufMut, Bytes, BytesMut};
use parking_lot::Mutex;
use tracing::warn;

use crate::{
    error::{Error, Result},
    ffi,
    types::{Address, Phy},
};

/// Callbacks for the Bluetooth controller.
pub trait Callbacks: Send + Sync {
    /// Request from rootcanal to send an HCI packet to the host.
    fn send_hci(&self, source_id: Id, h4_packet: Bytes);

    /// Called when the controller is receiving a packet
    fn on_receive_ll(&self, sender_id: Id, packet: &[u8], phy: Phy, rssi: i32);

    /// Called when an invalid packet is received.
    fn invalid_packet_received(&self, source_id: Id, reason: c_int, message: &str, data: &[u8]);
}

/// Trait used by the controller to call the Bluetooth manager.
pub trait BtOps: Send + Sync {
    /// Controller received a LL packet from rootcanal that needs to be
    /// broadcasted
    fn broadcast_rootcanal_ll_packet(&self, send_id: Id, packet: &[u8], phy: Phy, tx_power: i32);

    /// Controller needs distance to another device by address
    fn estimate_distance(
        &self,
        source_id: u32,
        source_addr: &[u8; 6],
        destination_addr: &[u8; 6],
    ) -> u32;
}

// A wrapper around the raw C++ controller pointer.
struct FfiController(*mut c_void);

// SAFETY: This wrapper is only ever accessed through a Mutex in ControllerImpl,
// which guarantees exclusive access and makes it safe to send across threads.
unsafe impl Send for FfiController {}

/// The internal state of a Bluetooth controller.
pub(crate) struct ControllerImpl {
    controller: Mutex<FfiController>,
    id: Id,
    address: Address,
    pub(crate) callbacks: Box<dyn Callbacks>,
    bt_ops: Box<dyn BtOps>,
    hci_commands_in: AtomicU64,
    hci_events_out: AtomicU64,
    invalid_packets: AtomicU64,
    ll_packets_in: AtomicU64,
    ll_packets_out: AtomicU64,
    ll_packets_dropped: AtomicU64,
    ll_packets_in_ble: AtomicU64,
    ll_packets_in_classic: AtomicU64,
    ll_packets_out_ble: AtomicU64,
    ll_packets_out_classic: AtomicU64,
    ble_p2p_tx_count: AtomicU64,
    ble_p2p_rx_count: AtomicU64,
    classic_p2p_tx_count: AtomicU64,
    classic_p2p_rx_count: AtomicU64,
}

/// A Bluetooth controller.
pub(crate) type Controller = Arc<ControllerImpl>;

/// A collection of packet statistics.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// The number of HCI packets received from the host.
    pub hci_commands_in: u64,
    /// The number of HCI packets sent to the host.
    pub hci_events_out: u64,
    /// The number of invalid packets received.
    pub invalid_packets: u64,
    /// The number of link layer packets received from the peer.
    pub ll_packets_in: u64,
    /// The number of link layer packets sent to the peer.
    pub ll_packets_out: u64,
    /// The number of link layer packets dropped.
    pub ll_packets_dropped: u64,
    /// The number of BLE link layer packets received.
    pub ll_packets_in_ble: u64,
    /// The number of Classic link layer packets received.
    pub ll_packets_in_classic: u64,
    /// The number of BLE link layer packets sent.
    pub ll_packets_out_ble: u64,
    /// The number of Classic link layer packets sent.
    pub ll_packets_out_classic: u64,
    /// The number of payload-bearing BLE P2P packets sent over the air.
    pub ble_p2p_tx_count: u64,
    /// The number of payload-bearing BLE P2P packets received over the air.
    pub ble_p2p_rx_count: u64,
    /// The number of payload-bearing Classic P2P packets sent over the air.
    pub classic_p2p_tx_count: u64,
    /// The number of payload-bearing Classic P2P packets received over the air.
    pub classic_p2p_rx_count: u64,
}

// The context that is passed to the C++ code.
type CallbackContext = Weak<ControllerImpl>;

impl ControllerImpl {
    pub(crate) fn new(
        id: Id,
        address: Address,
        callbacks: Box<dyn Callbacks>,
        bt_ops: Box<dyn BtOps>,
        properties: Option<&[u8]>,
    ) -> Controller {
        Arc::new_cyclic(|weak| {
            // Create the context for the C++ side, using a Weak pointer to avoid cycles.
            let context = Box::new(weak.clone());
            let context_ptr = Box::into_raw(context);

            let (proto_ptr, proto_len) = match properties {
                Some(bytes) => (bytes.as_ptr(), bytes.len()),
                None => (std::ptr::null(), 0),
            };

            // Call the FFI to get the real controller pointer.
            // SAFETY: The `address` pointer is valid for the duration of this call.
            // The C++ side is expected to copy the address data, not store the pointer.
            // We transfer ownership of `context_ptr` to the C++ library. The C++
            // library is responsible for calling `ffi_controller_delete`, which
            // will eventually drop the `CallbackContext` and its contents.
            let controller_ptr = unsafe {
                ffi::ffi_controller_new(
                    address.as_bytes().as_ptr(),
                    Some(send_hci_trampoline),
                    Some(send_ll_trampoline),
                    Some(invalid_packet_trampoline),
                    Some(ranging_estimator_trampoline),
                    context_ptr.cast::<c_void>(),
                    proto_ptr,
                    proto_len,
                )
            };

            ControllerImpl {
                controller: Mutex::new(FfiController(controller_ptr)),
                id,
                address,
                callbacks,
                bt_ops,
                hci_commands_in: AtomicU64::new(0),
                hci_events_out: AtomicU64::new(0),
                invalid_packets: AtomicU64::new(0),
                ll_packets_in: AtomicU64::new(0),
                ll_packets_out: AtomicU64::new(0),
                ll_packets_dropped: AtomicU64::new(0),
                ll_packets_in_ble: AtomicU64::new(0),
                ll_packets_in_classic: AtomicU64::new(0),
                ll_packets_out_ble: AtomicU64::new(0),
                ll_packets_out_classic: AtomicU64::new(0),
                ble_p2p_tx_count: AtomicU64::new(0),
                ble_p2p_rx_count: AtomicU64::new(0),
                classic_p2p_tx_count: AtomicU64::new(0),
                classic_p2p_rx_count: AtomicU64::new(0),
            }
        })
    }

    /// Receives an HCI packet from the host.
    pub(crate) fn receive_hci(&self, data: Bytes) {
        self.hci_commands_in.fetch_add(1, Ordering::Relaxed);
        let (&first, payload) = match data.as_ref() {
            [first, rest @ ..] => (first, rest),
            [] => return,
        };
        let controller = self.controller.lock();
        let idc = first as c_int;
        // SAFETY: The `controller.0` pointer is guaranteed to be valid
        // as long as `self` exists. `payload` is a valid slice, and we provide its
        // length to ensure the C++ side does not read out of bounds.
        unsafe {
            ffi::ffi_controller_receive_hci(
                controller.0,
                idc,
                payload.as_ptr(),
                payload.len() as ffi::size_t,
            )
        };
    }

    /// Receives a link layer packet from a peer.
    pub(crate) fn receive_ll(&self, data: &[u8], phy: Phy, rssi: i32) {
        self.ll_packets_in.fetch_add(1, Ordering::Relaxed);
        let is_p2p = netsim_packets::link_layer::fast_inspect_p2p_payload(data);
        match phy {
            Phy::LowEnergy => {
                self.ll_packets_in_ble.fetch_add(1, Ordering::Relaxed);
                if is_p2p {
                    self.ble_p2p_rx_count.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ => {
                self.ll_packets_in_classic.fetch_add(1, Ordering::Relaxed);
                if is_p2p {
                    self.classic_p2p_rx_count.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        let controller = self.controller.lock();
        // SAFETY: The `controller.0` pointer is guaranteed to be valid
        // as long as `self` exists. `data` is a valid slice, and we provide its
        // length to ensure the C++ side does not read out of bounds.
        unsafe {
            ffi::ffi_controller_receive_ll(
                controller.0,
                data.as_ptr(),
                data.len() as ffi::size_t,
                phy as c_int,
                rssi,
            )
        };
    }

    /// Reconfigures the controller with new properties.
    pub(crate) fn set_properties(&self, properties: &[u8]) -> Result<()> {
        let controller = self.controller.lock();

        // Defensively handle empty slices to avoid passing dangling pointers to FFI
        let (ptr, len) = if properties.is_empty() {
            (std::ptr::null(), 0)
        } else {
            (properties.as_ptr(), properties.len())
        };

        // SAFETY: The `controller.0` pointer is guaranteed to be valid.
        // `ptr` is either null (with len 0) or points to a valid slice of bytes.
        // The C++ function parses the properties synchronously and does not retain the
        // pointer.
        let success =
            unsafe { ffi::ffi_controller_set_properties(controller.0, ptr, len as ffi::size_t) };
        if success { Ok(()) } else { Err(Error::SetPropertiesFailed) }
    }

    /// Advances the controller's state by one tick.
    pub(crate) fn tick(&self) {
        let controller = self.controller.lock();
        // SAFETY: The `controller.0` pointer is guaranteed to be valid
        // as long as `self` exists, as its lifetime is tied to the `ControllerImpl`.
        unsafe { ffi::ffi_controller_tick(controller.0) };
    }

    /// Returns true if the controller has a connection to the given address.
    pub(crate) fn has_le_connection(
        &self,
        source_addr: &[u8; 6],
        destination_addr: &[u8; 6],
    ) -> bool {
        let controller = self.controller.lock();
        // SAFETY: The `controller.0` pointer is guaranteed to be valid
        // as long as `self` exists, as its lifetime is tied to the `ControllerImpl`.
        // The address slices are also guaranteed to be valid.
        unsafe {
            ffi::ffi_controller_has_le_connection(
                controller.0,
                source_addr.as_ptr(),
                destination_addr.as_ptr(),
            )
        }
    }

    /// Returns the controller's address.
    pub(crate) fn get_address(&self) -> Address {
        self.address
    }

    /// Returns the controller's unique id.
    pub(crate) fn get_id(&self) -> Id {
        self.id
    }

    /// Returns the current packet statistics.
    pub(crate) fn get_stats(&self) -> Stats {
        Stats {
            hci_commands_in: self.hci_commands_in.load(Ordering::Relaxed),
            hci_events_out: self.hci_events_out.load(Ordering::Relaxed),
            invalid_packets: self.invalid_packets.load(Ordering::Relaxed),
            ll_packets_in: self.ll_packets_in.load(Ordering::Relaxed),
            ll_packets_out: self.ll_packets_out.load(Ordering::Relaxed),
            ll_packets_dropped: self.ll_packets_dropped.load(Ordering::Relaxed),
            ll_packets_in_ble: self.ll_packets_in_ble.load(Ordering::Relaxed),
            ll_packets_in_classic: self.ll_packets_in_classic.load(Ordering::Relaxed),
            ll_packets_out_ble: self.ll_packets_out_ble.load(Ordering::Relaxed),
            ll_packets_out_classic: self.ll_packets_out_classic.load(Ordering::Relaxed),
            ble_p2p_tx_count: self.ble_p2p_tx_count.load(Ordering::Relaxed),
            ble_p2p_rx_count: self.ble_p2p_rx_count.load(Ordering::Relaxed),
            classic_p2p_tx_count: self.classic_p2p_tx_count.load(Ordering::Relaxed),
            classic_p2p_rx_count: self.classic_p2p_rx_count.load(Ordering::Relaxed),
        }
    }

    /// Increments the number of link layer packets dropped.
    pub(crate) fn increment_ll_packets_dropped(&self) {
        self.ll_packets_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Clears the packet statistics.
    pub(crate) fn clear_stats(&self) {
        self.hci_commands_in.store(0, Ordering::Relaxed);
        self.hci_events_out.store(0, Ordering::Relaxed);
        self.invalid_packets.store(0, Ordering::Relaxed);
        self.ll_packets_in.store(0, Ordering::Relaxed);
        self.ll_packets_out.store(0, Ordering::Relaxed);
        self.ll_packets_dropped.store(0, Ordering::Relaxed);
        self.ll_packets_in_ble.store(0, Ordering::Relaxed);
        self.ll_packets_in_classic.store(0, Ordering::Relaxed);
        self.ll_packets_out_ble.store(0, Ordering::Relaxed);
        self.ll_packets_out_classic.store(0, Ordering::Relaxed);
        self.ble_p2p_tx_count.store(0, Ordering::Relaxed);
        self.ble_p2p_rx_count.store(0, Ordering::Relaxed);
        self.classic_p2p_tx_count.store(0, Ordering::Relaxed);
        self.classic_p2p_rx_count.store(0, Ordering::Relaxed);
    }
}

impl Drop for ControllerImpl {
    fn drop(&mut self) {
        let controller = self.controller.lock();
        // SAFETY: This is called when the last `Arc<ControllerImpl>` is dropped,
        // ensuring the C++ object is deleted exactly once. The `controller.0`
        // pointer is valid at this point.
        unsafe { ffi::ffi_controller_delete(controller.0) };
    }
}

// A helper function to get the context back from the cookie.
unsafe fn context_from_cookie<'a>(cookie: *mut c_void) -> &'a mut CallbackContext {
    // SAFETY: The `cookie` is an opaque pointer passed to us from the C++ side.
    // The FFI contract guarantees that this is the same pointer we provided
    // during `ffi_controller_new` and that it points to a valid `CallbackContext`.
    // The lifetime of the returned reference is tied to the scope of the calling
    // trampoline function, which is safe.
    unsafe { &mut *(cookie.cast::<CallbackContext>()) }
}

// The trampoline function that is called by the C++ code.
extern "C" fn send_hci_trampoline(
    cookie: *mut c_void,
    idc: c_int,
    data: *const u8,
    data_len: ffi::size_t,
) {
    // SAFETY: The FFI contract guarantees that the C++ side provides a valid
    // `cookie`.
    let context = unsafe { context_from_cookie(cookie) };

    if let Some(controller) = context.upgrade() {
        // SAFETY: The FFI contract guarantees that `data` points to a buffer of at
        // least `data_len` bytes that is valid for the duration of this call.
        let data_slice = unsafe { std::slice::from_raw_parts(data, data_len as ffi::size_t) };
        controller.hci_events_out.fetch_add(1, Ordering::Relaxed);
        let mut h4_buffer = BytesMut::with_capacity(data_len + 1);
        h4_buffer.put_u8(idc as u8);
        h4_buffer.extend_from_slice(data_slice);
        controller.callbacks.send_hci(controller.get_id(), h4_buffer.into());
    }
}

// The trampoline function that is called by the C++ code.
extern "C" fn invalid_packet_trampoline(
    cookie: *mut c_void,
    reason: c_int,
    message: *const std::os::raw::c_char,
    data: *const u8,
    data_len: ffi::size_t,
) {
    warn!("invalid packet received");
    // SAFETY: The `cookie` is an opaque pointer passed to us from the C++ side.
    let context = unsafe { context_from_cookie(cookie) };
    if let Some(controller) = context.upgrade() {
        controller.invalid_packets.fetch_add(1, Ordering::Relaxed);
        // SAFETY: The FFI contract guarantees that `message` is a valid,
        // null-terminated C string.
        let message_str = unsafe { std::ffi::CStr::from_ptr(message) }.to_str().unwrap_or("");
        // SAFETY: The FFI contract guarantees that `data` points to a buffer of at
        // least `data_len` bytes.
        let data_slice = unsafe { std::slice::from_raw_parts(data, data_len as ffi::size_t) };
        controller.callbacks.invalid_packet_received(
            controller.get_id(),
            reason,
            message_str,
            data_slice,
        );
    }
}

// The trampoline function that is called by the C++ code.
extern "C" fn send_ll_trampoline(
    cookie: *mut c_void,
    data: *const u8,
    data_len: ffi::size_t,
    phy: c_int,
    tx_power: c_int,
) {
    // SAFETY: The FFI contract guarantees that the C++ side provides a valid
    // `cookie`.
    let context = unsafe { context_from_cookie(cookie) };
    if let Some(controller) = context.upgrade() {
        let phy = match Phy::try_from(phy) {
            Ok(phy) => phy,
            Err(err) => {
                warn!("Unknown PHY {err}");
                return;
            }
        };
        // SAFETY: The FFI contract guarantees that `data` points to a buffer of at
        // least `data_len` bytes that is valid for the duration of this call.
        let data_slice = unsafe { std::slice::from_raw_parts(data, data_len as ffi::size_t) };
        controller.ll_packets_out.fetch_add(1, Ordering::Relaxed);
        let is_p2p = netsim_packets::link_layer::fast_inspect_p2p_payload(data_slice);
        match phy {
            Phy::LowEnergy => {
                controller.ll_packets_out_ble.fetch_add(1, Ordering::Relaxed);
                if is_p2p {
                    controller.ble_p2p_tx_count.fetch_add(1, Ordering::Relaxed);
                }
            }
            _ => {
                controller.ll_packets_out_classic.fetch_add(1, Ordering::Relaxed);
                if is_p2p {
                    controller.classic_p2p_tx_count.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        // rootcanal request to send ll through bluetooth medium
        controller.bt_ops.broadcast_rootcanal_ll_packet(
            controller.get_id(),
            data_slice,
            phy,
            tx_power,
        );
    }
}

// The trampoline function that is called by the C++ code for ranging.
//
// # Safety
// `cookie` must be a valid pointer to a `CallbackContext` created by
// `ControllerImpl::new`. The `source_addr` and `destination_addr` must be valid
// pointers to 6-byte arrays representing Bluetooth addresses.
unsafe extern "C" fn ranging_estimator_trampoline(
    cookie: *mut c_void,
    source_addr: *const u8,
    destination_addr: *const u8,
) -> u32 {
    if cookie.is_null() {
        return 100;
    }

    // SAFETY: We verified `cookie` is not null. The C++ caller is contractually
    // bound to pass back the exact, unmodified `cookie` pointer that was
    // originally created by `Box::into_raw` and passed to `ffi_controller_new`.
    // This guarantees it is a valid pointer to a `CallbackContext`
    // (Weak<ControllerImpl>
    let context = unsafe { context_from_cookie(cookie) };

    if let Some(controller) = context.upgrade() {
        let source_id = controller.get_id();

        // SAFETY: The C++ FFI layer guarantees that `source_addr` and
        // `destination_addr` are valid, non-null pointers to 6-byte arrays
        // containing MAC addresses.
        let (source, destination): (&[u8; 6], &[u8; 6]) =
            unsafe { (&*(source_addr.cast::<[u8; 6]>()), &*(destination_addr.cast::<[u8; 6]>())) };

        controller.bt_ops.estimate_distance(source_id, source, destination)
    } else {
        100
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use pdl_runtime::Packet;

    use super::*;
    use crate::controller::Id as ControllerId;
    struct MockControllerCallbacks;
    impl Callbacks for MockControllerCallbacks {
        fn send_hci(&self, _source_id: Id, _data: Bytes) {}
        fn on_receive_ll(&self, _sender_id: Id, _packet: &[u8], _phy: Phy, _rssi: i32) {}
        fn invalid_packet_received(
            &self,
            _source_id: Id,
            _reason: c_int,
            _message: &str,
            _data: &[u8],
        ) {
        }
    }

    struct MockBtOps;
    impl BtOps for MockBtOps {
        fn broadcast_rootcanal_ll_packet(
            &self,
            _sender_id: ControllerId,
            _packet: &[u8],
            _phy: Phy,
            _tx_power: i32,
        ) {
        }

        fn estimate_distance(
            &self,
            _source_id: u32,
            _source_addr: &[u8; 6],
            _destination_addr: &[u8; 6],
        ) -> u32 {
            0
        }
    }

    #[test]
    fn test_controller_stats() {
        let address = Address::from_str("01:02:03:04:05:06").unwrap();
        let controller = ControllerImpl::new(
            1,
            address,
            Box::new(MockControllerCallbacks),
            Box::new(MockBtOps),
            None,
        );

        // Check initial stats.
        assert_eq!(controller.get_stats(), Stats::default());

        // Modify some stats.
        controller.hci_commands_in.fetch_add(1, Ordering::Relaxed);
        controller.ll_packets_out.fetch_add(5, Ordering::Relaxed);

        let stats = controller.get_stats();
        assert_eq!(stats.hci_commands_in, 1);
        assert_eq!(stats.ll_packets_out, 5);
        assert_eq!(stats.hci_events_out, 0);

        // Clear stats.
        controller.clear_stats();
        assert_eq!(controller.get_stats(), Stats::default());
    }

    #[test]
    fn test_receive_hci_increments_counter() {
        let address = Address::from_str("01:02:03:04:05:06").unwrap();
        let controller = ControllerImpl::new(
            1,
            address,
            Box::new(MockControllerCallbacks),
            Box::new(MockBtOps),
            None,
        );

        assert_eq!(controller.get_stats().hci_commands_in, 0);
        controller.receive_hci(Bytes::from_static(&[1, 1, 2, 3]));
        assert_eq!(controller.get_stats().hci_commands_in, 1);
    }

    #[test]
    fn test_receive_ll_increments_counter() {
        let address = Address::from_str("01:02:03:04:05:06").unwrap();
        let controller = ControllerImpl::new(
            1,
            address,
            Box::new(MockControllerCallbacks),
            Box::new(MockBtOps),
            None,
        );

        assert_eq!(controller.get_stats().ll_packets_in, 0);
        controller.receive_ll(&[1, 2, 3], Phy::LowEnergy, -80);
        assert_eq!(controller.get_stats().ll_packets_in, 1);
    }

    #[test]
    fn test_receive_ll_p2p_counter() {
        let address = Address::from_str("01:02:03:04:05:06").unwrap();
        let controller = ControllerImpl::new(
            1,
            address,
            Box::new(MockControllerCallbacks),
            Box::new(MockBtOps),
            None,
        );

        use netsim_packets::link_layer::{Acl, Address as LlAddress};

        let src = LlAddress::try_from(1).unwrap();
        let dest = LlAddress::try_from(2).unwrap();

        assert_eq!(controller.get_stats().ble_p2p_rx_count, 0);
        assert_eq!(controller.get_stats().classic_p2p_rx_count, 0);

        // Payload-bearing empty PDU (ACL empty)
        let acl_empty = Acl {
            source_address: src,
            destination_address: dest,
            packet_boundary_flag: 0,
            broadcast_flag: 0,
            data: vec![],
        };
        let mut acl_empty_bytes = Vec::new();
        acl_empty.encode(&mut acl_empty_bytes).unwrap();

        controller.receive_ll(&acl_empty_bytes, Phy::LowEnergy, -80);
        assert_eq!(controller.get_stats().ble_p2p_rx_count, 0);
        assert_eq!(controller.get_stats().classic_p2p_rx_count, 0);

        // Payload-bearing PDU (ACL payload) - BLE
        let acl_payload = Acl {
            source_address: src,
            destination_address: dest,
            packet_boundary_flag: 0,
            broadcast_flag: 0,
            data: vec![1, 2, 3],
        };
        let mut acl_payload_bytes = Vec::new();
        acl_payload.encode(&mut acl_payload_bytes).unwrap();

        controller.receive_ll(&acl_payload_bytes, Phy::LowEnergy, -80);
        assert_eq!(controller.get_stats().ble_p2p_rx_count, 1);
        assert_eq!(controller.get_stats().classic_p2p_rx_count, 0);

        // Payload-bearing PDU (ACL payload) - Classic
        controller.receive_ll(&acl_payload_bytes, Phy::BrEdr, -80);
        assert_eq!(controller.get_stats().ble_p2p_rx_count, 1);
        assert_eq!(controller.get_stats().classic_p2p_rx_count, 1);
    }
}
