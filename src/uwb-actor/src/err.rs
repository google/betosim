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

//! Typed error codes for the `uwb-actor` crate (`90000..=99999`).
//!
//! `pica` currently uses only the single target-based fallback code `98000`
//! (`UWB_PICA` in `netsim_logger.rs`); `uwb-actor` is limited to sub-band
//! `+000..+499` in each 1,000-code slice to leave `+500..+999` open if `pica`
//! adds typed codes later.
//!
//! Pass these as `code = crate::err::Code::Variant as u32` to `error!` /
//! `warn!` so `ErrorStatsLayer` counts them in `NetsimStats.error_stats`.
//! Codes are stable telemetry IDs: never renumber or reuse an existing code.

/// Stable telemetry error codes emitted by `uwb-actor`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Code {
    // +4000..+5999: Client & Transport
    /// Failed to send packet to UWB guest packet sink.
    PacketSinkSendFailed = 94001,
    /// Failed to notify DeviceActor of UWB chip removal.
    DeviceNotifyChipRemovedFailed = 94002,

    // +6000..+7999: Internal & State
    /// Failed to estimate ranging measurement between two UWB chips.
    RangingEstimateFailed = 96001,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::Code;

    const ALL_CODES: [Code; 3] = [
        Code::PacketSinkSendFailed,
        Code::DeviceNotifyChipRemovedFailed,
        Code::RangingEstimateFailed,
    ];

    #[test]
    fn test_codes_in_uwb_subband_and_unique() {
        let mut seen = HashSet::with_capacity(ALL_CODES.len());
        for code in ALL_CODES {
            let val = code as u32;
            assert!(90_000 < val && val <= 99_999, "{code:?} ({val}) must lie in 90001..=99999");
            assert!(val % 1000 < 500, "{code:?} ({val}) must lie in sub-band +000..+499");
            assert_ne!(val, 98_000, "{code:?} collides with pica default 98000");
            assert!(seen.insert(val), "duplicate error code {val} on {code:?}");
        }
        assert_eq!(seen.len(), ALL_CODES.len());
    }
}
