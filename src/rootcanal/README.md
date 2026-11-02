# Netsim Rootcanal RS

This crate provides a safe Rust interface to the C++ `rootcanal` library through the `rootcanal-ffi` interface. It is designed to be used in testing and simulation scenarios for Bluetooth controllers.

## Features

- Safe, idiomatic Rust interface for the `rootcanal` library.
- Create and manage multiple Bluetooth controllers.
- Send and receive HCI and Link Layer packets.
- A callback-based API for handling events from the controllers.
- Packet statistics for monitoring the state of the controllers.

## Usage

Here is an example of how to use this crate to create a `Rootcanal` instance, add two controllers, and send a packet from one to the other.

First, you need to implement the `bluetooth::Callbacks` and `controller::Callbacks` traits to handle events from the `Rootcanal` instance and the `Controller` instances.

```rust
use netsim_rootcanal_rs::{
    rootcanal::{self, Rootcanal},
    controller,
    types::{Address, Idc, Phy},
};
use std::ffi::c_int;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Callbacks for the Bluetooth instance.
struct MyBluetoothCallbacks;
impl bluetooth::Callbacks for MyBluetoothCallbacks {
    fn on_send_ll_packet(
        &self,
        _source_id: u32,
        _destination_id: u32,
        _phy: Phy,
        tx_power: i32,
    ) -> Option<i32> {
        // Allow the packet to be sent with the given tx_power.
        Some(tx_power)
    }
}

// Callbacks for the Controller instances.
struct MyControllerCallbacks {
    packet_received: AtomicBool,
}

impl controller::Callbacks for MyControllerCallbacks {
    fn send_hci(&self, address: Address, idc: Idc, data: Vec<u8>) {
        println!(
            "HCI packet received from controller {}: {:?} {:?}",
            address, idc, data
        );
        self.packet_received.store(true, Ordering::Relaxed);
    }

    fn invalid_packet_received(
        &self,
        address: Address,
        reason: c_int,
        message: &str,
        data: &[u8],
    ) {
        println!(
            "Invalid packet received from controller {}: reason={}, message='{}', data={:?}",
            address, reason, message, data
        );
    }
}

// Create a Bluetooth instance.
let rootcanal = Rootcanal::new(Box::new(MyBluetoothCallbacks));

// Create two controllers.
let callbacks1 = Arc::new(MyControllerCallbacks {
    packet_received: AtomicBool::new(false),
});
let addr1 = Address::from_str("01:02:03:04:05:06").unwrap();
bluetooth
    .new_controller(1, addr1, Box::new(callbacks1.clone()))
    .unwrap();

let callbacks2 = Arc::new(MyControllerCallbacks {
    packet_received: AtomicBool::new(false),
});
let addr2 = Address::from_str("AA:BB:CC:DD:EE:FF").unwrap();
bluetooth
    .new_controller(2, addr2, Box::new(callbacks2.clone()))
    .unwrap();

// Send a packet from controller 1. This will be received by controller 2.
// In a real scenario, you would send a valid HCI command packet.
let packet = vec![0x01, 0x03, 0x0c, 0x00];
bluetooth.receive_hci(1, Idc::Cmd, &packet);

// Advance the state of the controllers.
bluetooth.tick();

// The packet should have been received by controller 2.
assert!(callbacks2.packet_received.load(Ordering::Relaxed));

```

## Building

This crate is intended to be built as part of the larger `netsim` project.

## Testing

To run the tests for this crate, use the following command from the root of the `netsim` project:

```bash
./scripts/build_tools.py --task test --crate netsim-rootcanal-rs
```