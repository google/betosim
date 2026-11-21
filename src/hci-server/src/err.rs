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

//! Typed error codes for the `hci-server` crate (`50_001..=59_999`,
//! `code % 1000` sub-band `+500..=+699`; `50_000` is reserved for the untyped
//! `BLUETOOTH` fallback).
//!
//! Pass these as `code = crate::err::Code::Variant as u32` to `error!` /
//! `warn!` so `ErrorStatsLayer` counts them in `NetsimStats.error_stats`.
//! Codes are stable telemetry IDs: never renumber or reuse an existing code.

/// Numeric telemetry codes attached to `tracing` error and warning events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum Code {
    // +2000..+3999: OS & System I/O (`+500..=+699`)
    HciAcceptConnection = 52500,

    // +4000..+5999: Client & Transport (`+500..=+699`)
    HciSocketRead = 54500,

    // +6000..+7999: Internal & State (`+500..=+699`)
    AddVirtualHciChip = 56500,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_CODES: &[Code] =
        &[Code::HciAcceptConnection, Code::HciSocketRead, Code::AddVirtualHciChip];

    #[test]
    fn test_codes_in_hci_server_subband() {
        assert!(!ALL_CODES.is_empty());
        // Uniqueness is enforced at compile time by rustc (E0081).
        for &code in ALL_CODES {
            // No wildcard: a new variant fails to compile here; also add it to ALL_CODES so its
            // range is checked.
            match code {
                Code::HciAcceptConnection | Code::HciSocketRead | Code::AddVirtualHciChip => {}
            }
            let val = code as u32;
            assert!((50_001..=59_999).contains(&val), "{code:?} ({val}) out of BLUETOOTH band");
            assert!(
                (val % 10_000) < 8_000,
                "{code:?} ({val}) in reserved 8000..9999 External Backend / FFI band"
            );
            assert!(
                (500..=699).contains(&(val % 1000)),
                "{code:?} ({val}) out of hci-server sub-band (+500..=+699)"
            );
        }
    }
}
