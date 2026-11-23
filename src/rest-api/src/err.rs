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

//! Typed error codes for the `rest-api` crate (`40_001..=49_999`, `code % 1000`
//! sub-band `+500..=+799`; `40_000` is reserved for the untyped `FRONTEND`
//! fallback).
//!
//! Pass these as `code = crate::err::Code::Variant as u32` to `error!` /
//! `warn!` so `ErrorStatsLayer` counts them in `NetsimStats.error_stats`.
//! Codes are stable telemetry IDs: never renumber or reuse an existing code.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum Code {
    // +2000..+3999: OS & System I/O (`+500..=+799`)
    ListenerLocalAddr = 42500,
    ListenerAccept = 42501,
    CaptureFileRead = 42502,

    // +6000..+7999: Internal & State (`+500..=+799`)
    GetRadioStats = 46500,
}

#[cfg(test)]
pub(crate) type ErrorStats = Vec<(u32, u32)>;

#[cfg(test)]
pub(crate) fn run_with_error_stats<R>(f: impl std::future::Future<Output = R>) -> (R, ErrorStats) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (table, _guard) = common::util::netsim_logger::isolated_error_stats();
    let res = rt.block_on(f);
    drop(rt);
    (res, table.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_CODES: &[Code] = &[
        Code::ListenerLocalAddr,
        Code::ListenerAccept,
        Code::CaptureFileRead,
        Code::GetRadioStats,
    ];

    #[test]
    fn test_codes_in_rest_api_subband() {
        // Uniqueness is enforced at compile time by rustc (E0081).
        for &code in ALL_CODES {
            // No wildcard: a new variant fails to compile here; also add it to
            // ALL_CODES so its range is checked.
            match code {
                Code::ListenerLocalAddr
                | Code::ListenerAccept
                | Code::CaptureFileRead
                | Code::GetRadioStats => {}
            }
            let val = code as u32;
            assert!((40_001..=49_999).contains(&val), "{code:?} ({val}) out of FRONTEND band");
            assert!(
                (val % 10_000) < 8_000,
                "{code:?} ({val}) in External Backend / FFI band (+8000..+9999)"
            );
            assert!(
                (500..=799).contains(&(val % 1000)),
                "{code:?} ({val}) out of rest-api sub-band (+500..=+799)"
            );
        }
    }
}
