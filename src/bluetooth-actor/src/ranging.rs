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

//! Ranging library for Bluetooth RSSI calculation.

/// The Free Space Path Loss (FSPL) model is considered as the standard
/// under the ideal scenario.
/// (dBm) PATH_LOSS at 1m for isotropic antenna transmitting BLE.
const PATH_LOSS_AT_1M: f32 = 40.20;

const NEARBY_SHARE_TX_POWER_DEFAULT: i8 = -49;
const RSSI_MIN_DBM: f32 = -120.0;
const RSSI_MAX_DBM: f32 = 20.0;
const FSPL_MULTIPLIER: f32 = 20.0;

/// Convert distance to RSSI for BLE beacons using the free space path loss
/// equation directly from `tx_power` without Nearby Share fallback.
///
/// # Parameters
///
/// * `tx_power`: transmitted power (dBm) calibrated to 1 meter.
/// * `distance`: distance in meters (m).
///
/// # Returns
///
/// The rssi that would be measured at that distance, in the range -120..20 dBm.
pub(crate) fn beacon_distance_to_rssi(tx_power: i8, distance: f32) -> i8 {
    if distance <= 0.0 {
        (tx_power as f32 + PATH_LOSS_AT_1M).clamp(RSSI_MIN_DBM, RSSI_MAX_DBM) as i8
    } else {
        (tx_power as f32 - FSPL_MULTIPLIER * distance.log10()).clamp(RSSI_MIN_DBM, RSSI_MAX_DBM)
            as i8
    }
}

/// Convert distance to RSSI using the free space path loss equation.
/// See [Free-space_path_loss][1].
///
/// [1]: http://en.wikipedia.org/wiki/Free-space_path_loss
///
/// # Parameters
///
/// * `distance`: distance in meters (m).
/// * `tx_power`: transmitted power (dBm) calibrated to 1 meter.
///
/// # Returns
///
/// The rssi that would be measured at that distance, in the
/// range -120..20 dBm,
pub(crate) fn distance_to_rssi(tx_power: i8, distance: f32) -> i8 {
    // Rootcanal reporting tx_power of 0 or 1 during Nearby Share
    let new_tx_power = match tx_power {
        0 | 1 => NEARBY_SHARE_TX_POWER_DEFAULT,
        _ => tx_power,
    };
    beacon_distance_to_rssi(new_tx_power, distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rssi_at_0m() {
        let rssi_at_0m = distance_to_rssi(-120, 0.0);
        assert_eq!(rssi_at_0m, -79);
    }

    #[test]
    fn rssi_at_1m() {
        // With transmit power at 0 dBm verify a reasonable rssi at 1m
        let rssi_at_1m = distance_to_rssi(0, 1.0);
        assert!(rssi_at_1m < -35 && rssi_at_1m > -55);
    }

    #[test]
    fn rssi_saturate_inf() {
        // Verify that the rssi saturates at -120 for very large distances.
        let rssi_inf = distance_to_rssi(-120, 1000.0);
        assert_eq!(rssi_inf, -120);
    }

    #[test]
    fn rssi_saturate_sup() {
        // Verify that the rssi saturates at +20 for the largest tx power
        // and nearest distance.
        let rssi_sup = distance_to_rssi(20, 0.0);
        assert_eq!(rssi_sup, 20);
    }

    #[test]
    fn test_beacon_distance_to_rssi_preserves_zero_and_one() {
        // Beacon distance calculation does not apply Nearby Share 0 | 1 => -49 dBm fallback.
        // At 1.0 m, log10(1.0) == 0.0, so RSSI == tx_power.
        assert_eq!(beacon_distance_to_rssi(0, 1.0), 0);
        assert_eq!(beacon_distance_to_rssi(1, 1.0), 1);
    }

    #[test]
    fn test_beacon_distance_to_rssi_distances_and_saturation() {
        // At distance <= 0.0 m, path loss at 1m is added without taking log10.
        assert_eq!(beacon_distance_to_rssi(0, 0.0), 20); // 40.20 clamped to 20
        assert_eq!(beacon_distance_to_rssi(0, -1.0), 20); // negative distance guarded
        assert_eq!(beacon_distance_to_rssi(-50, 0.0), -9); // -50 + 40.20 = -9.8 -> -9
        assert_eq!(beacon_distance_to_rssi(-50, -1.0), -9);

        // At 10.0 m: log10(10.0) == 1.0 -> RSSI == tx_power - 20.
        assert_eq!(beacon_distance_to_rssi(1, 10.0), -19);
        assert_eq!(beacon_distance_to_rssi(0, 10.0), -20);

        // Saturation at extremes.
        assert_eq!(beacon_distance_to_rssi(-120, 1000.0), -120);
        assert_eq!(beacon_distance_to_rssi(20, 0.0), 20);
    }
}
