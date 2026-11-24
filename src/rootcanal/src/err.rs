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

//! Typed error codes for the `rootcanal` crate (`50_001..=59_999`,
//! `code % 1000` sub-band `+700..=+799`; `50_000` is reserved for the untyped
//! `BLUETOOTH` fallback).
//!
//! Covers the External Backend / FFI band (`+8000..=+9999`, e.g.
//! `58_700..=58_799`) per `common::err` category bands.
//! Pass these as `code = crate::err::Code::Variant as u32` to `error!` /
//! `warn!` so `ErrorStatsLayer` counts them in `NetsimStats.error_stats`.
//! Codes are stable telemetry IDs: never renumber or reuse an existing code.

#[derive(Clone, Copy, Debug)]
#[repr(u32)]
pub(crate) enum Code {
    // +8000..+9999: External Backend / FFI band (`+700..=+799`)
    /// Unknown physical layer (PHY) type supplied by outbound C++ FFI callback.
    UnknownPhy = 58700,
    /// Rootcanal C++ error log message dispatched.
    RootcanalError = 58701,
    /// Rootcanal C++ fatal log message dispatched.
    RootcanalFatal = 58702,
    /// Unknown log level ordinal received by `rootcanal_log_callback`.
    UnknownLogLevel = 58703,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_CODES: &[Code] =
        &[Code::UnknownPhy, Code::RootcanalError, Code::RootcanalFatal, Code::UnknownLogLevel];

    #[test]
    fn test_codes_in_rootcanal_subband() {
        // Uniqueness is enforced at compile time by rustc (E0081).
        for &code in ALL_CODES {
            let val = code as u32;
            assert!((50_001..=59_999).contains(&val), "{code:?} ({val}) out of BLUETOOTH band");
            assert!(
                (700..=799).contains(&(val % 1000)),
                "{code:?} ({val}) out of rootcanal sub-band (+700..=+799)"
            );
            // No wildcard: a new variant fails to compile here; also add it to ALL_CODES so its
            // range is checked.
            match code {
                Code::UnknownPhy
                | Code::RootcanalError
                | Code::RootcanalFatal
                | Code::UnknownLogLevel => {
                    assert!(
                        (58_000..=59_999).contains(&val),
                        "{code:?} ({val}) out of External Backend / FFI band"
                    );
                }
            }
        }
    }
}
