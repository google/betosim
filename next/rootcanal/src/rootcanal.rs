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

//! This module defines common Bluetooth data types, such as addresses.

use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use bytes::Bytes;
use parking_lot::Mutex;

use crate::{
    controller::{
        BtOps, Callbacks as ControllerCallbacks, Controller, ControllerImpl, Id as ControllerId,
        Stats,
    },
    error::{Error, Result},
    types::{Address, Phy},
};

/// The callbacks for Bluetooth
pub trait Callbacks: Send + Sync {
    /// Intercept and optionally rewrite a link layer packet before it is sent.
    ///
    /// The `source_id` is the id of the controller sending the packet.
    /// The destination_id is the id of the controller receiving the packet.
    ///
    /// To modify the tx_power, return `Some(new_tx_power)`.
    /// To drop the packet, return `None`.
    fn on_send_ll(
        &self,
        source_id: ControllerId,
        destination_id: ControllerId,
        packet: &[u8],
        phy: Phy,
        tx_power: i32,
    ) -> Option<i32>;

    /// Called by the controller to estimate distance to a destination address.
    fn estimate_distance(&self, source_id: u32, destination_id: u32) -> u32;
}

/// The Bluetooth subsystem.
pub struct Rootcanal {
    controllers: Mutex<HashMap<ControllerId, Controller>>,
    callbacks: Box<dyn Callbacks>,
    disable_address_reuse: bool,
    on_packet: Box<dyn Fn(ControllerId, Bytes, Phy, i32) + Send + Sync>,
}

// A wrapper around the Bluetooth ops that a controller uses.  It
// holds a weak reference to the Bluetooth subsystem. This allows the
// controller to send link layer packets without holding a strong
// reference to the Bluetooth instance, preventing reference cycles.
struct BtOpsWrapper {
    bluetooth: Weak<Rootcanal>,
}

impl BtOps for BtOpsWrapper {
    // controller requests to send ll to peers
    fn broadcast_rootcanal_ll_packet(
        &self,
        sender_id: ControllerId,
        packet: &[u8],
        phy: Phy,
        tx_power: i32,
    ) {
        if let Some(bluetooth) = self.bluetooth.upgrade() {
            // Forwards a link layer packet to all other controllers. Used by
            // rootcanal peer messages from a controller.
            bluetooth.broadcast_to_peers(sender_id, packet, phy, tx_power);
        }
    }

    fn estimate_distance(
        &self,
        source_id: u32,
        source_addr: &[u8; 6],
        destination_addr: &[u8; 6],
    ) -> u32 {
        if let Some(bluetooth) = self.bluetooth.upgrade() {
            for controller in bluetooth.cloned_controllers() {
                if controller.get_id() == source_id {
                    continue;
                }

                if controller.has_le_connection(source_addr, destination_addr) {
                    let peer_id = controller.get_id();
                    return bluetooth.callbacks.estimate_distance(source_id, peer_id);
                }
            }
        }
        100
    }
}

extern "C" fn rootcanal_log_callback(
    level: std::os::raw::c_int,
    message: *const std::os::raw::c_char,
) {
    if message.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(|| {
        // SAFETY: Rootcanal passes a valid null-terminated C string pointing to the
        // formatted log message.
        let c_str = unsafe { std::ffi::CStr::from_ptr(message) };
        let msg = c_str.to_string_lossy();

        // Rootcanal log levels match rootcanal::log::Verbosity:
        // 0=kDebug, 1=kInfo, 2=kWarning, 3=kError, 4=kFatal
        match level {
            0 => tracing::debug!(target: "rootcanal", "{msg}"),
            1 => tracing::info!(target: "rootcanal", "{msg}"),
            2 => tracing::warn!(target: "rootcanal", "{msg}"),
            3 => tracing::error!(target: "rootcanal", "{msg}"),
            _ => tracing::error!(target: "rootcanal", "{msg}"),
        }
    });
}

/// Configures rootcanal logging to dispatch through Rust's `tracing` framework.
///
/// This routes rootcanal logs through active tracing subscribers and test
/// writers, preventing direct stdout/stderr pollution.
fn init_rootcanal_logging() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        // SAFETY: `ffi_set_log_callback` registers a static extern "C" function
        // pointer. The callback performs safe string conversion and
        // dispatches to `tracing` macros.
        unsafe {
            crate::ffi::ffi_set_log_callback(Some(rootcanal_log_callback));
        }
    });
}

impl Rootcanal {
    /// Creates a new Bluetooth subsystem.
    pub fn new(
        callbacks: Box<dyn Callbacks>,
        disable_address_reuse: bool,
        on_packet: Box<dyn Fn(ControllerId, Bytes, Phy, i32) + Send + Sync>,
    ) -> Arc<Self> {
        init_rootcanal_logging();
        Arc::new(Self {
            controllers: Mutex::new(HashMap::new()),
            callbacks,
            disable_address_reuse,
            on_packet,
        })
    }

    /// Creates a new Bluetooth controller with a unique id and possibly
    /// non-unique address
    pub fn add_controller(
        self: &Arc<Self>,
        id: ControllerId,
        address: Address,
        callbacks: Box<dyn ControllerCallbacks>,
        properties: Option<&[u8]>,
    ) -> Result<()> {
        let mut controllers = self.controllers.lock();
        if controllers.contains_key(&id) {
            return Err(Error::DuplicateControllerId(id));
        }
        if self.disable_address_reuse && controllers.values().any(|c| c.get_address() == address) {
            return Err(Error::AddressInUse(address));
        }
        let bt_ops = Box::new(BtOpsWrapper { bluetooth: Arc::downgrade(self) });
        let controller = ControllerImpl::new(id, address, callbacks, bt_ops, properties);
        controllers.insert(id, controller);
        Ok(())
    }

    /// Creates a new Bluetooth controller with a unique id and possibly
    /// non-unique address
    pub fn new_controller(
        self: &Arc<Self>,
        id: ControllerId,
        address: Address,
        callbacks: Box<dyn ControllerCallbacks>,
        properties: Option<&[u8]>,
        // for when controller sends ll to other controllers
    ) -> Result<()> {
        self.add_controller(id, address, callbacks, properties)
    }

    /// Removes a Bluetooth controller.
    pub fn remove_controller(&self, id: ControllerId) -> Result<()> {
        let controller = self.controllers.lock().remove(&id);
        controller.ok_or(Error::ControllerNotFound(id)).map(|_| ())
    }

    /// Injects a link layer packet from an external source into the simulation.
    /// This is intended for use by test clients.
    pub fn inject_ll_packet(&self, sender_id: ControllerId, packet: &[u8], phy: Phy, rssi: i32) {
        // Pass network level packet to the external client.
        self.broadcast_to_peers(sender_id, packet, phy, rssi);
    }

    fn broadcast_to_peers(&self, sender_id: ControllerId, packet: &[u8], phy: Phy, rssi: i32) {
        for controller in self.cloned_controllers() {
            let receiver_id = controller.get_id();
            if receiver_id != sender_id {
                if let Some(new_rssi) =
                    self.callbacks.on_send_ll(sender_id, receiver_id, packet, phy, rssi)
                {
                    // sniffer controllers want to track packets received
                    controller.callbacks.on_receive_ll(sender_id, packet, phy, new_rssi);
                    (self.on_packet)(receiver_id, Bytes::copy_from_slice(packet), phy, new_rssi);
                } else {
                    controller.increment_ll_packets_dropped();
                }
            }
        }
    }

    /// Delivers a packet to a specific controller.
    pub fn deliver_packet(&self, receiver_id: ControllerId, packet: &[u8], phy: Phy, rssi: i32) {
        let controller = self.controllers.lock().get(&receiver_id).cloned();
        if let Some(controller) = controller {
            controller.receive_ll(packet, phy, rssi);
        }
    }

    // Use to get a copy of controllers without holding the lock
    fn cloned_controllers(&self) -> Vec<Controller> {
        self.controllers.lock().values().cloned().collect()
    }

    /// Advances the state of all controllers by one tick.
    pub fn tick(&self) {
        for controller in self.cloned_controllers() {
            controller.tick();
        }
    }

    /// Returns the number of controllers.
    pub fn len(&self) -> usize {
        self.controllers.lock().len()
    }

    /// Returns true if there are no controllers.
    pub fn is_empty(&self) -> bool {
        self.controllers.lock().is_empty()
    }

    /// Returns a vector of the current controller IDs.
    #[cfg(test)]
    fn get_controller_ids(&self) -> Vec<ControllerId> {
        let mut keys: Vec<ControllerId> = self.controllers.lock().keys().copied().collect();
        keys.sort();
        keys
    }

    /// Receives an HCI packet from the host for a specific controller.
    pub fn receive_hci(&self, controller_id: ControllerId, h4_packet: Bytes) -> Result<()> {
        let controller = self
            .controllers
            .lock()
            .get(&controller_id)
            .cloned()
            .ok_or(Error::ControllerNotFound(controller_id))?;
        controller.receive_hci(h4_packet);
        Ok(())
    }

    /// Returns the address of a specific controller.
    pub fn get_address(&self, controller_id: ControllerId) -> Result<Address> {
        self.controllers
            .lock()
            .get(&controller_id)
            .map(|c| c.get_address())
            .ok_or(Error::ControllerNotFound(controller_id))
    }

    /// Returns the current packet statistics for a specific controller.
    pub fn get_stats(&self, controller_id: ControllerId) -> Result<Stats> {
        self.controllers
            .lock()
            .get(&controller_id)
            .map(|c| c.get_stats())
            .ok_or(Error::ControllerNotFound(controller_id))
    }

    /// Clears the packet statistics for a specific controller.
    pub fn clear_stats(&self, controller_id: ControllerId) -> Result<()> {
        self.controllers
            .lock()
            .get(&controller_id)
            .ok_or(Error::ControllerNotFound(controller_id))
            .map(|controller| controller.clear_stats())
    }

    /// Reconfigures a specific controller's properties.
    pub fn set_properties(&self, controller_id: ControllerId, properties: &[u8]) -> Result<()> {
        let controller = self
            .controllers
            .lock()
            .get(&controller_id)
            .cloned()
            .ok_or(Error::ControllerNotFound(controller_id))?;
        controller.set_properties(properties)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::c_int,
        str::FromStr,
        sync::atomic::{AtomicU32, Ordering},
    };

    use super::*;
    use crate::types::Address;

    struct MockCallbacks {
        drop_packet: bool,
        packets_sent: AtomicU32,
    }

    impl Callbacks for MockCallbacks {
        fn on_send_ll(
            &self,
            _source_id: ControllerId,
            _destination_id: ControllerId,
            _packet: &[u8],
            _phy: Phy,
            tx_power: i32,
        ) -> Option<i32> {
            self.packets_sent.fetch_add(1, Ordering::Relaxed);
            if self.drop_packet { None } else { Some(tx_power) }
        }

        fn estimate_distance(&self, _source_id: u32, _destination_id: u32) -> u32 {
            0
        }
    }

    struct MockControllerCallbacks;
    impl ControllerCallbacks for MockControllerCallbacks {
        fn send_hci(&self, _source_id: ControllerId, _h4_packet: bytes::Bytes) {}
        fn on_receive_ll(&self, _sender_id: ControllerId, _packet: &[u8], _phy: Phy, _rssi: i32) {}
        fn invalid_packet_received(
            &self,
            _source_id: ControllerId,
            _reason: c_int,
            _message: &str,
            _data: &[u8],
        ) {
        }
    }

    /// Test helper to create a Bluetooth instance and a specified number of
    /// controllers.
    fn setup_bluetooth_with_controllers(bluetooth: &Arc<Rootcanal>, num_controllers: u32) {
        for i in 1..=num_controllers {
            let addr = Address::from_str(&format!("01:02:03:04:05:{:02X}", i)).unwrap();
            bluetooth.add_controller(i, addr, Box::new(MockControllerCallbacks {}), None).unwrap();
        }
    }

    #[test]
    fn test_address_from_str_valid() {
        let addr_str = "01:23:45:67:89:AB";
        let addr = Address::from_str(addr_str).unwrap();
        assert_eq!(addr.address, [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB]);
    }

    #[test]
    fn test_address_from_str_invalid_length() {
        let addr_str = "01:23:45:67:89";
        assert!(Address::from_str(addr_str).is_err());
    }

    #[test]
    fn test_address_from_str_invalid_chars() {
        let addr_str = "01:23:45:67:89:XX";
        assert!(Address::from_str(addr_str).is_err());
    }

    #[test]
    fn test_address_types() {
        let resolvable = Address::from_str("41:23:45:67:89:AB").unwrap();
        assert!(resolvable.is_resolvable());

        let non_resolvable = Address::from_str("01:23:45:67:89:AB").unwrap();
        assert!(non_resolvable.is_non_resolvable());

        let static_identity = Address::from_str("C1:23:45:67:89:AB").unwrap();
        assert!(static_identity.is_static_identity());
    }

    #[test]
    fn test_send_ll_packet() {
        let callbacks =
            Box::new(MockCallbacks { drop_packet: false, packets_sent: AtomicU32::new(0) });
        let bluetooth_weak = Arc::new(Mutex::new(Weak::<Rootcanal>::new()));
        let bluetooth_weak_clone = bluetooth_weak.clone();
        let on_packet = Box::new(move |receiver_id, packet: Bytes, phy, rssi| {
            let bluetooth = bluetooth_weak_clone.lock().upgrade();
            if let Some(bluetooth) = bluetooth {
                bluetooth.deliver_packet(receiver_id, &packet, phy, rssi);
            }
        });
        let bluetooth = Rootcanal::new(callbacks, false, on_packet);
        *bluetooth_weak.lock() = Arc::downgrade(&bluetooth);
        setup_bluetooth_with_controllers(&bluetooth, 2);

        // TODO: Send valid link layer packets.
        bluetooth.inject_ll_packet(1, &[1, 2, 3], Phy::LowEnergy, -80);

        // Controller 1 should not receive its own packet.
        assert_eq!(bluetooth.get_stats(1).unwrap().ll_packets_in, 0);
        // Controller 2 should receive the packet.
        assert_eq!(bluetooth.get_stats(2).unwrap().ll_packets_in, 1);
    }

    #[test]
    fn test_send_ll_packet_dropped() {
        let callbacks =
            Box::new(MockCallbacks { drop_packet: true, packets_sent: AtomicU32::new(0) });
        let bluetooth = Rootcanal::new(callbacks, false, Box::new(|_, _, _, _| {}));
        setup_bluetooth_with_controllers(&bluetooth, 2);

        // TODO: Send valid link layer packets.
        bluetooth.inject_ll_packet(1, &[1, 2, 3], Phy::LowEnergy, -80);

        assert_eq!(bluetooth.get_stats(1).unwrap().ll_packets_in, 0);
        assert_eq!(bluetooth.get_stats(2).unwrap().ll_packets_in, 0);
        assert_eq!(bluetooth.get_stats(1).unwrap().ll_packets_dropped, 0);
        assert_eq!(bluetooth.get_stats(2).unwrap().ll_packets_dropped, 1);
    }

    #[test]
    fn test_delete_controller() {
        let callbacks =
            Box::new(MockCallbacks { drop_packet: false, packets_sent: AtomicU32::new(0) });
        let bluetooth = Rootcanal::new(callbacks, false, Box::new(|_, _, _, _| {}));
        setup_bluetooth_with_controllers(&bluetooth, 2);

        assert_eq!(bluetooth.len(), 2);

        assert!(bluetooth.remove_controller(1).is_ok());
        assert_eq!(bluetooth.len(), 1);
        assert_eq!(bluetooth.get_controller_ids(), vec![2]);

        // This call should not panic.
        // TODO: Send valid link layer packets.
        bluetooth.inject_ll_packet(2, &[4, 5, 6], Phy::LowEnergy, -70);
    }

    #[test]
    fn test_delete_controller_invalid_id() {
        let callbacks =
            Box::new(MockCallbacks { drop_packet: false, packets_sent: AtomicU32::new(0) });
        let bluetooth = Rootcanal::new(callbacks, false, Box::new(|_, _, _, _| {}));
        setup_bluetooth_with_controllers(&bluetooth, 1);

        let result = bluetooth.remove_controller(2);
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::ControllerNotFound(id) => assert_eq!(id, 2),
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn test_add_controller_duplicate_id() {
        let callbacks =
            Box::new(MockCallbacks { drop_packet: false, packets_sent: AtomicU32::new(0) });
        let bluetooth = Rootcanal::new(callbacks, false, Box::new(|_, _, _, _| {}));
        setup_bluetooth_with_controllers(&bluetooth, 1);

        let addr = Address::from_str("01:02:03:04:05:06").unwrap();

        let result = bluetooth.add_controller(1, addr, Box::new(MockControllerCallbacks {}), None);

        assert!(result.is_err());
        match result.err().unwrap() {
            Error::DuplicateControllerId(id) => assert_eq!(id, 1),
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn test_receive_hci_invalid_controller() {
        let callbacks =
            Box::new(MockCallbacks { drop_packet: false, packets_sent: AtomicU32::new(0) });
        let bluetooth = Rootcanal::new(callbacks, false, Box::new(|_, _, _, _| {}));
        setup_bluetooth_with_controllers(&bluetooth, 1);

        let result = bluetooth.receive_hci(2, Bytes::from_static(&[1, 1, 2, 3]));
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::ControllerNotFound(id) => assert_eq!(id, 2),
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn test_clear_stats() {
        let callbacks =
            Box::new(MockCallbacks { drop_packet: false, packets_sent: AtomicU32::new(0) });
        let bluetooth = Rootcanal::new(callbacks, false, Box::new(|_, _, _, _| {}));
        setup_bluetooth_with_controllers(&bluetooth, 1);

        // Clear stats for a valid controller.
        assert!(bluetooth.clear_stats(1).is_ok());

        // Clear stats for an invalid controller.
        let result = bluetooth.clear_stats(2);
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::ControllerNotFound(id) => assert_eq!(id, 2),
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn test_get_address_invalid_id() {
        let callbacks =
            Box::new(MockCallbacks { drop_packet: false, packets_sent: AtomicU32::new(0) });
        let bluetooth = Rootcanal::new(callbacks, false, Box::new(|_, _, _, _| {}));
        setup_bluetooth_with_controllers(&bluetooth, 1);

        let result = bluetooth.get_address(2);
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::ControllerNotFound(id) => assert_eq!(id, 2),
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn test_get_stats_invalid_id() {
        let callbacks =
            Box::new(MockCallbacks { drop_packet: false, packets_sent: AtomicU32::new(0) });
        let bluetooth = Rootcanal::new(callbacks, false, Box::new(|_, _, _, _| {}));
        setup_bluetooth_with_controllers(&bluetooth, 1);

        let result = bluetooth.get_stats(2);
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::ControllerNotFound(id) => assert_eq!(id, 2),
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn test_disable_address_reuse() {
        let callbacks =
            Box::new(MockCallbacks { drop_packet: false, packets_sent: AtomicU32::new(0) });
        // Create with disable_address_reuse = true
        let bluetooth = Rootcanal::new(callbacks, true, Box::new(|_, _, _, _| {}));

        let addr = Address::from_str("01:02:03:04:05:06").unwrap();

        // Adding first controller should succeed
        assert!(
            bluetooth.add_controller(1, addr, Box::new(MockControllerCallbacks {}), None).is_ok()
        );

        // Adding second controller with SAME address should fail
        let result = bluetooth.add_controller(2, addr, Box::new(MockControllerCallbacks {}), None);
        assert!(result.is_err());
        match result.err().unwrap() {
            Error::AddressInUse(a) => assert_eq!(a, addr),
            _ => panic!("unexpected error type"),
        }
    }
}
