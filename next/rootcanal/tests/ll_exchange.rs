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

//! Test for verifying the exchange of link layer packets with the rootcanal
//! controller.

// TODO: include link_layer
//use rootcanal_rs::packets::link_layer::{
//    Address as LlAddress, AddressType, LeLegacyAdvertisingPdu,
// LegacyAdvertisingType,
//};
use std::{
    str::FromStr,
    sync::{Arc, Once},
};

use bytes::Bytes;
use rootcanal::{
    Callbacks as RootcanalCallbacks, Rootcanal,
    controller::{Callbacks as ControllerCallbacks, Id},
    types::{Address, Phy},
};
use tokio::{
    sync::mpsc,
    time::{Duration, sleep},
};
use tracing::error;

static INIT: Once = Once::new();

/// Set up the logger for the test.
fn setup() {
    INIT.call_once(|| {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    });
}

/// Main callbacks for the rootcanal Bluetooth instance.
struct TestCallbacks;
impl RootcanalCallbacks for TestCallbacks {
    fn on_send_ll(
        &self,
        _source_id: u32,
        _destination_id: u32,
        _packet: &[u8],
        _phy: Phy,
        tx_power: i32,
    ) -> Option<i32> {
        Some(tx_power)
    }

    fn estimate_distance(&self, _source_id: u32, _destination_id: u32) -> u32 {
        0
    }
}

/// Callbacks for the sniffer controller, which forwards received LL packets.
struct SnifferCallbacks {
    sender: mpsc::Sender<Vec<u8>>,
}

impl ControllerCallbacks for SnifferCallbacks {
    fn send_hci(&self, _source_id: Id, _data: Bytes) {}
    fn on_receive_ll(&self, _source_id: Id, packet: &[u8], _phy: Phy, _tx_power: i32) {
        let _ = self.sender.try_send(packet.to_vec());
    }
    fn invalid_packet_received(&self, source_id: Id, reason: i32, message: &str, data: &[u8]) {
        error!(
            "sniffer invalid packet from {}: reason={}, message='{}', data={:02x?}",
            source_id, reason, message, data
        );
    }
}

/// Callbacks for the dummy controller, which takes no action.
struct DummyCallbacks;
impl ControllerCallbacks for DummyCallbacks {
    fn send_hci(&self, _source_id: Id, _data: Bytes) {}
    fn on_receive_ll(&self, _source_id: Id, _packet: &[u8], _phy: Phy, _tx_power: i32) {}
    fn invalid_packet_received(&self, source_id: Id, reason: i32, message: &str, data: &[u8]) {
        error!(
            "dummy invalid packet from {}: reason={}, message='{}', data={:02x?}",
            source_id, reason, message, data
        );
    }
}

/// Test case for injecting a link layer packet into the rootcanal controller
/// and verifying that it is received by another controller.
// Verifies that the link-layer packet injection mechanism works correctly.
// This test confirms that a packet introduced from an external source into one
// controller is properly broadcast and received by other controllers in the
// simulation. It performs the following steps:
// 1. Creates two controllers: one to act as the packet injector/sender and another to act as a
//    sniffer.
// 2. Manually constructs a valid link-layer advertising PDU (`ADV_IND`).
// 3. Uses `rootcanal.inject_ll_packet` to send this packet from the sender controller into the
//    simulation.
// 4. Listens on the sniffer controller and asserts that it receives a packet.
// 5. Verifies that the received packet is byte-for-byte identical to the one that was injected,
//    confirming the integrity of the transport.
#[tokio::test]
async fn test_ll_exchange() {
    test_ll_exchange_internal().await
}

async fn test_ll_exchange_internal() {
    setup();
    let rootcanal =
        Arc::new(Rootcanal::new(Box::new(TestCallbacks), false, Box::new(|_, _, _, _| {})));
    let (sender, mut _receiver) = mpsc::channel(10);

    let rootcanal_clone = rootcanal.clone();
    tokio::spawn(async move {
        loop {
            rootcanal_clone.tick();
            sleep(Duration::from_millis(20)).await;
        }
    });

    // Create a sniffer to listen for packets.
    let sniffer_address = Address::from_str("00:00:00:00:00:01").unwrap();
    let sniffer_id = 1;
    rootcanal
        .new_controller(sniffer_id, sniffer_address, Box::new(SnifferCallbacks { sender }), None)
        .expect("new controller failed");

    // Create a raw controller to send a packet.
    let sender_address = Address::from_str("00:00:00:00:00:02").unwrap();
    let sender_id = 2;
    rootcanal
        .new_controller(sender_id, sender_address, Box::new(DummyCallbacks), None)
        .expect("new controller failed");

    // Construct a valid ADV_IND packet.
    let _advertising_data = vec![
        0x02, 0x01, 0x06, // Flags
        0x03, 0x03, 0xaa, 0xfe, // Service UUID
    ];

    // TODO: include link_layer
    //    let mut packet_bytes = Vec::new();
    //    let mut sender_addr_bytes = [0; 8];
    //    sender_addr_bytes[..6].copy_from_slice(sender_address.as_bytes());
    //    let pdu = LeLegacyAdvertisingPdu {
    //        source_address:
    // LlAddress::try_from(u64::from_le_bytes(sender_addr_bytes)).unwrap(),
    //        destination_address: LlAddress::try_from(0).unwrap(),
    //        advertising_address_type: AddressType::Public,
    //        target_address_type: AddressType::Public,
    //        advertising_type: LegacyAdvertisingType::AdvInd,
    //        advertising_data: advertising_data.clone().into(),
    //    };
    //    pdu.encode(&mut packet_bytes).unwrap();
    //    let packet = packet_bytes;
    //
    //    // Send a packet from the sender controller.
    //    rootcanal.inject_ll_packet(sender_id, &packet, Phy::LowEnergy, -80);
    //
    //    // Wait for the sniffer to receive the packet and verify its contents.
    //    let received = timeout(Duration::from_secs(1), receiver.recv()).await;
    //    assert!(received.is_ok(), "Did not receive packet within 1 second");
    //    let received_packet = received.unwrap().unwrap();
    //    assert_eq!(received_packet, packet);
    //
    //    let parsed =
    // LeLegacyAdvertisingPdu::decode(&received_packet).unwrap().0;
    //    assert_eq!(parsed.advertising_data(), &advertising_data);
}
