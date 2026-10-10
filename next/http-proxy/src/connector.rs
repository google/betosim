// Copyright 2024 The Android Open Source Project
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

use std::net::SocketAddr;

use base64::{Engine as _, engine::general_purpose};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

use crate::{Error, Result, rewriter};

const HTTP_VERSION: &str = "1.1";

/// Establishes a TCP connection to a target address through an HTTP proxy.
///
/// The `Connector` handles the CONNECT request handshake with the proxy,
/// including optional Basic authentication.
#[derive(Clone)]
pub struct Connector {
    proxy_addr: SocketAddr,
    username: Option<String>,
    password: Option<String>,
}

impl Connector {
    /// Creates a new `Connector` with proxy address and optional authentication
    /// details
    pub fn new(proxy_addr: SocketAddr, username: Option<String>, password: Option<String>) -> Self {
        Connector { proxy_addr, username, password }
    }

    /// Returns the proxy address.
    pub fn proxy_addr(&self) -> SocketAddr {
        self.proxy_addr
    }

    /// Returns the proxy authorization header if username is provided.
    ///
    /// If no password is provided, it defaults to an empty password per RFC
    /// 7617.
    pub(crate) fn auth_header(&self) -> Option<String> {
        if let Some(username) = &self.username {
            let password = self.password.as_deref().unwrap_or("");
            let encoded_auth = base64_encode(format!("{}:{}", username, password).as_bytes());
            let auth_header = format!(
                "Proxy-Authorization: Basic {}\r\n",
                String::from_utf8_lossy(&encoded_auth)
            );
            Some(auth_header)
        } else {
            None
        }
    }

    /// Establishes a TCP connection to the given address.
    ///
    /// If the address is on port 80, it will be rewritten to an absolute-form
    /// request. Otherwise, it will connect directly.
    pub async fn connect(&self, addr: SocketAddr) -> Result<TcpStream> {
        let proxy_addr = self.proxy_addr();
        let auth_header = self.auth_header();
        if addr.port() == 80 {
            rewriter::connect_with_header_rewrite(proxy_addr, auth_header).await
        } else {
            self.connect_with_tunnel(addr).await
        }
    }

    /// Establishes a TCP connection to the given address through the proxy.
    async fn connect_with_tunnel(&self, addr: SocketAddr) -> Result<TcpStream> {
        let mut stream = TcpStream::connect(self.proxy_addr).await?;

        // Construct the CONNECT request
        let mut request = format!("CONNECT {} HTTP/{}\r\n", addr, HTTP_VERSION);

        // Authentication
        if let Some(auth_header) = self.auth_header() {
            // Add the header to the request
            request.push_str(&auth_header);
        }

        // Add the final CRLF
        request.push_str("\r\n");
        stream.write_all(request.as_bytes()).await?;

        // Read the proxy's response
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).await?;
        if response.starts_with(&format!("HTTP/{} 200", HTTP_VERSION)) {
            Ok(reader.into_inner())
        } else {
            Err(Error::ConnectionError(addr, response.trim_end_matches("\r\n").to_string()))
        }
    }
}

fn base64_encode(src: &[u8]) -> Vec<u8> {
    general_purpose::STANDARD.encode(src).into_bytes()
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::AsyncReadExt,
        net::{TcpListener, lookup_host},
    };

    use super::*;

    #[tokio::test]
    async fn test_connect() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = listener.local_addr().unwrap();

        let addr: SocketAddr = lookup_host("localhost:8000").await.unwrap().next().unwrap();

        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            // Server expects a client greeting with no auth methods
            let expected_greeting = format!("CONNECT {} HTTP/1.1\r\n", &addr);

            let mut reader = BufReader::new(stream);
            let mut line = String::new();

            reader.read_line(&mut line).await.unwrap();

            assert_eq!(line, expected_greeting);

            // Server sends a response with no auth method selected
            let response = "HTTP/1.1 200 Connection established\r\n\r\n";
            let mut stream = reader.into_inner();
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = Connector::new(proxy_addr, None, None);

        client.connect(addr).await.unwrap();

        handle.await.unwrap(); // Wait for the task to complete

        Ok(())
    }

    #[tokio::test]
    async fn test_connect_with_auth() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = listener.local_addr().unwrap();

        let addr: SocketAddr = lookup_host("localhost:8000").await.unwrap().next().unwrap();

        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            // Server expects a client greeting with auth header
            let expected_greeting = format!(
                "CONNECT {} HTTP/1.1\r\nProxy-Authorization: Basic dXNlcjpwYXNzd29yZA==\r\n\r\n",
                &addr
            );

            let mut buf = [0; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            let actual_greeting = String::from_utf8_lossy(&buf[..n]);

            assert_eq!(actual_greeting, expected_greeting);

            // Server sends a response
            let response = "HTTP/1.1 200 Connection established\r\n\r\n";

            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = Connector::new(proxy_addr, Some("user".into()), Some("password".into()));

        client.connect(addr).await.unwrap();

        handle.await.unwrap(); // Wait for the task to complete

        Ok(())
    }

    #[test]
    fn test_proxy_base64_encode_success() {
        let input = b"hello world";
        let encoded = base64_encode(input);
        assert_eq!(encoded, b"aGVsbG8gd29ybGQ=");
    }

    #[test]
    fn test_proxy_base64_encode_empty_input() {
        let input = b"";
        let encoded = base64_encode(input);
        assert_eq!(encoded, b"");
    }

    #[tokio::test]
    async fn test_connect_with_port_80() {
        let (tx, rx) = tokio::sync::oneshot::channel();

        // Mock server that receives the rewritten request
        let server_task = tokio::spawn(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 1024];
            let n = stream.read(&mut buffer).await.unwrap();
            String::from_utf8_lossy(&buffer[..n]).to_string()
        });

        let proxy_addr = rx.await.unwrap();
        let connector = Connector::new(proxy_addr, None, None);

        // This is the address we want to connect to, which is on port 80.
        let target_addr: SocketAddr = "127.0.0.1:80".parse().unwrap();

        // The client stream that our test will write to.
        let mut client_stream = connector.connect(target_addr).await.unwrap();

        // The client writes a simple HTTP request to the stream.
        let request = "GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        client_stream.write_all(request.as_bytes()).await.unwrap();

        // The mock server should receive the rewritten request.
        let received_request = server_task.await.unwrap();
        let expected_request = "GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n";
        assert_eq!(received_request, expected_request);
    }

    #[test]
    fn test_proxy_addr() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let connector = Connector::new(addr, None, None);
        assert_eq!(connector.proxy_addr(), addr);
    }

    #[test]
    fn test_auth_header_with_empty_password() {
        let connector =
            Connector::new("127.0.0.1:8080".parse().unwrap(), Some("user".into()), Some("".into()));
        assert_eq!(
            connector.auth_header(),
            Some("Proxy-Authorization: Basic dXNlcjo=\r\n".to_string())
        );
    }

    #[tokio::test]
    async fn test_connect_with_percent_encoded_auth() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        let addr: SocketAddr = lookup_host("localhost:8000").await.unwrap().next().unwrap();

        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let expected_greeting = format!(
                "CONNECT {} HTTP/1.1\r\nProxy-Authorization: Basic dXNlcjpwYXNzL3dvcmQ=\r\n\r\n",
                &addr
            );
            let mut buf = [0; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            let actual_greeting = String::from_utf8_lossy(&buf[..n]);
            assert_eq!(actual_greeting, expected_greeting);
            let response = "HTTP/1.1 200 Connection established\r\n\r\n";
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = Connector::new(proxy_addr, Some("user".into()), Some("pass/word".into()));
        client.connect(addr).await.unwrap();
        handle.await.unwrap();
        Ok(())
    }

    #[tokio::test]
    async fn test_connect_with_empty_password_auth() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        let addr: SocketAddr = lookup_host("localhost:8000").await.unwrap().next().unwrap();

        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let expected_greeting = format!(
                "CONNECT {} HTTP/1.1\r\nProxy-Authorization: Basic dXNlcjo=\r\n\r\n",
                &addr
            );
            let mut buf = [0; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            let actual_greeting = String::from_utf8_lossy(&buf[..n]);
            assert_eq!(actual_greeting, expected_greeting);
            let response = "HTTP/1.1 200 Connection established\r\n\r\n";
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = Connector::new(proxy_addr, Some("user".into()), Some("".into()));
        client.connect(addr).await.unwrap();
        handle.await.unwrap();
        Ok(())
    }

    #[tokio::test]
    async fn test_connect_rejected_with_407() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        let addr: SocketAddr = lookup_host("localhost:8000").await.unwrap().next().unwrap();

        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0; 1024];
            let _ = stream.read(&mut buf).await.unwrap();
            let response = "HTTP/1.1 407 Proxy Authentication Required\r\n\r\n";
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = Connector::new(proxy_addr, Some("user".into()), Some("wrongpass".into()));
        let res = client.connect(addr).await;
        assert!(res.is_err());
        handle.await.unwrap();
        Ok(())
    }

    #[test]
    fn test_auth_header_bare_username_defaults_to_empty_password() {
        let client = Connector::new("127.0.0.1:8080".parse().unwrap(), Some("user".into()), None);
        assert_eq!(
            client.auth_header(),
            Some("Proxy-Authorization: Basic dXNlcjo=\r\n".to_string())
        );
    }

    #[tokio::test]
    async fn test_connect_with_bare_username() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        let addr: SocketAddr = lookup_host("localhost:8000").await.unwrap().next().unwrap();

        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let expected_greeting = format!(
                "CONNECT {} HTTP/1.1\r\nProxy-Authorization: Basic dXNlcjo=\r\n\r\n",
                &addr
            );
            let mut buf = [0; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            let actual_greeting = String::from_utf8_lossy(&buf[..n]);
            assert_eq!(actual_greeting, expected_greeting);
            let response = "HTTP/1.1 200 Connection established\r\n\r\n";
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client = Connector::new(proxy_addr, Some("user".into()), None);
        client.connect(addr).await.unwrap();
        handle.await.unwrap();
        Ok(())
    }

    #[tokio::test]
    async fn test_connect_with_percent_decoded_auth_end_to_end() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        let addr: SocketAddr = lookup_host("localhost:8000").await.unwrap().next().unwrap();

        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let expected_greeting = format!(
                "CONNECT {} HTTP/1.1\r\nProxy-Authorization: Basic dXNlcjpwYXNzL3dvcmQ=\r\n\r\n",
                &addr
            );
            let mut buf = [0; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            let actual_greeting = String::from_utf8_lossy(&buf[..n]);
            assert_eq!(actual_greeting, expected_greeting);
            let response = "HTTP/1.1 200 Connection established\r\n\r\n";
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let client =
            crate::util::ProxyConfig::from_string(&format!("http://user:pass%2fword@{proxy_addr}"))
                .unwrap()
                .into_connector();
        client.connect(addr).await.unwrap();
        handle.await.unwrap();
        Ok(())
    }
}
