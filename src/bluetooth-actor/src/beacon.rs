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

//! This module provides functionality for creating and managing Bluetooth
//! beacon chips in RootCanal.
//!
//! It translates `BeaconParams` into HCI extended advertising commands with
//! legacy PDU properties, configuring transmit power, advertising data,
//! scan response data, and advertising intervals.

use std::{sync::Arc, time::Duration};

use netsim_model::{BeaconParams, ChipError, ChipId, Interval};
use netsim_packets::{
    Address as PacketsAddress, AdvertisingEventProperties, AdvertisingFilterPolicy, Enable,
    EnabledSet, FragmentPreference, LeClearAdvertisingSets, LeSetExtendedAdvertisingData,
    LeSetExtendedAdvertisingEnable, LeSetExtendedAdvertisingParameters,
    LeSetExtendedScanResponseData, Operation, OwnAddressType, PeerAddressType, PrimaryPhyType,
    Reset, SecondaryPhyType,
};
use rootcanal::{Address, Rootcanal};
use zerocopy::U16;

use crate::{
    beacon_utils::{MAX_AD_PAYLOAD_LEN, construct_data, resolve_tx_power_dbm},
    utils::{send_hci_command, send_hci_command_with_payload},
};

const DEFAULT_BEACON_INTERVAL_MS: u64 = 1000;
const MODE_LOW_POWER_MS: u64 = 1000;
const MODE_BALANCED_MS: u64 = 250;
const MODE_LOW_LATENCY_MS: u64 = 100;

const ADV_INTERVAL_MIN_SLOTS: u64 = 0x0020;
const ADV_INTERVAL_MAX_SLOTS: u64 = 0x4000;
const BLE_SLOT_NUMERATOR: u64 = 8;
const BLE_SLOT_DENOMINATOR: u64 = 5;
const ADV_CHANNELS_ALL: u8 = 0x07;

const DEFAULT_ADV_HANDLE: u8 = 0;
const HCI_TX_POWER_MIN_DBM: i8 = -127;
const HCI_TX_POWER_MAX_DBM: i8 = 20;

/// Creates a new `BeaconChip`.
pub(crate) fn create(
    rootcanal: &Arc<Rootcanal>,
    chip_id: ChipId,
    params: &BeaconParams,
    device_name: &str,
) -> Result<Option<tokio::task::AbortHandle>, ChipError> {
    // Reset the controller first.
    send_hci_command(rootcanal, chip_id, Reset {})?;
    configure(rootcanal, chip_id, params, device_name, false)
}

/// Updates an existing `BeaconChip` with new advertising parameters and data.
pub(crate) fn update(
    rootcanal: &Arc<Rootcanal>,
    chip_id: ChipId,
    params: &BeaconParams,
    device_name: &str,
) -> Result<Option<tokio::task::AbortHandle>, ChipError> {
    configure(rootcanal, chip_id, params, device_name, true)
}

fn configure(
    rootcanal: &Arc<Rootcanal>,
    chip_id: ChipId,
    params: &BeaconParams,
    device_name: &str,
    disable_first: bool,
) -> Result<Option<tokio::task::AbortHandle>, ChipError> {
    let address: Address = params.ble_beacon.address.parse().map_err(|err| {
        ChipError::InvalidArguments(Box::from(format!(
            "invalid beacon address '{}': {}",
            params.ble_beacon.address, err
        )))
    })?;

    let tx_power_dbm = resolve_tx_power_dbm(params.ble_beacon.settings.as_ref())
        .clamp(HCI_TX_POWER_MIN_DBM, HCI_TX_POWER_MAX_DBM);

    let adv_data = if let Some(adv_data) = &params.ble_beacon.adv_data {
        construct_data(
            adv_data,
            adv_data.include_device_name.then_some(device_name),
            true,
            tx_power_dbm,
        )
    } else {
        construct_data(&netsim_model::AdvertiseData::default(), None, true, tx_power_dbm)
    };

    if adv_data.len() > MAX_AD_PAYLOAD_LEN {
        return Err(ChipError::InvalidArguments(Box::from(format!(
            "advertising data length {} exceeds maximum payload of {MAX_AD_PAYLOAD_LEN} bytes",
            adv_data.len()
        ))));
    }

    let scan_resp_data = if let Some(scan_resp) = &params.ble_beacon.scan_response {
        construct_data(
            scan_resp,
            scan_resp.include_device_name.then_some(device_name),
            false,
            tx_power_dbm,
        )
    } else {
        vec![]
    };

    if scan_resp_data.len() > MAX_AD_PAYLOAD_LEN {
        return Err(ChipError::InvalidArguments(Box::from(format!(
            "scan response data length {} exceeds maximum payload of {MAX_AD_PAYLOAD_LEN} bytes",
            scan_resp_data.len()
        ))));
    }

    if disable_first {
        // Clear existing advertising sets before reconfiguring to prevent stale parameter
        // or scan response carry-over.
        send_hci_command(
            rootcanal,
            chip_id,
            LeSetExtendedAdvertisingEnable {
                enable: Enable::DISABLED,
                num_sets: 0,
                enabled_sets: (),
            },
        )?;
        send_hci_command(rootcanal, chip_id, LeClearAdvertisingSets)?;
    }

    let interval_ms = params
        .ble_beacon
        .settings
        .as_ref()
        .and_then(|s| s.interval.as_ref())
        .map(|interval| match interval {
            Interval::Milliseconds(ms) => *ms,
            Interval::AdvertiseMode(mode) => match *mode {
                netsim_model::AdvertiseMode::LowPower => MODE_LOW_POWER_MS,
                netsim_model::AdvertiseMode::Balanced => MODE_BALANCED_MS,
                netsim_model::AdvertiseMode::LowLatency => MODE_LOW_LATENCY_MS,
            },
        })
        .unwrap_or(DEFAULT_BEACON_INTERVAL_MS);
    let slots = (interval_ms.saturating_mul(BLE_SLOT_NUMERATOR) / BLE_SLOT_DENOMINATOR)
        .clamp(ADV_INTERVAL_MIN_SLOTS, ADV_INTERVAL_MAX_SLOTS);
    let [b0, b1, b2, _] = (slots as u32).to_le_bytes();
    let interval_bytes = [b0, b1, b2];

    let scannable = params.ble_beacon.settings.as_ref().map(|s| s.scannable).unwrap_or_default();
    let advertising_event_properties = if scannable {
        AdvertisingEventProperties::LEGACY_ADV_SCAN_IND
    } else {
        AdvertisingEventProperties::LEGACY_ADV_NONCONN_IND
    };

    send_hci_command(
        rootcanal,
        chip_id,
        LeSetExtendedAdvertisingParameters {
            advertising_handle: DEFAULT_ADV_HANDLE,
            advertising_event_properties,
            primary_advertising_interval_min: interval_bytes,
            primary_advertising_interval_max: interval_bytes,
            primary_advertising_channel_map: ADV_CHANNELS_ALL,
            own_address_type: OwnAddressType::PUBLIC_DEVICE_ADDRESS,
            peer_address_type: PeerAddressType::PUBLIC_DEVICE_OR_IDENTITY_ADDRESS,
            peer_address: PacketsAddress { bytes: address.address },
            advertising_filter_policy: AdvertisingFilterPolicy::ALL_DEVICES,
            advertising_tx_power: tx_power_dbm,
            primary_advertising_phy: PrimaryPhyType::LE_1M,
            secondary_advertising_max_skip: 0,
            secondary_advertising_phy: SecondaryPhyType::LE_1M,
            advertising_sid: 0,
            scan_request_notification_enable: Enable::DISABLED,
        },
    )?;

    send_hci_command_with_payload(
        rootcanal,
        chip_id,
        LeSetExtendedAdvertisingData {
            advertising_handle: DEFAULT_ADV_HANDLE,
            operation: Operation::COMPLETE_ADVERTISEMENT,
            fragment_preference: FragmentPreference::CONTROLLER_MAY_FRAGMENT,
            advertising_data_length: adv_data.len() as u8,
            advertising_data: (),
        },
        &adv_data,
    )?;

    // Non-scannable legacy advertisements cannot accept scan response data.
    if scannable {
        send_hci_command_with_payload(
            rootcanal,
            chip_id,
            LeSetExtendedScanResponseData {
                advertising_handle: DEFAULT_ADV_HANDLE,
                operation: Operation::COMPLETE_ADVERTISEMENT,
                fragment_preference: FragmentPreference::CONTROLLER_MAY_FRAGMENT,
                scan_response_data_length: scan_resp_data.len() as u8,
                scan_response_data: (),
            },
            &scan_resp_data,
        )?;
    }

    let enabled_set = EnabledSet {
        advertising_handle: DEFAULT_ADV_HANDLE,
        duration: U16::ZERO,
        max_extended_advertising_events: 0,
    };

    send_hci_command(
        rootcanal,
        chip_id,
        LeSetExtendedAdvertisingEnable {
            enable: Enable::ENABLED,
            num_sets: 1,
            enabled_sets: [enabled_set],
        },
    )?;

    let timeout_handle =
        params.ble_beacon.settings.as_ref().map(|s| s.timeout).filter(|&t| t > 0).map(
            |timeout_ms| {
                let rc = Arc::clone(rootcanal);
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                    let _ = send_hci_command(
                        &rc,
                        chip_id,
                        LeSetExtendedAdvertisingEnable {
                            enable: Enable::DISABLED,
                            num_sets: 1,
                            enabled_sets: [enabled_set],
                        },
                    );
                })
                .abort_handle()
            },
        );

    Ok(timeout_handle)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use netsim_model::{AdvertiseData, AdvertiseSettings, BleBeacon, TxPower};
    use parking_lot::Mutex;
    use rootcanal::controller::Callbacks as ControllerCallbacks;

    use super::*;
    use crate::utils::DummyCallbacks;

    struct TestControllerCallbacks {
        events: Arc<Mutex<Vec<Vec<u8>>>>,
        invalid_count: Arc<AtomicUsize>,
    }

    impl ControllerCallbacks for TestControllerCallbacks {
        fn send_hci(&self, _source_id: rootcanal::controller::Id, packet: bytes::Bytes) {
            self.events.lock().push(packet.to_vec());
        }

        fn on_receive_ll(
            &self,
            _source_id: rootcanal::controller::Id,
            _packet: &[u8],
            _phy: rootcanal::types::Phy,
            _tx_power: i32,
        ) {
        }

        fn invalid_packet_received(
            &self,
            _source_id: rootcanal::controller::Id,
            _reason: std::ffi::c_int,
            _message: &str,
            _data: &[u8],
        ) {
            self.invalid_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn drain_and_verify_events(
        events: &Arc<Mutex<Vec<Vec<u8>>>>,
        invalid_count: &Arc<AtomicUsize>,
    ) -> Vec<i8> {
        assert_eq!(
            invalid_count.load(Ordering::SeqCst),
            0,
            "No invalid HCI packets should be received"
        );
        let mut evts = events.lock();
        let mut selected_tx_powers = Vec::new();
        for pkt in evts.drain(..) {
            assert!(pkt.len() > 1, "HCI packet too short");
            assert_eq!(pkt[0], 0x04, "Expected HCI Event packet type 0x04");
            if let Some(netsim_packets::HciEvent::CommandComplete { header, return_parameters }) =
                netsim_packets::parse_hci_event(&pkt[1..])
            {
                assert_eq!(
                    return_parameters.first(),
                    Some(&0x00),
                    "HCI command complete status should be 0x00 (SUCCESS) for opcode {:#06X}",
                    header.op_code.get()
                );
                if header.op_code
                    == <LeSetExtendedAdvertisingParameters as netsim_packets::HciCommand>::OP_CODE
                {
                    assert!(
                        return_parameters.len() >= 2,
                        "LeSetExtendedAdvertisingParameters response missing Selected_Tx_Power"
                    );
                    selected_tx_powers.push(return_parameters[1] as i8);
                }
            }
        }
        selected_tx_powers
    }

    #[test]
    fn test_beacon_reconfigure_scannable_to_non_scannable() {
        let rootcanal = Rootcanal::new(Box::new(DummyCallbacks), false, Box::new(|_, _, _, _| {}));
        let chip_id = ChipId(1);
        let events = Arc::new(Mutex::new(Vec::new()));
        let invalid_count = Arc::new(AtomicUsize::new(0));
        rootcanal
            .new_controller(
                chip_id.0,
                "00:11:22:33:44:55".parse().unwrap(),
                Box::new(TestControllerCallbacks {
                    events: Arc::clone(&events),
                    invalid_count: Arc::clone(&invalid_count),
                }),
                None,
            )
            .unwrap();

        let params_scannable = BeaconParams {
            ble_beacon: BleBeacon {
                address: "00:11:22:33:44:55".to_string(),
                settings: Some(AdvertiseSettings {
                    tx_power: Some(TxPower::Dbm(-10)),
                    scannable: true,
                    ..Default::default()
                }),
                adv_data: Some(AdvertiseData { include_device_name: true, ..Default::default() }),
                scan_response: Some(AdvertiseData {
                    manufacturer_data: vec![0x01, 0x02],
                    ..Default::default()
                }),
            },
        };

        assert!(create(&rootcanal, chip_id, &params_scannable, "TestBeacon").is_ok());
        let powers = drain_and_verify_events(&events, &invalid_count);
        assert_eq!(powers, vec![-10]);

        let params_non_scannable = BeaconParams {
            ble_beacon: BleBeacon {
                address: "00:11:22:33:44:55".to_string(),
                settings: Some(AdvertiseSettings {
                    tx_power: Some(TxPower::Dbm(5)),
                    scannable: false,
                    ..Default::default()
                }),
                adv_data: Some(AdvertiseData { include_device_name: true, ..Default::default() }),
                scan_response: None,
            },
        };

        // Updating from scannable to non-scannable succeeds because disable_first clears
        // prior advertising sets.
        assert!(update(&rootcanal, chip_id, &params_non_scannable, "TestBeacon").is_ok());
        let powers = drain_and_verify_events(&events, &invalid_count);
        assert_eq!(powers, vec![5]);
    }

    #[test]
    fn test_beacon_tx_power_clamping_bounds() {
        let rootcanal = Rootcanal::new(Box::new(DummyCallbacks), false, Box::new(|_, _, _, _| {}));
        let chip_id = ChipId(1);
        let events = Arc::new(Mutex::new(Vec::new()));
        let invalid_count = Arc::new(AtomicUsize::new(0));
        rootcanal
            .new_controller(
                chip_id.0,
                "00:11:22:33:44:55".parse().unwrap(),
                Box::new(TestControllerCallbacks {
                    events: Arc::clone(&events),
                    invalid_count: Arc::clone(&invalid_count),
                }),
                None,
            )
            .unwrap();

        let params_min = BeaconParams {
            ble_beacon: BleBeacon {
                address: "00:11:22:33:44:55".to_string(),
                settings: Some(AdvertiseSettings {
                    tx_power: Some(TxPower::Dbm(-128)),
                    scannable: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
        };
        assert!(create(&rootcanal, chip_id, &params_min, "BeaconMin").is_ok());
        let powers = drain_and_verify_events(&events, &invalid_count);
        assert_eq!(powers, vec![-127]);

        let params_max = BeaconParams {
            ble_beacon: BleBeacon {
                address: "00:11:22:33:44:55".to_string(),
                settings: Some(AdvertiseSettings {
                    tx_power: Some(TxPower::Dbm(127)),
                    scannable: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
        };
        assert!(update(&rootcanal, chip_id, &params_max, "BeaconMax").is_ok());
        let powers = drain_and_verify_events(&events, &invalid_count);
        assert_eq!(powers, vec![20]);
    }

    #[test]
    fn test_beacon_invalid_inputs_rejected() {
        let rootcanal = Rootcanal::new(Box::new(DummyCallbacks), false, Box::new(|_, _, _, _| {}));
        let chip_id = ChipId(1);
        let events = Arc::new(Mutex::new(Vec::new()));
        let invalid_count = Arc::new(AtomicUsize::new(0));
        rootcanal
            .new_controller(
                chip_id.0,
                "00:11:22:33:44:55".parse().unwrap(),
                Box::new(TestControllerCallbacks {
                    events: Arc::clone(&events),
                    invalid_count: Arc::clone(&invalid_count),
                }),
                None,
            )
            .unwrap();

        let params_invalid_addr = BeaconParams {
            ble_beacon: BleBeacon { address: "not-an-address".to_string(), ..Default::default() },
        };
        let err = create(&rootcanal, chip_id, &params_invalid_addr, "Beacon");
        assert!(matches!(err, Err(ChipError::InvalidArguments(_))));

        let params_oversized_adv = BeaconParams {
            ble_beacon: BleBeacon {
                address: "00:11:22:33:44:55".to_string(),
                adv_data: Some(AdvertiseData {
                    manufacturer_data: vec![0xAA; 32],
                    ..Default::default()
                }),
                ..Default::default()
            },
        };
        let err = create(&rootcanal, chip_id, &params_oversized_adv, "Beacon");
        assert!(matches!(err, Err(ChipError::InvalidArguments(_))));

        let params_oversized_scan = BeaconParams {
            ble_beacon: BleBeacon {
                address: "00:11:22:33:44:55".to_string(),
                scan_response: Some(AdvertiseData {
                    manufacturer_data: vec![0xBB; 32],
                    ..Default::default()
                }),
                ..Default::default()
            },
        };
        let err = create(&rootcanal, chip_id, &params_oversized_scan, "Beacon");
        assert!(matches!(err, Err(ChipError::InvalidArguments(_))));
    }
}
