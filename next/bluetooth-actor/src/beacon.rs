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

//! This module provides the functionality for creating and managing Bluetooth
//! beacon chips.
//!
//! It includes the `create` function for initializing a new beacon with
//! advertising parameters and data, and placeholder functions for updating
//! and retrieving chip information.
//!
//! NOTE: This module is currently missing the complete setup for converting
//! `BeaconParams` into the appropriate HCI commands for full configuration.

use std::{sync::Arc, time::Duration};

use netsim_model::{BeaconParams, ChipError, ChipId, Interval};
use netsim_packets::{
    Address as PacketsAddress, AdvertisingFilterPolicy, AdvertisingType, Enable,
    LeSetAdvertisingData, LeSetAdvertisingEnable, LeSetAdvertisingParameters,
    LeSetScanResponseData, OwnAddressType, PeerAddressType, Reset,
};
use rootcanal::{Address, Rootcanal};
use zerocopy::U16;

use crate::{
    beacon_utils::{MAX_AD_PAYLOAD_LEN, construct_data, resolve_tx_power_dbm},
    utils::send_hci_command,
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
            "Invalid beacon address '{}': {}",
            params.ble_beacon.address, err
        )))
    })?;

    let tx_power_dbm = resolve_tx_power_dbm(params.ble_beacon.settings.as_ref());

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

    let mut adv_data_payload = [0u8; MAX_AD_PAYLOAD_LEN];
    let Some(dest) = adv_data_payload.get_mut(..adv_data.len()) else {
        return Err(ChipError::InvalidArguments(Box::from(format!(
            "Advertising data exceeds maximum payload size of {MAX_AD_PAYLOAD_LEN} bytes"
        ))));
    };
    dest.copy_from_slice(&adv_data);

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

    let mut scan_resp_payload = [0u8; MAX_AD_PAYLOAD_LEN];
    let Some(dest) = scan_resp_payload.get_mut(..scan_resp_data.len()) else {
        return Err(ChipError::InvalidArguments(Box::from(format!(
            "Scan response data exceeds maximum payload size of {MAX_AD_PAYLOAD_LEN} bytes"
        ))));
    };
    dest.copy_from_slice(&scan_resp_data);

    if disable_first {
        // Disable advertising before reconfiguring parameters.
        send_hci_command(
            rootcanal,
            chip_id,
            LeSetAdvertisingEnable { advertising_enable: Enable::DISABLED },
        )?;
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
    let slots = (interval_ms * BLE_SLOT_NUMERATOR / BLE_SLOT_DENOMINATOR)
        .clamp(ADV_INTERVAL_MIN_SLOTS, ADV_INTERVAL_MAX_SLOTS) as u16;

    let scannable = params.ble_beacon.settings.as_ref().map(|s| s.scannable).unwrap_or_default();
    let advertising_type =
        if scannable { AdvertisingType::ADV_SCAN_IND } else { AdvertisingType::ADV_NONCONN_IND };

    // LE Set Advertising Parameters
    send_hci_command(
        rootcanal,
        chip_id,
        LeSetAdvertisingParameters {
            advertising_interval_min: U16::new(slots),
            advertising_interval_max: U16::new(slots),
            advertising_type,
            own_address_type: OwnAddressType::PUBLIC_DEVICE_ADDRESS,
            peer_address_type: PeerAddressType::PUBLIC_DEVICE_OR_IDENTITY_ADDRESS,
            peer_address: PacketsAddress { bytes: address.address },
            advertising_channel_map: ADV_CHANNELS_ALL,
            advertising_filter_policy: AdvertisingFilterPolicy::ALL_DEVICES,
        },
    )?;

    send_hci_command(
        rootcanal,
        chip_id,
        LeSetAdvertisingData {
            advertising_data_length: adv_data.len() as u8,
            advertising_data: adv_data_payload,
        },
    )?;

    send_hci_command(
        rootcanal,
        chip_id,
        LeSetScanResponseData {
            scan_response_data_length: scan_resp_data.len() as u8,
            scan_response_data: scan_resp_payload,
        },
    )?;

    // LE Set Advertising Enable
    send_hci_command(
        rootcanal,
        chip_id,
        LeSetAdvertisingEnable { advertising_enable: Enable::ENABLED },
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
                        LeSetAdvertisingEnable { advertising_enable: Enable::DISABLED },
                    );
                })
                .abort_handle()
            },
        );

    Ok(timeout_handle)
}
