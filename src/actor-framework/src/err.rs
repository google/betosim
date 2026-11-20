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

//! Typed error codes for the `actor-framework` crate (`20000..=29999`,
//! `code % 1000` sub-band `+500..+799`).
//!
//! Each `10,000`-code subsystem block (`+0000` reserved for untyped `ERROR`
//! fallback) is partitioned into five `2,000`-code category bands
//! (`go/netsim-error-stats-dd`): `+0001..+1999` Config & Input,
//! `+2000..+3999` OS & System I/O, `+4000..+5999` Client & Transport,
//! `+6000..+7999` Internal & State, and `+8000..+9999` External Backend / FFI.
//! Within `20000..=29999` (Actor & Device), three crates share the block,
//! splitting `code % 1000` across `device-actor` (`+000..+499`),
//! `actor-framework` (`+500..+799`, e.g. `26500..=26799`), and `link-actor`
//! (`+800..+999`).
//!
//! Pass these as `code = err::Code::Variant as u32` to `error!` / `warn!` so
//! `ErrorStatsLayer` counts them in `NetsimStats.error_stats`. Codes are
//! stable telemetry IDs: never renumber or reuse an existing code.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum Code {
    // +6000..+7999: Internal & State
    MonitoredTaskFailed = 26500,
    ActorLoopStalled = 26501,
}
