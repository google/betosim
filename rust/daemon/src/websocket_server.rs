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

use crate::http_server::thread_pool::ThreadPool;
use crate::transport::websocket::{generate_websocket_accept, run_websocket_transport};

use anyhow::Result;
use http::{Response, StatusCode};
use log::{info, warn};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};

static TARGET_PATH: &str = "/v1/websocket/bt";

fn bind_listener(websocket_port: u16) -> Result<TcpListener> {
    Ok(TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, websocket_port)))
        .or_else(|e| {
            warn!("Failed to bind to 127.0.0.1:{websocket_port} in netsimd frontend http server. Trying [::1]:{websocket_port}. {e:?}");
            TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, websocket_port)))
        })?)
}

fn handle_websocket(req: &httparse::Request) -> Result<Response<Vec<u8>>> {
    info!("{:?}", req.headers);
    let websocket_accept =
        match req.headers.iter().find(|header| header.name.to_lowercase() == "sec-websocket-key") {
            Some(header) => {
                let key_str: &str = core::str::from_utf8(header.value)?;
                generate_websocket_accept(key_str.to_string())
            }
            None => {
                return Err(anyhow::anyhow!("Missing Sec-Websocket-Key in header"));
            }
        };
    Ok(Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Accept", websocket_accept)
        .header("Connection", "Upgrade")
        .body(Vec::new())?)
}

fn handle_connection(mut stream: TcpStream) -> Result<()> {
    // HTTP Request Parsing
    let mut buffer = vec![0u8; 2048];
    // Use blocking read from std::io::Read
    let bytes_read = stream.read(&mut buffer)?;
    if bytes_read == 0 {
        warn!("Client disconnected before sending data.");
        return Ok(());
    }

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    let parse_status = req.parse(&buffer[..bytes_read])?;

    if parse_status.is_partial() {
        warn!("Request too large for buffer or incomplete.");
        // Use blocking write_all and flush from std::io::Write
        stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")?;
        stream.flush()?;
        return Ok(());
    }

    // Endpoint Validation
    let path_str = match req.path {
        Some(p) => p,
        None => {
            warn!("No path in request.");
            // Use blocking write_all and flush
            stream.write_all(
                b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            )?;
            stream.flush()?;
            return Err(anyhow::anyhow!("HTTP request path is missing"));
        }
    };
    let (base_path, query_string_opt) = match path_str.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (path_str, None),
    };

    if base_path != TARGET_PATH {
        warn!("Invalid path in request: {base_path}");
        let response_str =
            "HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".to_string();
        // Use blocking write_all and flush
        stream.write_all(response_str.as_bytes())?;
        stream.flush()?;
        return Ok(()); // Request handled (rejected), not a server error
    }

    // Parse optional query
    let mut queries = HashMap::new();
    if let Some(query_string) = query_string_opt {
        if !query_string.is_empty() {
            for pair in query_string.split('&') {
                if pair.is_empty() {
                    continue;
                }
                if let Some((key, value)) = pair.split_once('=') {
                    // Note: In a real application, you'd want to URL-decode keys and values
                    queries.insert(key, value);
                }
            }
        }
    }

    // Proceed Websocket header check using the synchronous handler
    let websocket_response = handle_websocket(&req)?;

    if websocket_response.status() == StatusCode::SWITCHING_PROTOCOLS {
        let status = websocket_response.status();
        let headers = websocket_response.headers();
        let body = websocket_response.body();
        let status_line =
            format!("HTTP/1.1 {} {}\r\n", status.as_u16(), status.canonical_reason().unwrap_or(""));
        stream.write_all(status_line.as_bytes())?;
        for (name, value) in headers.iter() {
            let header_line = format!("{}: {}\r\n", name, value.to_str()?);
            stream.write_all(header_line.as_bytes())?;
        }
        stream.write_all(b"\r\n")?;
        stream.write_all(body)?;
        stream.flush()?;

        // Now, the synchronous WebSocket transport takes over this blocking thread
        run_websocket_transport(stream, queries);
    } else {
        // Send a bad request response if not a WebSocket upgrade
        stream.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")?;
        stream.flush()?;
    }
    Ok(())
}

pub fn run_websocket_server(instance_num: u16) -> Result<u16> {
    let websocket_port = std::env::var("NETSIM_WS_PORT")?.parse::<u16>()? + instance_num - 1;
    let _ = std::thread::Builder::new().name("ws_server".to_string()).spawn(move || {
        let listener = match bind_listener(websocket_port) {
            Ok(listener) => listener,
            Err(e) => {
                warn!("{e:?}");
                return;
            }
        };
        let pool = ThreadPool::new(4);
        info!("Websocket server is listening on http://localhost:{websocket_port}");
        for stream in listener.incoming() {
            let stream = stream.unwrap();
            pool.execute(move || {
                let _ = handle_connection(stream).map_err(|e| warn!("{e:?}"));
            })
        }
    });
    Ok(websocket_port)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Cursor;

    #[test]
    fn test_handle_websocket_valid() {
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = httparse::Request::new(&mut headers);
        let request_str =
            "GET /v1/websocket/bt HTTP/1.1\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";
        let buffer = Cursor::new(request_str.as_bytes());
        let bytes_read = buffer.get_ref().len();
        let parse_status = req.parse(&buffer.get_ref()[..bytes_read]).unwrap();
        assert!(parse_status.is_complete());
        let response = handle_websocket(&req).unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        assert_eq!(response.headers().get("Upgrade").unwrap(), "websocket");
        assert_eq!(
            response.headers().get("Sec-WebSocket-Accept").unwrap(),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
        assert_eq!(response.headers().get("Connection").unwrap(), "Upgrade");
    }

    #[test]
    fn test_handle_websocket_missing_key() {
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = httparse::Request::new(&mut headers);
        let request_str = "GET /v1/websocket/bt HTTP/1.1\r\n\r\n";
        let buffer = Cursor::new(request_str.as_bytes());
        let bytes_read = buffer.get_ref().len();
        let parse_status = req.parse(&buffer.get_ref()[..bytes_read]).unwrap();
        assert!(parse_status.is_complete());
        let response = handle_websocket(&req);
        assert!(response.is_err());
    }
}
