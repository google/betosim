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

//! BDD World test fixture for Rootcanal integration tests.

use std::{
    str::FromStr,
    sync::{Arc, Once},
    time::Duration,
};

use bytes::Bytes;
use netsim_packets::link_layer::{
    Address as LlAddress, AddressType, LeLegacyAdvertisingPdu, LegacyAdvertisingType,
};
use pdl_runtime::Packet;
use rootcanal::{
    Callbacks as RootcanalCallbacks, Rootcanal,
    controller::{Callbacks as ControllerCallbacks, Id},
    types::{Address, Phy},
};
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{sleep, timeout},
};
use tracing::error;

pub(crate) const HCI_RESET: u16 = 0x0c03;
pub(crate) const LE_SET_ADVERTISING_PARAMETERS: u16 = 0x2006;
pub(crate) const LE_SET_ADVERTISING_DATA: u16 = 0x2008;
pub(crate) const LE_SET_ADVERTISING_ENABLE: u16 = 0x200a;

const HCI_EVENT_TIMEOUT: Duration = Duration::from_secs(1);
const LL_PACKET_TIMEOUT: Duration = Duration::from_secs(5);

static INIT: Once = Once::new();

fn setup() {
    INIT.call_once(|| {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    });
}

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

struct DutCallbacks {
    sender: mpsc::Sender<Vec<u8>>,
}

impl ControllerCallbacks for DutCallbacks {
    fn send_hci(&self, _source_id: Id, data: Bytes) {
        let _ = self.sender.try_send(data.to_vec());
    }

    fn on_receive_ll(&self, _source_id: Id, _packet: &[u8], _phy: Phy, _tx_power: i32) {}

    fn invalid_packet_received(&self, source_id: Id, reason: i32, message: &str, data: &[u8]) {
        error!(
            "dut invalid packet from {}: reason={}, message='{}', data={:02x?}",
            source_id, reason, message, data
        );
    }
}

pub(crate) struct World {
    rootcanal: Arc<Rootcanal>,
    ticker_handle: JoinHandle<()>,
    ll_tx: mpsc::Sender<Vec<u8>>,
    ll_rx: mpsc::Receiver<Vec<u8>>,
    hci_tx: mpsc::Sender<Vec<u8>>,
    hci_rx: mpsc::Receiver<Vec<u8>>,
    last_injected_packet: Option<Vec<u8>>,
}

impl Drop for World {
    fn drop(&mut self) {
        self.ticker_handle.abort();
    }
}

impl World {
    pub(crate) fn new() -> Self {
        setup();
        let rootcanal = Rootcanal::new(Box::new(TestCallbacks), false, Box::new(|_, _, _, _| {}));
        let (ll_tx, ll_rx) = mpsc::channel(10);
        let (hci_tx, hci_rx) = mpsc::channel(10);

        let rootcanal_clone = Arc::clone(&rootcanal);
        let ticker_handle = tokio::spawn(async move {
            loop {
                rootcanal_clone.tick();
                sleep(Duration::from_millis(20)).await;
            }
        });

        Self { rootcanal, ticker_handle, ll_tx, ll_rx, hci_tx, hci_rx, last_injected_packet: None }
    }

    fn send_hci_command(&self, controller_id: Id, opcode: u16, params: &[u8]) {
        let [lo, hi] = opcode.to_le_bytes();
        let param_len = u8::try_from(params.len()).expect("HCI command parameters must fit in u8");
        let mut cmd = Vec::with_capacity(4 + params.len());
        cmd.extend_from_slice(&[0x01, lo, hi, param_len]);
        cmd.extend_from_slice(params);
        self.rootcanal
            .receive_hci(controller_id, Bytes::from(cmd))
            .expect("receive_hci should succeed");
    }

    fn controller_ll_address(&self, id: Id) -> LlAddress {
        let addr = self.rootcanal.get_address(id).expect("controller should be registered");
        let mut addr_bytes = [0u8; 8];
        addr_bytes[..6].copy_from_slice(addr.as_bytes());
        LlAddress::try_from(u64::from_le_bytes(addr_bytes)).expect("valid 48-bit Bluetooth address")
    }

    pub(crate) fn given_sniffer_controller(&self, id: Id, address_str: &str) {
        let address = Address::from_str(address_str).expect("valid Bluetooth address");
        self.rootcanal
            .new_controller(
                id,
                address,
                Box::new(SnifferCallbacks { sender: self.ll_tx.clone() }),
                None,
            )
            .expect("new controller should succeed");
    }

    pub(crate) fn given_dut_controller(&self, id: Id, address_str: &str) {
        let address = Address::from_str(address_str).expect("valid Bluetooth address");
        self.rootcanal
            .new_controller(
                id,
                address,
                Box::new(DutCallbacks { sender: self.hci_tx.clone() }),
                None,
            )
            .expect("new controller should succeed");
    }

    pub(crate) fn when_send_hci_reset(&self, controller_id: Id) {
        self.send_hci_command(controller_id, HCI_RESET, &[]);
    }

    pub(crate) fn when_set_le_advertising_parameters(&self, controller_id: Id) {
        self.send_hci_command(
            controller_id,
            LE_SET_ADVERTISING_PARAMETERS,
            &[
                0xA0, 0x00, 0xA0, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07,
                0x00,
            ],
        );
    }

    pub(crate) fn when_set_le_advertising_data(&self, controller_id: Id, data: &[u8]) {
        assert!(data.len() <= 31, "LE advertising data must be at most 31 bytes");
        let mut params = Vec::with_capacity(32);
        params.push(u8::try_from(data.len()).expect("advertising data length fits in u8"));
        params.extend_from_slice(data);
        params.resize(32, 0);
        self.send_hci_command(controller_id, LE_SET_ADVERTISING_DATA, &params);
    }

    pub(crate) fn when_set_le_advertising_enable(&self, controller_id: Id, enable: bool) {
        self.send_hci_command(controller_id, LE_SET_ADVERTISING_ENABLE, &[u8::from(enable)]);
    }

    pub(crate) fn when_inject_legacy_advertising_pdu(
        &mut self,
        sender_id: Id,
        advertising_data: &[u8],
        phy: Phy,
        rssi: i32,
    ) {
        let pdu = LeLegacyAdvertisingPdu {
            source_address: self.controller_ll_address(sender_id),
            destination_address: LlAddress::try_from(0).expect("valid 48-bit address"),
            advertising_address_type: AddressType::Public,
            target_address_type: AddressType::Public,
            advertising_type: LegacyAdvertisingType::AdvInd,
            advertising_data: advertising_data.to_vec(),
        };
        let mut packet = Vec::new();
        pdu.encode(&mut packet).expect("encoding LeLegacyAdvertisingPdu should succeed");
        self.rootcanal.inject_ll_packet(sender_id, &packet, phy, rssi);
        self.last_injected_packet = Some(packet);
    }

    pub(crate) async fn then_command_complete_received(&mut self, expected_opcode: u16) {
        let event = timeout(HCI_EVENT_TIMEOUT, self.hci_rx.recv())
            .await
            .expect("timed out waiting for HCI event")
            .expect("HCI channel closed unexpectedly");
        assert!(event.len() >= 7, "HCI event too short: {:02x?}", event);
        assert_eq!(event[0], 0x04, "expected HCI Event packet type 0x04");
        assert_eq!(event[1], 0x0e, "expected Command Complete event code 0x0e");
        assert_eq!(
            u16::from_le_bytes([event[4], event[5]]),
            expected_opcode,
            "Command Complete opcode mismatch"
        );
        assert_eq!(event[6], 0x00, "expected Status OK (0x00)");
    }

    pub(crate) async fn then_sniffer_receives_advertising_pdu(
        &mut self,
        expected_sender_id: Id,
        expected_adv_data: &[u8],
    ) {
        let packet = timeout(LL_PACKET_TIMEOUT, self.ll_rx.recv())
            .await
            .expect("timed out waiting for link-layer packet on sniffer")
            .expect("LL channel closed unexpectedly");
        if let Some(injected) = self.last_injected_packet.take() {
            assert_eq!(packet, injected, "received LL packet should match injected packet");
        }
        let parsed =
            LeLegacyAdvertisingPdu::decode_full(&packet).expect("valid LeLegacyAdvertisingPdu");
        assert_eq!(
            parsed.source_address(),
            self.controller_ll_address(expected_sender_id),
            "source address mismatch"
        );
        assert_eq!(parsed.advertising_data(), expected_adv_data, "advertising data mismatch");
    }
}
