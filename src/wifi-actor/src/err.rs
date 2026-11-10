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

//! Typed error codes for the `wifi-actor` crate (`60000..=69999`).
//!
//! Pass these as `code = crate::err::Code::Variant as u32` to `error!` /
//! `warn!` so `ErrorStatsLayer` counts them in `NetsimStats.error_stats`.
//! Codes are stable telemetry IDs: never renumber or reuse an existing code.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Code {
    // +0001..+1999: Config, Input & Frame
    StatsFrameError = 60001,
    SlirpPktTooShort = 60002,
    TapPktTooShort = 60003,

    // +2000..+3999: OS, System I/O, TAP & mDNS
    TapNotSupportedFallback = 62001,
    TapReadFailed = 62002,
    TapDevTunOpenFailed = 62003,
    TapAttachFailed = 62004,
    TapAttachNotSupportedNonLinux = 62005,
    MdnsTxSendFailed = 62006,
    MdnsIpv6NotSupported = 62007,
    MdnsSocketRecvFailed = 62008,
    LifecycleMdnsForwarderFailed = 62009,

    // +4000..+5999: Client, Transport & Stream
    StatsNetworkError = 64001,
    StatsClientError = 64002,
    SlirpRegisterFailed = 64003,
    LifecycleApClientRegisterFailed = 64004,
    LifecycleStreamClosedDeleteFailed = 64005,
    LifecycleTaskClosedDeleteFailed = 64006,
    StatsHostapdError = 64007,

    // +6000..+7999: Internal, State, Medium & FTM
    StatsTransmissionError = 66001,
    StatsChipError = 66002,
    StatsOtherError = 66003,
    StatsInternalError = 66004,
    MediumClientMissing = 66005,
    MediumP2pTxClientMissing = 66006,
    MediumP2pRxClientMissing = 66007,
    MediumUnicastUnknownStation = 66008,
    TxEncryptedFrameDecodeFailed = 66009,
    FtmInvalidDistance = 66010,
    SlirpUnicastUnknownStation = 66011,
    TapUnicastUnknownStation = 66012,
}
