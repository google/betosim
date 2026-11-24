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

//! Typed error codes for the `bluetooth-actor` crate (`50_001..=59_999`,
//! `code % 1000` sub-band `+000..=+499`; `50_000` is reserved for the untyped
//! `BLUETOOTH` fallback).
//!
//! Pass these as `code = crate::err::Code::Variant as u32` to `error!` /
//! `warn!` so `ErrorStatsLayer` counts them in `NetsimStats.error_stats`.
//! Codes are stable telemetry IDs: never renumber or reuse an existing code.

/// Numeric telemetry codes attached to `tracing` error and warning events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum Code {
    // +0001..+1999: Config & Input (`+000..=+499`)
    /// Advertising service UUID length is not 2, 4, or 16 bytes.
    InvalidUuidLength = 50001,
    /// Invalid HCI or link-layer packet reported by `RootCanal`.
    InvalidPacketReceived = 50002,

    // +4000..+5999: Client & Transport (`+000..=+499`)
    /// Failed to enqueue an outbound HCI packet to the chip sink channel.
    HciPacketSendFailed = 54001,
    /// Failed to enqueue a captured link-layer packet to the sniffer channel.
    LlPacketSendFailed = 54002,
    /// Failed to write an outbound HCI packet to the chip sink.
    SinkTaskFailed = 54003,
    /// Failed to sync a beacon's default name to `DeviceActor`.
    SyncDeviceNameFailed = 54004,

    // +6000..+7999: Internal & State (`+000..=+499`)
    /// Failed to forward an inbound HCI packet to `RootCanal`.
    ReceiveHciError = 56001,
    /// Failed to delete a chip after its inbound packet stream closed.
    StreamClosedChipDeleteFailed = 56002,
    /// Failed to delete a chip after its outbound sink task closed.
    SinkClosedChipDeleteFailed = 56003,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_CODES: &[Code] = &[
        Code::InvalidUuidLength,
        Code::InvalidPacketReceived,
        Code::HciPacketSendFailed,
        Code::LlPacketSendFailed,
        Code::SinkTaskFailed,
        Code::SyncDeviceNameFailed,
        Code::ReceiveHciError,
        Code::StreamClosedChipDeleteFailed,
        Code::SinkClosedChipDeleteFailed,
    ];

    #[test]
    fn test_codes_in_bluetooth_actor_subband() {
        // Uniqueness is enforced at compile time by rustc (E0081).
        for &code in ALL_CODES {
            // No wildcard: a new variant fails to compile here; also add it to
            // ALL_CODES so its range is checked.
            match code {
                Code::InvalidUuidLength
                | Code::InvalidPacketReceived
                | Code::HciPacketSendFailed
                | Code::LlPacketSendFailed
                | Code::SinkTaskFailed
                | Code::SyncDeviceNameFailed
                | Code::ReceiveHciError
                | Code::StreamClosedChipDeleteFailed
                | Code::SinkClosedChipDeleteFailed => {}
            }
            let val = code as u32;
            assert!((50_001..=59_999).contains(&val), "{code:?} ({val}) out of BLUETOOTH band");
            assert!(
                (val % 10_000) < 8_000,
                "{code:?} ({val}) in reserved +8000..+9999 External Backend / FFI band"
            );
            assert!(
                (val % 1000) < 500,
                "{code:?} ({val}) out of bluetooth-actor sub-band (+000..=+499)"
            );
        }
    }
}
