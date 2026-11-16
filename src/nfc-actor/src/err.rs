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

//! Typed error codes for the `nfc-actor` crate (`80000..=89999`).
//!
//! Uses sub-band `+000..+499` (`+500..+999` / `88000` reserved for `casimir`).
//! Pass these as `code = crate::err::Code::Variant as u32` to `error!` /
//! `warn!` so `ErrorStatsLayer` counts them in `NetsimStats.error_stats`.
//! Codes are stable telemetry IDs: never renumber or reuse an existing code.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Code {
    // +0001..+1999: Config & Input
    /// Ingress data packet length < 3 bytes.
    DataPacketDecodeFailed = 80001,
    /// Ingress control packet failed decoding.
    ControlPacketDecodeFailed = 80002,

    // +4000..+5999: Client & Transport
    /// Failed to send packet to guest packet sink.
    GuestPacketSendFailed = 84001,
    /// Casimir framed reader stream returned an error.
    NciReaderError = 84002,

    // +6000..+7999: Internal & State
    /// Casimir scene client is not initialized.
    SceneNotStarted = 86001,

    // +8000..+9999: External Backend / FFI
    /// Failed to write guest packet to Casimir.
    CasimirPacketWriteFailed = 88001,
    /// Failed to remove device from Casimir scene during deletion.
    DeviceRemoveFailed = 88002,
    /// Failed to add RF device to Casimir scene.
    AddRfDeviceFailed = 88003,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::Code;

    const ALL_CODES: [Code; 8] = [
        Code::DataPacketDecodeFailed,
        Code::ControlPacketDecodeFailed,
        Code::GuestPacketSendFailed,
        Code::NciReaderError,
        Code::SceneNotStarted,
        Code::CasimirPacketWriteFailed,
        Code::DeviceRemoveFailed,
        Code::AddRfDeviceFailed,
    ];

    #[test]
    fn test_codes_in_nfc_subband_and_unique() {
        let mut seen = HashSet::with_capacity(ALL_CODES.len());
        for code in ALL_CODES {
            let val = code as u32;
            assert!(80_000 < val && val <= 89_999, "{code:?} ({val}) must lie in 80001..=89999");
            assert!(val % 1000 < 500, "{code:?} ({val}) must lie in sub-band +000..+499");
            assert_ne!(val, 88_000, "{code:?} collides with casimir default 88000");
            assert!(seen.insert(val), "duplicate error code {val} on {code:?}");
        }
        assert_eq!(seen.len(), ALL_CODES.len());
    }
}
