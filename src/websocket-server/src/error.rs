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

use http::header::{HeaderName, ToStrError};
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ServerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP parse error: {0}")]
    HttpParse(#[from] httparse::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] http::Error),
    #[error("Header to string error: {0}")]
    HeaderToStr(#[from] ToStrError),
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("Anyhow error: {0}")]
    Anyhow(#[from] anyhow::Error),
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("Invalid path: {0}")]
    InvalidPath(String),
    #[error("Missing header: {0}")]
    MissingHeader(HeaderName),
    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),
    #[error("Device actor error: {0}")]
    DeviceActor(#[from] netsim_model::ClientError),
}
