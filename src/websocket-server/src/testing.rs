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

//! Testing utilities for the WebSocket server.
//!
//! Defined in `src/testing.rs` under `#[cfg(feature = "testing")]` rather than `tests/` because
//! it requires `pub(crate)` handshake helpers in `crate::client` and is shared across crates
//! (`websocket-server/tests/world.rs` and `daemon/tests/world.rs`).

use std::net::TcpStream as StdTcpStream;

use bytes::Bytes;
use tokio::{io::AsyncWriteExt, net::TcpStream};
use tungstenite::{Message, WebSocket, protocol::Role};

use crate::client::{ClientError, generate_websocket_key, read_handshake_response};

/// Barebones RFC 6455 compliant WebSocket client for integration testing.
pub struct ManualWebSocketClient {
    ws: Option<WebSocket<StdTcpStream>>,
}

impl ManualWebSocketClient {
    /// Opens a WebSocket connection to `host:port` at `path` using the RFC 6455
    /// handshake with a randomly generated Sec-WebSocket-Key nonce.
    pub async fn connect(host: &str, port: u16, path: &str) -> Result<Self, ClientError> {
        let mut stream = TcpStream::connect(format!("{host}:{port}")).await?;

        let key = generate_websocket_key()?;

        let handshake = format!(
            "GET {path} HTTP/1.1\r\n\
             Host: {host}:{port}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {key}\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n",
        );

        stream.write_all(handshake.as_bytes()).await?;

        let leftover = read_handshake_response(&mut stream, &key).await?;

        // Transition to blocking std stream for tungstenite
        let std_stream = stream.into_std()?;
        std_stream.set_nonblocking(false)?;

        let ws = WebSocket::from_partially_read(std_stream, leftover, Role::Client, None);

        Ok(Self { ws: Some(ws) })
    }

    async fn with_ws<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut WebSocket<StdTcpStream>) -> R + Send + 'static,
        R: Send + 'static,
    {
        let mut ws = self.ws.take().expect("WebSocket missing");
        let (ws, out) = tokio::task::spawn_blocking(move || {
            let out = f(&mut ws);
            (ws, out)
        })
        .await
        .expect("WebSocket task panicked");
        self.ws = Some(ws);
        out
    }

    pub async fn send_binary(&mut self, data: impl Into<Bytes>) {
        let data = data.into();
        self.with_ws(move |ws| {
            ws.send(Message::Binary(data)).expect("Failed to write message");
        })
        .await;
    }

    pub async fn send_text(&mut self, text: &str) {
        let text = text.to_string();
        self.with_ws(move |ws| {
            ws.send(Message::Text(text.into())).expect("Failed to write text message");
        })
        .await;
    }

    pub async fn read_binary(&mut self) -> Bytes {
        self.with_ws(move |ws| {
            loop {
                match ws.read().expect("Failed to read message") {
                    Message::Binary(bin) => return bin,
                    Message::Ping(data) => {
                        ws.send(Message::Pong(data)).expect("Failed to pong");
                    }
                    Message::Close(_) => return Bytes::new(),
                    _ => continue,
                }
            }
        })
        .await
    }

    pub async fn close(&mut self) {
        self.with_ws(move |ws| {
            let _ = ws.close(None);
            let _ = ws.flush();
        })
        .await;
    }
}
