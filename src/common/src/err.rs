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

//! Typed error codes for the `common` crate (`10000..=19999`, `code % 1000`
//! sub-band `+500..+999`).
//!
//! Each `10,000`-code subsystem block (`+0000` reserved for untyped `ERROR`
//! fallback) is partitioned into five `2,000`-code category bands
//! (`go/netsim-error-stats-dd`): `+0001..+1999` Config & Input,
//! `+2000..+3999` OS & System I/O, `+4000..+5999` Client & Transport,
//! `+6000..+7999` Internal & State, and `+8000..+9999` External Backend / FFI.
//! Within `10000..=19999` (Daemon & Startup), `scripts/err_code_sites.py`
//! splits `code % 1000` between `daemon` (`+000..+499`) and `common`
//! (`+500..+999`, e.g. `10500..=10999`, `12500..=12999`, `16500..=16999`).
//!
//! Pass these as `code = err::Code::Variant as u32` to `error!` / `warn!` so
//! `ErrorStatsLayer` counts them in `NetsimStats.error_stats`. Codes are
//! stable telemetry IDs: never renumber or reuse an existing code.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum Code {
    // +0001..+1999: Config & Input
    CommonIniNotFound = 10500,
    /// Fires on startup if `TMPDIR` and discovery env are unset (headless/CI).
    DiscoveryEnvMissing = 10501,
    CommonIniNotAFile = 10502,
    IniServerAddrUnreachable = 10503,
    NetsimIniReadFailed = 10504,
    AvdConfigReadFailed = 10505,
    AvdMacInvalid = 10506,
    AvdMacCollision = 10507,
    InMemoryMacInvalid = 10508,

    // +2000..+3999: OS & System I/O
    CommonIniReadFailed = 12500,
    LoopbackV4Unavailable = 12501,
    ArtifactFilenameUnavailable = 12502,
    ArtifactOpenFailed = 12503,
    ArtifactZipFileRemoveFailed = 12504,
    ArtifactDirRemoveFailed = 12505,
    ArtifactZipCleanupFailed = 12506,
    ArtifactZippedFileRemoveFailed = 12507,
    LoopbackEphemeralBindFailed = 12508,
    LoopbackServerBindFailed = 12509,
    MacPersistFailed = 12510,
    MacResolveFailed = 12511,
    LoopbackPortInUseFallback = 12512,

    // +6000..+7999: Internal & State
    BtMacResolveJoinFailed = 16500,
    InMemoryMacGenerateFailed = 16501,
}
