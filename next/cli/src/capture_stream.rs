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

//! WebSocket client for streaming live binary packet and PCAP frames from
//! `/v1/capture`, `/v1/ble/scan`, and `/v1/ble/sniff`.
//!
//! The HTTP/1.1 upgrade handshake is parsed with `httparse`; all RFC 6455
//! framing (client masking, fragmentation, Ping/Pong, Close) is delegated to
//! `tungstenite`, matching `next/websocket-server`.

use std::{
    fs::File,
    future::Future,
    io::{self, BufWriter, Write},
    net::TcpStream as StdTcpStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use netsim_rest_api::ErrorResponse;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tungstenite::{
    Message, WebSocket,
    protocol::{CloseFrame, Role, frame::coding::CloseCode},
};

use crate::{
    args::StreamCapture,
    error::{Error, Result},
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HEADERS: usize = 64;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_ERROR_BODY_LEN: usize = 4096;
// How often the blocking reader wakes up to check for `Ctrl+C`.
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(200);
// How long to wait for the server to acknowledge our Close frame.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

// Standard RFC 6455 handshake key and expected SHA-1 + Base64 accept value.
const SEC_WEBSOCKET_KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
const EXPECTED_WEBSOCKET_ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";

/// Connects to `http_server` at `path_and_query` over WebSocket and passes
/// each binary frame payload to `on_frame(&mut state, payload)` until the
/// server closes the stream or `Ctrl+C` is received.
pub(crate) fn stream_frames<T, F>(
    http_server: &str,
    path_and_query: &str,
    state: T,
    on_frame: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnMut(&mut T, &[u8]) -> io::Result<()> + Send + 'static,
{
    if http_server.is_empty() {
        return Err("HTTP server address is not available; is netsimd running?".into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::from(format!("Failed to initialize async runtime: {e}")))?;

    let shutdown = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };

    runtime.block_on(stream_frames_with_state(
        http_server,
        path_and_query,
        state,
        shutdown,
        on_frame,
    ))
}

/// Connects to `/v1/capture?chip_id=<chip_id>` over WebSocket and writes raw
/// binary PCAP frames to `--output <file>` or `stdout` until the server closes
/// the stream or `Ctrl+C` is received.
pub(crate) fn stream_capture(http_server: &str, cmd: &StreamCapture) -> Result<()> {
    let writer: Box<dyn Write + Send> = if let Some(path) = &cmd.output {
        let file = File::create(path)
            .map_err(|e| Error::from(format!("Failed to create file {}: {e}", path.display())))?;
        Box::new(BufWriter::new(file))
    } else {
        Box::new(io::stdout())
    };
    let path = format!("{}?chip_id={}", netsim_rest_api::path::CAPTURE, cmd.chip_id);
    stream_frames(http_server, &path, writer, write_and_flush).map(|_| ())
}

async fn stream_frames_with_state<T, S, F>(
    addr: &str,
    path_and_query: &str,
    mut state: T,
    shutdown: S,
    mut on_frame: F,
) -> Result<T>
where
    T: Send + 'static,
    S: Future<Output = ()>,
    F: FnMut(&mut T, &[u8]) -> io::Result<()> + Send + 'static,
{
    let (stream, leftover) =
        tokio::time::timeout(HANDSHAKE_TIMEOUT, connect_and_handshake(addr, path_and_query))
            .await
            .map_err(|e| Error::from(format!("Timed out connecting to {addr}: {e}")))??;

    // tungstenite is synchronous, so hand the socket over as a blocking std
    // stream (same approach as `next/websocket-server`).
    let stream = stream.into_std()?;
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(SHUTDOWN_POLL_INTERVAL))?;
    let mut ws = WebSocket::from_partially_read(stream, leftover, Role::Client, None);

    let stop = Arc::new(AtomicBool::new(false));
    let stop_reader = Arc::clone(&stop);
    let mut stream_task = tokio::task::spawn_blocking(move || {
        run_stream_loop(&mut ws, &stop_reader, |data| on_frame(&mut state, data)).map(|()| state)
    });

    tokio::pin!(shutdown);
    let joined = tokio::select! {
        res = &mut stream_task => res,
        () = &mut shutdown => {
            stop.store(true, Ordering::Relaxed);
            stream_task.await
        }
    };
    joined.map_err(|e| Error::from(format!("WebSocket stream task panicked: {e}")))?
}

/// Reads WebSocket messages from `ws` and invokes `on_frame` on each binary
/// payload until the server closes the connection, `stop` is set, or
/// `on_frame` reports `BrokenPipe` (e.g. Wireshark exited).
fn run_stream_loop<F: FnMut(&[u8]) -> io::Result<()>>(
    ws: &mut WebSocket<StdTcpStream>,
    stop: &AtomicBool,
    mut on_frame: F,
) -> Result<()> {
    // Set once we start (or the server starts) the closing handshake.
    let mut close_deadline: Option<Instant> = None;
    loop {
        if close_deadline.is_none() && stop.load(Ordering::Relaxed) {
            close_deadline = Some(start_close(ws));
        }
        if close_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Ok(());
        }
        match ws.read() {
            Ok(Message::Binary(data)) => {
                if close_deadline.is_some() {
                    continue;
                }
                if let Err(e) = on_frame(&data) {
                    if e.kind() != io::ErrorKind::BrokenPipe {
                        return Err(e.into());
                    }
                    close_deadline = Some(start_close(ws));
                }
            }
            Ok(Message::Text(_)) => {
                return Err("Unsupported WebSocket text frame in binary stream".into());
            }
            Ok(Message::Close(_)) => {
                // Flush the Close reply that tungstenite queued, then wait for
                // the server to drop the connection.
                let _ = ws.flush();
                if close_deadline.is_none() {
                    close_deadline = Some(Instant::now() + CLOSE_TIMEOUT);
                }
            }
            Ok(Message::Ping(_)) => {
                // Flush the Pong reply that tungstenite queued immediately.
                let _ = ws.flush();
            }
            Ok(Message::Pong(_) | Message::Frame(_)) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Ok(());
            }
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::Io(e)) if close_deadline.is_some() => {
                // The peer may drop the socket before our Close reply lands.
                tracing::debug!("Connection closed during WebSocket close: {e}");
                return Ok(());
            }
            Err(tungstenite::Error::Protocol(
                tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
            )) => return Ok(()),
            Err(e) => return Err(format!("WebSocket error: {e}").into()),
        }
    }
}

/// Sends a Close frame with status 1000 (Normal Closure) and returns the
/// deadline for the server's acknowledgement.
fn start_close(ws: &mut WebSocket<StdTcpStream>) -> Instant {
    let frame = CloseFrame { code: CloseCode::Normal, reason: Default::default() };
    if let Err(e) = ws.close(Some(frame)) {
        tracing::debug!("Failed to send WebSocket Close frame: {e}");
    }
    Instant::now() + CLOSE_TIMEOUT
}

fn write_and_flush<W: Write>(writer: &mut W, bytes: &[u8]) -> io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}

/// Opens a TCP connection to `addr` and completes the RFC 6455 HTTP/1.1
/// upgrade handshake for `path_and_query`.
///
/// Returns the stream and any bytes the server sent after the 101 response
/// headers (the start of the first WebSocket frame).
async fn connect_and_handshake(addr: &str, path_and_query: &str) -> Result<(TcpStream, Vec<u8>)> {
    let mut stream = TcpStream::connect(addr)
        .await
        .map_err(|e| Error::from(format!("Failed to connect to {addr}: {e}")))?;

    let req = format!(
        "GET {path_and_query} HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {SEC_WEBSOCKET_KEY}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await?;
    stream.flush().await?;

    let mut buf = Vec::with_capacity(1024);
    loop {
        if read_some(&mut stream, &mut buf).await? == 0 {
            return Err("Server closed connection before completing WebSocket handshake".into());
        }
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut resp = httparse::Response::new(&mut headers);
        let status = resp
            .parse(&buf)
            .map_err(|e| Error::from(format!("Invalid HTTP response from server: {e}")))?;
        let httparse::Status::Complete(header_len) = status else {
            if buf.len() > MAX_HEADER_BYTES {
                return Err("HTTP response headers exceed maximum size".into());
            }
            continue;
        };

        let code = resp.code.unwrap_or_default();
        let header = |name: &str| {
            resp.headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| String::from_utf8_lossy(h.value).trim().to_string())
        };
        let upgrade = header("upgrade");
        let accept = header("sec-websocket-accept");
        let content_length = header("content-length").and_then(|v| v.parse::<usize>().ok());

        let rest = buf.split_off(header_len);
        if code != 101 {
            let body = read_error_body(&mut stream, rest, content_length).await?;
            let message = serde_json::from_slice::<ErrorResponse>(&body)
                .ok()
                .map(|e| e.error)
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| String::from_utf8_lossy(&body).trim().to_string());
            return Err(format!("HTTP Error {code}: {message}").into());
        }
        let is_websocket = upgrade.is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
        if !is_websocket || accept.as_deref() != Some(EXPECTED_WEBSOCKET_ACCEPT) {
            return Err("Invalid WebSocket upgrade response headers".into());
        }
        return Ok((stream, rest));
    }
}

/// Appends whatever is available on `stream` to `buf`, returning the number
/// of bytes read (0 on EOF).
async fn read_some(stream: &mut TcpStream, buf: &mut Vec<u8>) -> io::Result<usize> {
    let mut chunk = [0u8; 1024];
    let n = stream.read(&mut chunk).await?;
    buf.extend_from_slice(chunk.get(..n).unwrap_or_default());
    Ok(n)
}

/// Reads a bounded non-101 response body, starting from the bytes already
/// received after the headers.
async fn read_error_body(
    stream: &mut TcpStream,
    mut body: Vec<u8>,
    content_length: Option<usize>,
) -> io::Result<Vec<u8>> {
    match content_length {
        Some(len) => {
            let limit = len.min(MAX_ERROR_BODY_LEN);
            while body.len() < limit {
                if read_some(stream, &mut body).await? == 0 {
                    break;
                }
            }
            body.truncate(limit);
        }
        None if body.is_empty() => {
            read_some(stream, &mut body).await.unwrap_or_default();
            body.truncate(MAX_ERROR_BODY_LEN);
        }
        None => body.truncate(MAX_ERROR_BODY_LEN),
    }
    Ok(body)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{io::Read, net::TcpListener, thread};

    use tokio::sync::oneshot;

    use super::*;

    const PCAP_GLOBAL_HEADER: [u8; 24] = [
        0xd4, 0xc3, 0xb2, 0xa1, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xff, 0x00, 0x00, 0xc9, 0x00, 0x00, 0x00,
    ];

    async fn stream_capture_with_writer<W, F>(
        addr: &str,
        chip_id: u32,
        writer: W,
        shutdown: F,
    ) -> Result<W>
    where
        W: Write + Send + 'static,
        F: Future<Output = ()>,
    {
        let path = format!("{}?chip_id={chip_id}", netsim_rest_api::path::CAPTURE);
        stream_frames_with_state(addr, &path, writer, shutdown, write_and_flush).await
    }

    pub(crate) fn listen() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        (listener, addr)
    }

    fn read_request(sock: &mut StdTcpStream) -> String {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = sock.read(&mut tmp).unwrap();
            assert!(n > 0, "unexpected EOF during handshake");
            buf.extend_from_slice(&tmp[..n]);
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Accepts one client, validates its upgrade request for `expected_path`,
    /// and returns a server-role WebSocket.
    pub(crate) fn accept_upgrade(
        listener: &TcpListener,
        expected_path: &str,
    ) -> WebSocket<StdTcpStream> {
        let (mut sock, _) = listener.accept().unwrap();
        let req = read_request(&mut sock);
        assert_eq!(req.lines().next().unwrap(), format!("GET {expected_path} HTTP/1.1"));
        assert!(req.contains(&format!("Sec-WebSocket-Key: {SEC_WEBSOCKET_KEY}")));
        let resp = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {EXPECTED_WEBSOCKET_ACCEPT}\r\n\r\n"
        );
        sock.write_all(resp.as_bytes()).unwrap();
        WebSocket::from_raw_socket(sock, Role::Server, None)
    }

    /// Reads until the client sends a Close frame and returns its status code.
    fn expect_client_close(ws: &mut WebSocket<StdTcpStream>) -> Option<CloseCode> {
        loop {
            match ws.read() {
                Ok(Message::Close(frame)) => return frame.map(|f| f.code),
                Ok(_) => {}
                Err(e) => panic!("expected Close frame from client, got {e}"),
            }
        }
    }

    /// Reads until tungstenite reports the closing handshake as complete.
    pub(crate) fn drain_until_closed(ws: &mut WebSocket<StdTcpStream>) {
        while ws.read().is_ok() {}
    }

    #[tokio::test]
    async fn streams_pcap_global_header_and_records_until_server_close() {
        let (listener, addr) = listen();
        let record1 = b"\x01\x00\x00\x00\x02\x00\x00\x00\x04\x00\x00\x00\x04\x00\x00\x00\xaa\xbb";
        let record2 = b"\x03\x00\x00\x00\x04\x00\x00\x00\x02\x00\x00\x00\x02\x00\x00\x00\x11\x22";

        let server = thread::spawn(move || {
            let mut ws = accept_upgrade(&listener, "/v1/capture?chip_id=7");
            ws.send(Message::binary(PCAP_GLOBAL_HEADER.to_vec())).unwrap();
            ws.send(Message::binary(record1.to_vec())).unwrap();
            ws.send(Message::binary(record2.to_vec())).unwrap();
            ws.close(None).unwrap();
            drain_until_closed(&mut ws);
        });

        let output =
            stream_capture_with_writer(&addr, 7, Vec::new(), std::future::pending()).await.unwrap();
        server.join().unwrap();

        let mut expected = PCAP_GLOBAL_HEADER.to_vec();
        expected.extend_from_slice(record1);
        expected.extend_from_slice(record2);
        assert_eq!(output, expected);
    }

    #[tokio::test]
    async fn replies_to_ping_with_pong_and_continues_streaming() {
        let (listener, addr) = listen();

        let server = thread::spawn(move || {
            let mut ws = accept_upgrade(&listener, "/v1/capture?chip_id=1");
            ws.send(Message::binary(PCAP_GLOBAL_HEADER.to_vec())).unwrap();
            ws.send(Message::Ping(b"keepalive".to_vec().into())).unwrap();
            loop {
                match ws.read().unwrap() {
                    Message::Pong(payload) => {
                        assert_eq!(&payload[..], b"keepalive");
                        break;
                    }
                    other => panic!("expected Pong, got {other:?}"),
                }
            }
            ws.send(Message::binary(vec![0x5a; 200])).unwrap();
            ws.close(None).unwrap();
            drain_until_closed(&mut ws);
        });

        let output =
            stream_capture_with_writer(&addr, 1, Vec::new(), std::future::pending()).await.unwrap();
        server.join().unwrap();

        assert_eq!(output.len(), PCAP_GLOBAL_HEADER.len() + 200);
        assert_eq!(&output[..24], &PCAP_GLOBAL_HEADER);
        assert!(output[24..].iter().all(|&b| b == 0x5a));
    }

    #[tokio::test]
    async fn graceful_shutdown_sends_normal_close_and_exits_cleanly() {
        let (listener, addr) = listen();
        let (header_seen_tx, header_seen_rx) = oneshot::channel::<()>();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let server = thread::spawn(move || {
            let mut ws = accept_upgrade(&listener, "/v1/capture?chip_id=3");
            ws.send(Message::binary(PCAP_GLOBAL_HEADER.to_vec())).unwrap();
            let _ = header_seen_rx.blocking_recv();
            let _ = shutdown_tx.send(());

            assert_eq!(expect_client_close(&mut ws), Some(CloseCode::Normal));
            drain_until_closed(&mut ws);
        });

        struct NotifyingWriter {
            buf: Vec<u8>,
            notify: Option<oneshot::Sender<()>>,
        }
        impl Write for NotifyingWriter {
            fn write(&mut self, data: &[u8]) -> io::Result<usize> {
                self.buf.extend_from_slice(data);
                Ok(data.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                if let Some(tx) = self.notify.take() {
                    let _ = tx.send(());
                }
                Ok(())
            }
        }

        let writer = NotifyingWriter { buf: Vec::new(), notify: Some(header_seen_tx) };
        let writer = stream_capture_with_writer(&addr, 3, writer, async {
            let _ = shutdown_rx.await;
        })
        .await
        .unwrap();

        server.join().unwrap();
        assert_eq!(writer.buf, PCAP_GLOBAL_HEADER);
    }

    #[tokio::test]
    async fn surfaces_http_error_when_chip_not_found() {
        let (listener, addr) = listen();

        let server = thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let _ = read_request(&mut sock);
            let body = r#"{"error":"No capture for chip 999"}"#;
            let resp = format!(
                "HTTP/1.1 404 Not Found\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\r\n\
                 {body}",
                body.len()
            );
            sock.write_all(resp.as_bytes()).unwrap();
        });

        let err = stream_capture_with_writer(&addr, 999, Vec::new(), std::future::pending())
            .await
            .err()
            .unwrap();
        server.join().unwrap();
        assert!(
            err.to_string().contains("404") && err.to_string().contains("No capture for chip 999"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn broken_pipe_on_stdout_closes_and_exits_cleanly() {
        let (listener, addr) = listen();

        let server = thread::spawn(move || {
            let mut ws = accept_upgrade(&listener, "/v1/capture?chip_id=2");
            ws.send(Message::binary(PCAP_GLOBAL_HEADER.to_vec())).unwrap();
            ws.send(Message::binary(b"extra-frame-1".to_vec())).unwrap();
            assert_eq!(expect_client_close(&mut ws), Some(CloseCode::Normal));
            drain_until_closed(&mut ws);
        });

        struct BrokenPipeWriter;
        impl Write for BrokenPipeWriter {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        stream_capture_with_writer(&addr, 2, BrokenPipeWriter, std::future::pending())
            .await
            .unwrap();
        server.join().unwrap();
    }

    #[test]
    fn stream_capture_writes_to_output_file() {
        let (listener, addr) = listen();
        let server = thread::spawn(move || {
            let mut ws = accept_upgrade(&listener, "/v1/capture?chip_id=5");
            ws.send(Message::binary(PCAP_GLOBAL_HEADER.to_vec())).unwrap();
            ws.close(None).unwrap();
            drain_until_closed(&mut ws);
        });

        let out_path =
            std::env::temp_dir().join(format!("netsim_capture_stream_{}.pcap", std::process::id()));
        let cmd = StreamCapture { chip_id: 5, output: Some(out_path.clone()) };
        stream_capture(&addr, &cmd).unwrap();
        server.join().unwrap();

        let written = std::fs::read(&out_path).unwrap();
        let _ = std::fs::remove_file(&out_path);
        assert_eq!(written, PCAP_GLOBAL_HEADER);
    }

    #[tokio::test]
    async fn rejects_text_frame_in_stream() {
        let (listener, addr) = listen();

        let server = thread::spawn(move || {
            let mut ws = accept_upgrade(&listener, "/v1/capture?chip_id=11");
            ws.send(Message::text("hello text".to_string())).unwrap();
            drain_until_closed(&mut ws);
        });

        let res = stream_capture_with_writer(&addr, 11, Vec::new(), std::future::pending()).await;
        server.join().unwrap();
        assert!(res.unwrap_err().to_string().contains("Unsupported WebSocket text frame"));
    }
}
