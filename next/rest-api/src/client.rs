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

//! HTTP/1.1 REST Client for Netsim daemon, modeled on `reqwest`'s builder API.
//!
//! ```ignore
//! let devices = client.get_devices().send().await?;               // async
//! let aps = client.get_aps().send_blocking()?;                    // blocking
//! let dev: Device = client.post("/v1/devices").json(&create).send().await?;
//! client.delete("/v1/devices/1").send_blocking()?;
//! ```

use std::{
    fmt,
    io::{Read, Write},
    marker::PhantomData,
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

use hyper::{Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{ErrorResponse, ListApResponse, ListDeviceResponse, ListLinkResponse, VersionResponse};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Errors returned by [`NetsimRestClient`].
#[derive(Debug, thiserror::Error)]
pub enum RestError {
    #[error("Failed to connect to {0}: {1}")]
    Connect(String, #[source] std::io::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Request timed out")]
    Timeout(#[from] tokio::time::error::Elapsed),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid HTTP response: {0}")]
    InvalidResponse(&'static str),
    #[error("HTTP Error {}: {message}", .status.as_u16())]
    Status { status: StatusCode, message: String },
}

impl RestError {
    /// The HTTP status code if the server replied with a non-2xx status,
    /// like `reqwest::Error::status()`.
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Status { status, .. } => Some(*status),
            _ => None,
        }
    }
}

/// Client for interacting with the Netsim REST API over HTTP/1.1.
#[derive(Clone, Debug)]
pub struct NetsimRestClient {
    addr: String,
}

impl NetsimRestClient {
    /// Creates a new `NetsimRestClient` targeting `host:port`.
    ///
    /// `host` must be a hostname or IP literal without a port (e.g.
    /// `"localhost"`, `"127.0.0.1"`, `"::1"`, or `"[::1]"`).
    pub fn new(host: &str, port: u16) -> Self {
        let addr = if host.parse::<std::net::Ipv6Addr>().is_ok() {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        Self { addr }
    }

    /// Creates a new `NetsimRestClient` from an existing `host:port` address string.
    pub fn from_addr(addr: impl Into<String>) -> Self {
        Self { addr: addr.into() }
    }

    /// Starts building a request whose JSON response decodes into `T`.
    pub fn request<T>(&self, method: Method, path: impl Into<String>) -> RestRequest<'_, T> {
        RestRequest { client: self, method, path: path.into(), body: Ok(None), _t: PhantomData }
    }

    pub fn get<T>(&self, path: impl Into<String>) -> RestRequest<'_, T> {
        self.request(Method::GET, path)
    }

    pub fn post<T>(&self, path: impl Into<String>) -> RestRequest<'_, T> {
        self.request(Method::POST, path)
    }

    pub fn patch<T>(&self, path: impl Into<String>) -> RestRequest<'_, T> {
        self.request(Method::PATCH, path)
    }

    pub fn delete(&self, path: impl Into<String>) -> RestRequest<'_, ()> {
        self.request(Method::DELETE, path)
    }

    /// `GET /v1/version`
    pub fn get_version(&self) -> RestRequest<'_, VersionResponse> {
        self.get(crate::path::VERSION)
    }

    /// `GET /v1/devices`
    pub fn get_devices(&self) -> RestRequest<'_, ListDeviceResponse> {
        self.get(crate::path::DEVICES)
    }

    /// `GET /v1/links`
    pub fn get_links(&self) -> RestRequest<'_, ListLinkResponse> {
        self.get(crate::path::LINKS)
    }

    /// `GET /v1/aps`
    pub fn get_aps(&self) -> RestRequest<'_, ListApResponse> {
        self.get(crate::path::APS)
    }
}

/// A pending REST request. Run it with `.send().await`, or
/// `.send_blocking()` from synchronous code.
#[derive(Debug)]
#[must_use = "a RestRequest does nothing until .send() or .send_blocking()"]
pub struct RestRequest<'a, T> {
    client: &'a NetsimRestClient,
    method: Method,
    path: String,
    body: Result<Option<Vec<u8>>, serde_json::Error>,
    _t: PhantomData<fn() -> T>,
}

/// Formats the HTTP/1.1 request line and headers (everything before the body).
impl<T> fmt::Display for RestRequest<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} HTTP/1.1\r\nHost: {}\r\n", self.method, self.path, self.client.addr)?;
        if let Ok(Some(body)) = &self.body {
            write!(f, "Content-Type: application/json\r\nContent-Length: {}\r\n", body.len())?;
        }
        write!(f, "Connection: close\r\n\r\n")
    }
}

impl<'a, T: DeserializeOwned> RestRequest<'a, T> {
    /// Sets a JSON request body.
    pub fn json<B: Serialize + ?Sized>(mut self, body: &B) -> Self {
        self.body = serde_json::to_vec(body).map(Some);
        self
    }

    /// Sends the request asynchronously.
    pub async fn send(self) -> Result<T, RestError> {
        let (addr, req) = self.encode()?;
        // One deadline covers connect, write, and read, matching send_blocking().
        let buf = tokio::time::timeout(REQUEST_TIMEOUT, async {
            let mut stream = tokio::net::TcpStream::connect(addr)
                .await
                .map_err(|e| RestError::Connect(addr.to_string(), e))?;
            stream.write_all(&req).await?;
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await?;
            Ok::<_, RestError>(buf)
        })
        .await??;
        decode(&buf)
    }

    /// Sends the request synchronously.
    pub fn send_blocking(self) -> Result<T, RestError> {
        let (addr, req) = self.encode()?;
        // Try every resolved address (e.g. `[::1]` then `127.0.0.1` for
        // `localhost`), like `tokio::net::TcpStream::connect` in `send()`.
        let connect = || {
            let mut last_err = None;
            for socket_addr in addr.to_socket_addrs()? {
                match TcpStream::connect_timeout(&socket_addr, REQUEST_TIMEOUT) {
                    Ok(stream) => return Ok(stream),
                    Err(e) => last_err = Some(e),
                }
            }
            Err(last_err.unwrap_or_else(|| std::io::ErrorKind::NotFound.into()))
        };
        let mut stream = connect().map_err(|e| RestError::Connect(addr.to_string(), e))?;
        stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
        stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
        stream.write_all(&req)?;
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf)?;
        decode(&buf)
    }

    /// Returns the target address and the serialized request (head + body).
    fn encode(self) -> Result<(&'a str, Vec<u8>), RestError> {
        let client = self.client;
        let mut req = self.to_string().into_bytes();
        req.extend(self.body?.unwrap_or_default());
        Ok((&client.addr, req))
    }
}

/// Parses a raw HTTP/1.1 response. Non-2xx statuses become
/// [`RestError::Status`] carrying the server's [`ErrorResponse`] message when
/// present; an empty 2xx body decodes as JSON `null` so `T = ()` works.
fn decode<T: DeserializeOwned>(buf: &[u8]) -> Result<T, RestError> {
    let resp = String::from_utf8_lossy(buf);
    let (head, body) = resp
        .split_once("\r\n\r\n")
        .ok_or(RestError::InvalidResponse("missing header/body separator"))?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| StatusCode::from_bytes(s.as_bytes()).ok())
        .ok_or(RestError::InvalidResponse("malformed status line"))?;
    if !status.is_success() {
        let message = serde_json::from_str::<ErrorResponse>(body)
            .ok()
            .map(|e| e.error)
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| body.to_string());
        return Err(RestError::Status { status, message });
    }
    let body = body.trim();
    Ok(serde_json::from_str(if body.is_empty() { "null" } else { body })?)
}
