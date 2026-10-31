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
    fs::File,
    io::{self, BufRead, BufReader, Read, Write},
    marker::PhantomData,
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    time::Duration,
};

use hyper::{Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{ErrorResponse, ListApResponse, ListLinkResponse, VersionResponse};

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

    /// `GET /v1/links`
    pub fn get_links(&self) -> RestRequest<'_, ListLinkResponse> {
        self.get(crate::path::LINKS)
    }

    /// `GET /v1/aps`
    pub fn get_aps(&self) -> RestRequest<'_, ListApResponse> {
        self.get(crate::path::APS)
    }

    /// `POST /v1/aps`
    pub fn create_ap(&self, req: &netsim_model::ApCreate) -> RestRequest<'_, netsim_model::Ap> {
        self.post(crate::path::APS).json(req)
    }

    /// `PATCH /v1/aps/{id}`
    pub fn patch_ap(
        &self,
        id: u32,
        req: &netsim_model::ApUpdate,
    ) -> RestRequest<'_, netsim_model::Ap> {
        self.patch(format!("{}/{id}", crate::path::APS)).json(req)
    }

    /// `DELETE /v1/aps/{id}`
    pub fn delete_ap(&self, id: u32) -> RestRequest<'_, ()> {
        self.delete(format!("{}/{id}", crate::path::APS))
    }

    /// `POST /v1/aps/{id}:disconnect`
    pub fn disconnect_ap(&self, id: u32, req: &crate::DisconnectApRequest) -> RestRequest<'_, ()> {
        self.post(format!("{}/{id}:disconnect", crate::path::APS)).json(req)
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

impl<'a, T> RestRequest<'a, T> {
    /// Sets a JSON request body.
    pub fn json<B: Serialize + ?Sized>(mut self, body: &B) -> Self {
        self.body = serde_json::to_vec(body).map(Some);
        self
    }

    /// Sends the request asynchronously and returns the raw response body bytes.
    async fn send_bytes(self) -> Result<Vec<u8>, RestError> {
        let (addr, req) = self.encode()?;
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
        let mut body = Vec::new();
        read_response(buf.as_slice())?.copy_to(&mut body)?;
        Ok(body)
    }

    /// Sends the request synchronously and returns the raw response body bytes.
    fn send_bytes_blocking(self) -> Result<Vec<u8>, RestError> {
        let mut body = Vec::new();
        self.send_streaming_blocking()?.copy_to(&mut body)?;
        Ok(body)
    }

    /// Sends the request and reads the response head, leaving the body unread.
    fn send_streaming_blocking(self) -> Result<StreamingBody<BufReader<TcpStream>>, RestError> {
        let (addr, req) = self.encode()?;
        let mut stream = connect_blocking(addr)?;
        stream.write_all(&req)?;
        read_response(BufReader::new(stream))
    }

    /// Returns the target address and the serialized request (head + body).
    fn encode(self) -> Result<(&'a str, Vec<u8>), RestError> {
        let client = self.client;
        let mut req = self.to_string().into_bytes();
        req.extend(self.body?.unwrap_or_default());
        Ok((&client.addr, req))
    }
}

impl RestRequest<'_, crate::Pcap> {
    /// Sends the request synchronously and streams the raw response body into
    /// a file at `path`. The body is written to `<path>.part` and renamed to
    /// `path` only once it is complete, so a non-2xx status or a short body
    /// leaves any existing file at `path` untouched.
    pub fn send_to_file_blocking(self, path: &Path) -> Result<u64, RestError> {
        let body = self.send_streaming_blocking()?;
        let mut partial = path.as_os_str().to_owned();
        partial.push(".part");
        let partial = PathBuf::from(partial);
        File::create(&partial)
            .map_err(RestError::from)
            .and_then(|mut file| body.copy_to(&mut file))
            .and_then(|copied| {
                std::fs::rename(&partial, path).map(|()| copied).map_err(RestError::from)
            })
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&partial);
            })
    }
}

impl<'a, T: DeserializeOwned> RestRequest<'a, T> {
    /// Sends the request asynchronously.
    pub async fn send(self) -> Result<T, RestError> {
        let body = self.send_bytes().await?;
        decode_json(&body)
    }

    /// Sends the request synchronously.
    pub fn send_blocking(self) -> Result<T, RestError> {
        let body = self.send_bytes_blocking()?;
        decode_json(&body)
    }
}

/// The unread body of a response.
struct StreamingBody<R> {
    reader: R,
    content_length: Option<u64>,
}

impl<R: BufRead> StreamingBody<R> {
    /// Copies the body into `writer`, failing if it is shorter than the
    /// response's `Content-Length`.
    fn copy_to<W: Write>(mut self, writer: &mut W) -> Result<u64, RestError> {
        let copied = match self.content_length {
            Some(len) => {
                let copied = io::copy(&mut Read::take(self.reader, len), writer)?;
                if copied < len {
                    return Err(RestError::InvalidResponse(
                        "response body shorter than Content-Length",
                    ));
                }
                copied
            }
            None => io::copy(&mut self.reader, writer)?,
        };
        writer.flush()?;
        Ok(copied)
    }
}

/// Reads the response head. Returns the unread body for a 2xx status, or the
/// decoded [`RestError::Status`] otherwise.
fn read_response<R: BufRead>(mut reader: R) -> Result<StreamingBody<R>, RestError> {
    let (status, content_length) = read_head(&mut reader)?;
    let body = StreamingBody { reader, content_length };
    if !status.is_success() {
        let mut message = Vec::new();
        body.copy_to(&mut message)?;
        return Err(decode_error(status, &message));
    }
    Ok(body)
}

/// Reads the status line and headers up to the blank line, returning the
/// status and `Content-Length` (if present).
fn read_head(reader: &mut impl BufRead) -> Result<(StatusCode, Option<u64>), RestError> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let status = parse_status(&line)?;
    let mut content_length = None;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err(RestError::InvalidResponse("missing header/body separator"));
        }
        let line = line.trim_end();
        if line.is_empty() {
            return Ok((status, content_length));
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(RestError::InvalidResponse("Transfer-Encoding is not supported"));
        }
        if name.eq_ignore_ascii_case("content-length") {
            let len = value
                .trim()
                .parse::<u64>()
                .map_err(|_err| RestError::InvalidResponse("malformed Content-Length header"))?;
            content_length = Some(len);
        }
    }
}

fn connect_blocking(addr: &str) -> Result<TcpStream, RestError> {
    // Try every resolved address (e.g. `[::1]` then `127.0.0.1` for
    // `localhost`), like `tokio::net::TcpStream::connect` in `send_bytes()`.
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
    let stream = connect().map_err(|e| RestError::Connect(addr.to_string(), e))?;
    stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
    stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
    Ok(stream)
}

fn parse_status(head_str: &str) -> Result<StatusCode, RestError> {
    head_str
        .split_whitespace()
        .nth(1)
        .and_then(|s| StatusCode::from_bytes(s.as_bytes()).ok())
        .ok_or(RestError::InvalidResponse("malformed status line"))
}

fn decode_error(status: StatusCode, body: &[u8]) -> RestError {
    let body_str = String::from_utf8_lossy(body);
    let message = serde_json::from_str::<ErrorResponse>(&body_str)
        .ok()
        .map(|e| e.error)
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| body_str.into_owned());
    RestError::Status { status, message }
}

/// Parses a JSON response body. An empty 2xx body decodes as JSON `null` so
/// `T = ()` works.
fn decode_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, RestError> {
    let body = String::from_utf8_lossy(body);
    let body = body.trim();
    Ok(serde_json::from_str(if body.is_empty() { "null" } else { body })?)
}
