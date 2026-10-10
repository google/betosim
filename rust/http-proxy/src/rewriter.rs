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

//! # HTTP Request Rewriter
//!
//! This module provides functionality to parse and rewrite an HTTP request
//! from its `origin-form` (style used for direct communication with a server)
//! to its `absolute-form` (style required for proxying).
//!
//! ## Key Components
//!
//! - **`rewrite_request_to_absolute_form`**: The primary function that reads from a
//!   `BufRead` stream and performs the transformation.
//! - **`Error`**: An enum that defines possible errors, such as I/O
//!   issues, malformed requests, or a missing `Host` header.
//!
//! This is typically used in the core logic of an HTTP proxy server.

use crate::{Error, Result};
use log::warn;
use std::net::SocketAddr;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

// --- Core Rewriting Function ---

/// Reads an HTTP request, rewriting the target from origin-form to absolute-form.
///
/// ## HTTP Request Target Forms Explained
///
/// An HTTP request-line is `METHOD TARGET HTTP_VERSION`. The `TARGET` has several forms:
///
/// 1.  **Origin-Form (Direct Style to Server)**
///     - This is the most common form, sent to an origin server.
///     - The target only contains the resource path and query string.
///     - Example: `GET /path/to/resource.html HTTP/1.1`
///
/// 2.  **Absolute-Form (Proxy Style)**
///     - This form is required when sending a request to an HTTP proxy.
///     - The target must be the full URI so the proxy knows which server to contact.
///     - Example: `GET http://www.example.com/path/to/resource.html HTTP/1.1`
///
/// This function performs the conversion from **origin-form** to **absolute-form**.
///
/// # Arguments
///
/// * `reader` - A mutable reference to a buffered reader containing the raw HTTP request.
///
/// # Returns
///
/// A `Result` containing either:
/// - `Ok(String)`: The rewritten request (new request-line + original headers).
/// - `Err(Error)`: An error that occurred during processing.
async fn rewrite_request_to_absolute_form<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    auth_header: Option<String>,
) -> Result<String> {
    let mut header_lines = Vec::new();
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;
        if line.trim().is_empty() || bytes_read == 0 {
            break;
        }
        header_lines.push(line);
    }

    // --- Try to find a valid Host header ---
    let host = header_lines
        .iter()
        .find(|h| h.to_lowercase().starts_with("host:"))
        .and_then(|h| h.split_once(':'))
        .map(|(_, value)| value.trim())
        .filter(|v| !v.is_empty());

    // --- Rewrite if Host is present, otherwise pass through ---
    let final_request_line = if let Some(host) = host {
        let request_parts: Vec<&str> = request_line.split_whitespace().collect();
        if request_parts.len() != 3 {
            return Err(Error::MalformedRequestLine(request_line.trim_end().to_string()));
        }
        let method = request_parts[0];
        let path = request_parts[1];
        let version = request_parts[2];
        let absolute_uri = format!("http://{}{}", host, path);
        format!("{} {} {}\r\n", method, absolute_uri, version)
    } else {
        // If no valid Host header, use the original request line.
        warn!("No valid Host header found. Passing request through without rewrite.");
        request_line
    };

    if let Some(auth_header) = auth_header {
        header_lines.push(auth_header);
    }

    // --- Assemble the final request ---
    let mut final_request = String::new();
    final_request.push_str(&final_request_line);
    for header in header_lines {
        final_request.push_str(&header);
    }
    final_request.push_str("\r\n");

    Ok(final_request)
}

/// Creates a proxy stream that forwards data to a given server SocketAddr
/// after performing a header rewrite on the initial data.
///
/// This function first connects to the destination to ensure it's available,
/// then sets up an intermediate TCP pipe and returns a stream that the caller can write to.
///
/// # Arguments
/// * `destination_addr`: The `SocketAddr` of the final destination server.
pub async fn connect_with_header_rewrite(
    proxy_addr: SocketAddr,
    auth_header: Option<String>,
) -> Result<TcpStream> {
    // If a proxy is specified, connect there. Otherwise, connect to the original destination.
    let connect_addr = proxy_addr;

    // 1. Connect to the next hop (either proxy or final destination).
    let mut destination_stream = TcpStream::connect(connect_addr).await?;

    // 2. Create the intermediate listener on a random port.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let pipe_addr = listener.local_addr()?;

    // 3. Spawn the background bridge task.
    tokio::spawn(async move {
        let task_logic = async {
            let (pipe_server_stream, _client_addr) = listener.accept().await?;
            let mut reader = BufReader::new(pipe_server_stream);

            // Peek at the buffer to see if there's data to read without consuming it.
            let buffer = reader.fill_buf().await?;
            if buffer.is_empty() {
                return Ok::<_, Error>(());
            }

            let new_header = rewrite_request_to_absolute_form(&mut reader, auth_header).await?;
            destination_stream.write_all(new_header.as_bytes()).await?;
            tokio::io::copy_bidirectional(&mut reader, &mut destination_stream).await?;
            Ok::<_, Error>(())
        };

        if let Err(e) = task_logic.await {
            warn!("Proxy bridge task failed: {}", e);
        }
    });

    // 4. Connect to the intermediate listener and return the client-side stream.
    let pipe_client_stream = TcpStream::connect(pipe_addr).await?;
    Ok(pipe_client_stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Connector;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn test_rewrite_passes_on_missing_host_header() {
        let request = b"GET /test HTTP/1.1\r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let result = rewrite_request_to_absolute_form(&mut reader, None).await.unwrap();
        assert_eq!(result, "GET /test HTTP/1.1\r\n\r\n");
    }

    /// Tests a standard, well-formed GET request.
    #[tokio::test]
    async fn test_successful_get_request_rewrite() {
        let request =
            b"GET /path/to/page.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let result = rewrite_request_to_absolute_form(&mut reader, None).await.unwrap();
        let expected = "GET http://example.com/path/to/page.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\n\r\n";
        assert_eq!(result, expected);
    }

    /// Tests that methods other than GET, like POST, are handled correctly.
    #[tokio::test]
    async fn test_successful_post_request_rewrite() {
        let request =
            b"POST /api/v1/users HTTP/1.1\r\nHost: api.service.io\r\nContent-Length: 42\r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let result = rewrite_request_to_absolute_form(&mut reader, None).await.unwrap();
        let expected = "POST http://api.service.io/api/v1/users HTTP/1.1\r\nHost: api.service.io\r\nContent-Length: 42\r\n\r\n";
        assert_eq!(result, expected);
    }

    /// Verifies that the 'Host' header key is treated as case-insensitive.
    #[tokio::test]
    async fn test_host_header_is_case_insensitive() {
        let request = b"GET / HTTP/1.1\r\nhOsT: case-matters-not.com\r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let result = rewrite_request_to_absolute_form(&mut reader, None).await.unwrap();
        let expected =
            "GET http://case-matters-not.com/ HTTP/1.1\r\nhOsT: case-matters-not.com\r\n\r\n";
        assert_eq!(result, expected);
    }

    /// Ensures that extra whitespace around the `Host` header value is trimmed.
    #[tokio::test]
    async fn test_host_header_trims_whitespace() {
        let request = b"GET / HTTP/1.1\r\nHost:  spaced-out.net  \r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let result = rewrite_request_to_absolute_form(&mut reader, None).await.unwrap();
        let expected = "GET http://spaced-out.net/ HTTP/1.1\r\nHost:  spaced-out.net  \r\n\r\n";
        assert_eq!(result, expected);
    }

    /// Tests that an error is returned if the 'Host' header is completely missing.
    #[tokio::test]
    async fn test_pass_through_on_missing_host_header() {
        let request = b"GET /path HTTP/1.1\r\nUser-Agent: No-Host-Client\r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let result = rewrite_request_to_absolute_form(&mut reader, None).await.unwrap();
        assert_eq!(result, "GET /path HTTP/1.1\r\nUser-Agent: No-Host-Client\r\n\r\n");
    }

    /// Tests that an error is returned if the 'Host' header is present but has an empty value.
    #[tokio::test]
    async fn test_pass_through_on_empty_host_header_value() {
        let request = b"GET /path HTTP/1.1\r\nHost: \r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let result = rewrite_request_to_absolute_form(&mut reader, None).await.unwrap();
        assert_eq!(result, "GET /path HTTP/1.1\r\nHost: \r\n\r\n");
    }

    /// Tests that an error is returned for a malformed request-line.
    #[tokio::test]
    async fn test_error_on_malformed_request_line() {
        // This request line only has two parts, which is invalid.
        let request = b"GET /path\r\nHost: example.com\r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let result = rewrite_request_to_absolute_form(&mut reader, None).await;
        let expected_error = Error::MalformedRequestLine("GET /path".to_string());
        assert_eq!(result, Err(expected_error));
    }

    /// Test 1: The "happy path" success case.
    #[tokio::test]
    async fn test_proxy_with_header_rewrite() {
        // 1. Setup server that reads a request and writes a response.
        let (tx, rx) = oneshot::channel::<SocketAddr>();
        let server_task = tokio::spawn(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            let (mut socket, _) = listener.accept().await.unwrap();

            let received_request = {
                let mut reader = BufReader::new(&mut socket);
                let mut headers = String::new();
                let mut content_length = 0;

                // Read headers line by line
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap() == 0 {
                        break; // Connection closed
                    }
                    let lower_line = line.to_lowercase();
                    if lower_line.starts_with("content-length:") {
                        if let Some(val) = lower_line.split(':').nth(1) {
                            content_length = val.trim().parse().unwrap_or(0);
                        }
                    }
                    headers.push_str(&line);
                    if line == "\r\n" {
                        break; // End of headers
                    }
                }

                let mut body = String::new();
                if content_length > 0 {
                    let mut body_buffer = vec![0; content_length];
                    reader.read_exact(&mut body_buffer).await.unwrap();
                    body = String::from_utf8_lossy(&body_buffer).to_string();
                }
                format!("{}{}", headers, body)
            };

            socket.write_all(b"pong").await.unwrap();
            received_request
        });

        // 2. Test Logic
        let server_addr = rx.await.expect("Server failed to start");
        let mut proxy_stream = connect_with_header_rewrite(server_addr, None).await.unwrap();
        let request =
            "POST /resource HTTP/1.1\r\nHost: my-server.com\r\nContent-Length: 4\r\n\r\nbody";
        proxy_stream.write_all(request.as_bytes()).await.unwrap();

        // 3. Read the echoed response.
        let mut response_body = [0u8; 4];
        proxy_stream.read_exact(&mut response_body).await.unwrap();

        // 4. Verify the result.
        let received_data = server_task.await.unwrap();
        let expected_data = "POST http://my-server.com/resource HTTP/1.1\r\nHost: my-server.com\r\nContent-Length: 4\r\n\r\nbody";
        assert_eq!(received_data, expected_data);
        assert_eq!(String::from_utf8_lossy(&response_body), "pong");
    }

    /// Test 2: Destination server is unreachable.
    #[tokio::test]
    async fn test_unreachable_destination() {
        let unreachable_addr: SocketAddr = "127.0.0.1:59999".parse().unwrap();
        let result = connect_with_header_rewrite(unreachable_addr, None).await;
        assert!(result.is_err(), "Function should fail when destination is unreachable");
        if let Err(Error::IoError(e)) = result {
            assert_eq!(e.kind(), std::io::ErrorKind::ConnectionRefused);
        } else {
            panic!("Expected IoError with ConnectionRefused, but got {:?}", result);
        }
    }

    /// Test 3: Client connects but sends no data, resulting in an empty request.
    #[tokio::test]
    async fn test_client_sends_no_data() {
        // 1. Setup server and synchronization channel
        let (tx, rx) = oneshot::channel::<SocketAddr>();
        let server_task = tokio::spawn(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            socket.read_to_end(&mut buffer).await.unwrap();
            buffer
        });

        // 2. Test Logic: Connect and immediately drop the stream.
        let server_addr = rx.await.expect("Server failed to start");
        let proxy_stream = connect_with_header_rewrite(server_addr, None).await.unwrap();
        drop(proxy_stream);

        // 3. Verify the result: The server should receive no data because the client sent none.
        let received_data = server_task.await.unwrap();
        assert!(received_data.is_empty());
    }

    /// Test 4: A large message body is proxied correctly.
    #[tokio::test]
    async fn test_large_body_proxy() {
        // 1. Setup server and synchronization channel
        let (tx, rx) = oneshot::channel::<SocketAddr>();
        let server_task = tokio::spawn(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            socket.read_to_end(&mut buffer).await.unwrap();
            buffer
        });

        // 2. Test Logic
        let server_addr = rx.await.expect("Server failed to start");
        let mut proxy_stream = connect_with_header_rewrite(server_addr, None).await.unwrap();
        let large_body = "a".repeat(10 * 1024); // 10 KB body
        let initial_data = format!(
            "POST /upload HTTP/1.1\r\nHost: large-data.com\r\nContent-Length: {}\r\n\r\n{}",
            large_body.len(),
            large_body
        );
        proxy_stream.write_all(initial_data.as_bytes()).await.unwrap();
        drop(proxy_stream);

        // 3. Verify the result
        let received_data = server_task.await.unwrap();
        let expected_data = format!(
            "POST http://large-data.com/upload HTTP/1.1\r\nHost: large-data.com\r\nContent-Length: {}\r\n\r\n{}",
            large_body.len(),
            large_body
        );
        assert_eq!(String::from_utf8(received_data).unwrap(), expected_data);
    }

    #[tokio::test]
    async fn test_proxy_with_auth() {
        let request =
            b"GET /path/to/page.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\n\r\n";
        let mut reader = BufReader::new(&request[..]);
        let connector = Connector::new(
            "127.0.0.1:8080".parse().unwrap(),
            Some("user".into()),
            Some("password".into()),
        );
        let result =
            rewrite_request_to_absolute_form(&mut reader, connector.auth_header()).await.unwrap();
        let expected = "GET http://example.com/path/to/page.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: test\r\nProxy-Authorization: Basic dXNlcjpwYXNzd29yZA==\r\n\r\n";
        assert_eq!(result, expected);
    }
}
