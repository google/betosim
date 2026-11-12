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

//! Typed error codes for the `ap-actor` crate (`70000..=79999`).
//!
//! Pass these as `code = crate::err::Code::Variant as u32` to `error!` /
//! `warn!` so `ErrorStatsLayer` counts them in `NetsimStats.error_stats`.
//! Codes are stable telemetry IDs: never renumber or reuse an existing code.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Code {
    // +0001..+1999: Config & Input
    FrameDecodeFailed = 70001,
    AuthFrameTooShort = 70002,
    AuthFixedFieldsInvalid = 70003,
    AuthSaeNotEnabled = 70004,
    AuthSaePayloadMissing = 70005,
    EapolStartEnterpriseDisabled = 70006,
    StreamFrameParseFailed = 70007,
    SaeCommitTooShort = 70008,
    SaeUnsupportedGroup = 70009,
    SaePeerElementLenInvalid = 70010,
    SaeConfirmTooShort = 70011,
    ForceDisconnectInvalidMac = 70012,
    ForceDisconnectUnknownMac = 70013,
    DeleteApNotFound = 70014,
    DisconnectUnknownMac = 70015,

    // +4000..+5999: Client & Transport
    DelayedFrameSinkClosed = 74001,
    BeaconSinkClosed = 74002,
    StreamUnknownId = 74003,
    UnicastResponseSinkClosed = 74004,
    DeauthSendFailed = 74005,

    // +6000..+7999: Internal & State
    EapStartFailed = 76001,
    EapHandleFailed = 76002,
    WpaHandleEapolFailed = 76003,
    SaeOwnElementLenInvalid = 76004,
    SaeKPointLenInvalid = 76005,
    SaeKdfOutputLenInvalid = 76006,
    SaeKckMissing = 76007,
    // Counted to surface guest passphrase mismatches and PTK/EAPOL bugs.
    WpaM2MicInvalid = 76008,
    WpaM4MicInvalid = 76009,

    // +8000..+9999: External Backend / FFI
    SaePweDeriveFailed = 78001,
    SaeCommitGenerateFailed = 78002,
    SaePeerScalarMulFailed = 78003,
    SaePointAddFailed = 78004,
    SaeKComputeFailed = 78005,
    CcmpDecryptFailed = 78006,
}
