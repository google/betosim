// Copyright 2025 The Android Open Source Project
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

//! # Wi-Fi Error Handling
//!
//! This module defines the `WifiError` enum, which represents various error conditions that can occur
//! within the Wi-Fi module. It also provides a `WifiResult` type alias for convenient error
//! handling.
use pdl_runtime::{DecodeError, EncodeError};

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum WifiError {
    /// Errors related to the hostapd.
    Hostapd(String),
    /// Errors related to network connectivity (e.g., slirp).
    Network(String),
    /// Errors related to client-specific operations or state.
    Client(String),
    /// Errors encountered while parsing, decoding, or handling IEEE 802.11 frames.
    Frame(String),
    /// Errors related to transmission or reception of frame.
    Transmission(String),
    /// Other uncategorized errors.
    Other(String),
}

impl std::fmt::Display for WifiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WifiError::Hostapd(msg) => write!(f, "Hostapd error: {msg}"),
            WifiError::Network(msg) => write!(f, "Network error: {msg}"),
            WifiError::Client(msg) => write!(f, "Client error: {msg}"),
            WifiError::Frame(msg) => write!(f, "Frame error: {msg}"),
            WifiError::Transmission(msg) => write!(f, "Transmission error: {msg}"),
            WifiError::Other(msg) => write!(f, "Other error: {msg}"),
        }
    }
}

impl std::error::Error for WifiError {}

#[cfg(not(feature = "cuttlefish"))]
impl From<http_proxy::Error> for WifiError {
    fn from(err: http_proxy::Error) -> Self {
        WifiError::Network(format!("HTTP proxy error: {:?}", err))
    }
}

impl From<std::io::Error> for WifiError {
    fn from(err: std::io::Error) -> Self {
        WifiError::Network(format!("IO error: {err:?}"))
    }
}

impl From<DecodeError> for WifiError {
    fn from(err: DecodeError) -> Self {
        WifiError::Frame(format!("Frame decoding failed: {err:?}"))
    }
}

impl From<EncodeError> for WifiError {
    fn from(err: EncodeError) -> Self {
        WifiError::Frame(format!("Frame encoding failed: {err:?}"))
    }
}

pub type WifiResult<T> = Result<T, WifiError>;
