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

// # Http Proxy Utils
//
// This module provides functionality for parsing proxy configuration
// strings and converting `TcpStream` objects to raw file
// descriptors.
//
// The `ProxyConfig` struct holds the parsed proxy configuration,
// including address, username, and password. The
// `from_string` function parses a proxy configuration string in the
// format `[protocol://][username:password@]host:port` or
// `[protocol://][username:password@]/[host/]:port` and returns a
// `ProxyConfig` struct.
//
// The `into_raw_descriptor` function converts a `TcpStream` object
// to a raw file descriptor (`RawDescriptor`), which is an `i32`
// representing the underlying socket. This is used for compatibility
// with libraries that require raw file descriptors, such as
// `libslirp_rs`.

use std::net::SocketAddr;
#[cfg(unix)]
use std::os::fd::IntoRawFd;
#[cfg(windows)]
use std::os::windows::io::IntoRawSocket;

use percent_encoding::percent_decode_str;
use tokio::net::TcpStream;
use url::Url;

use crate::{Connector, Error, Result};

type RawDescriptor = i32;

/// Proxy configuration
pub(crate) struct ProxyConfig {
    addr: SocketAddr,
    username: Option<String>,
    password: Option<String>,
}

impl ProxyConfig {
    /// Converts this configuration into a `Connector`.
    pub(crate) fn into_connector(self) -> Connector {
        Connector::new(self.addr, self.username, self.password)
    }
    /// Parses a proxy configuration string and returns a `ProxyConfig` struct.
    ///
    /// The function expects the proxy configuration string to be in the
    /// following format:
    ///
    /// ```text
    /// [protocol://][username:password@]host:port
    /// [protocol://][username:password@]/[host/]:port
    /// ```
    ///
    /// where:
    ///
    /// * `protocol`: The network protocol (e.g., `http`, `https`). If not
    ///   provided, defaults to `http`.
    /// * `username` and `password` are optional credentials for authentication.
    /// * `host`: The hostname or IP address of the proxy server. If it's an
    ///   IPv6 address, it should be enclosed in square brackets (e.g.,
    ///   `"[::1]"`).
    /// * `port`: The port number on which the proxy server is listening.
    ///
    /// # Errors
    /// Returns an [Error] if the input string is not in a
    /// valid format or if the hostname/port resolution fails.
    pub(crate) fn from_string(config_string: &str) -> Result<ProxyConfig> {
        let normalized = if !config_string.contains("://") {
            format!("http://{config_string}")
        } else {
            config_string.to_string()
        };

        let parsed = Url::parse(&normalized).map_err(|err| match err {
            url::ParseError::InvalidIpv4Address
            | url::ParseError::InvalidIpv6Address
            | url::ParseError::IdnaError
            | url::ParseError::InvalidDomainCharacter => Error::InvalidHost(Box::new(err)),
            url::ParseError::InvalidPort => Error::InvalidPortNumber(Box::new(err)),
            _ => Error::MalformedConfigString,
        })?;

        let port = parsed.port_or_known_default().ok_or_else(|| {
            Error::InvalidPortNumber(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "No port number in the URL",
            )))
        })?;

        let addr = parsed
            .socket_addrs(|| Some(port))
            .map_err(|err| Error::InvalidHost(Box::new(err)))?
            .into_iter()
            .next()
            .ok_or_else(|| {
                Error::InvalidHost(Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "No address found",
                )))
            })?;

        let username = Some(parsed.username())
            .filter(|s| !s.is_empty())
            .map(|s| percent_decode_str(s).decode_utf8_lossy().into_owned());
        let password = parsed
            .password()
            .filter(|s| !s.is_empty())
            .map(|s| percent_decode_str(s).decode_utf8_lossy().into_owned());

        Ok(ProxyConfig { addr, username, password })
    }
}

/// Convert TcpStream to RawDescriptor (i32)
pub(crate) fn into_raw_descriptor(stream: TcpStream) -> Result<RawDescriptor> {
    let std_stream = stream.into_std()?;
    std_stream.set_nonblocking(false)?;

    // Use into_raw_fd for Unix to pass raw file descriptor to C
    #[cfg(unix)]
    return Ok(std_stream.into_raw_fd());

    // Use into_raw_socket for Windows to pass raw socket to C
    #[cfg(windows)]
    Ok(std_stream.into_raw_socket().try_into().map_err(std::io::Error::other)?)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn parse_configuration_string_success() {
        // Test data
        let data = [
            (
                "127.0.0.1:8080",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)),
                    username: None,
                    password: None,
                },
            ),
            (
                "http://127.0.0.1:8080",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)),
                    username: None,
                    password: None,
                },
            ),
            (
                "https://127.0.0.1:8080",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)),
                    username: None,
                    password: None,
                },
            ),
            (
                "user:pass@192.168.0.18:3128",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(192, 168, 0, 18)), 3128)),
                    username: Some("user".to_string()),
                    password: Some("pass".to_string()),
                },
            ),
            (
                "https://[::1]:7000",
                ProxyConfig {
                    addr: SocketAddr::from((
                        IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                        7000,
                    )),
                    username: None,
                    password: None,
                },
            ),
            (
                "[::1]:7000",
                ProxyConfig {
                    addr: SocketAddr::from((
                        IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                        7000,
                    )),
                    username: None,
                    password: None,
                },
            ),
            (
                ":@127.0.0.1:8080",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)),
                    username: None,
                    password: None,
                },
            ),
            (
                "http://:@127.0.0.1:8080",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)),
                    username: None,
                    password: None,
                },
            ),
            (
                "user:p%40ss@192.168.0.18:3128",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(192, 168, 0, 18)), 3128)),
                    username: Some("user".to_string()),
                    password: Some("p@ss".to_string()),
                },
            ),
            (
                "user:pass%2fword@192.168.0.18:3128",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(192, 168, 0, 18)), 3128)),
                    username: Some("user".to_string()),
                    password: Some("pass/word".to_string()),
                },
            ),
            (
                "user%20name:pass@192.168.0.18:3128",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(192, 168, 0, 18)), 3128)),
                    username: Some("user name".to_string()),
                    password: Some("pass".to_string()),
                },
            ),
            (
                "user@127.0.0.1:8080",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)),
                    username: Some("user".to_string()),
                    password: None,
                },
            ),
            (
                "user:@127.0.0.1:8080",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)),
                    username: Some("user".to_string()),
                    password: None,
                },
            ),
            (
                "127.0.0.1",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80)),
                    username: None,
                    password: None,
                },
            ),
            (
                "https://127.0.0.1",
                ProxyConfig {
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 443)),
                    username: None,
                    password: None,
                },
            ),
        ];

        // TODO: Mock DNS server to to test hostname. e.g.
        // "proxy.example.com:3000".
        for (input, expected) in data {
            let result = ProxyConfig::from_string(input);
            assert!(
                result.is_ok(),
                "Unexpected error {} for input: {}",
                result.err().unwrap(),
                input
            );
            let result = result.ok().unwrap();
            assert_eq!(result.addr, expected.addr, "For input: {}", input);
            assert_eq!(result.username, expected.username, "For input: {}", input);
            assert_eq!(result.password, expected.password, "For input: {}", input);
        }
    }

    #[test]
    fn parse_configuration_string_with_errors() {
        let dummy_error = || Box::new(std::io::Error::other("dummy"));
        let data = [
            ("http://", Error::MalformedConfigString),
            ("", Error::MalformedConfigString),
            ("256.0.0.1:8080", Error::InvalidHost(dummy_error())),
            ("127.0.0.1:foo", Error::InvalidPortNumber(dummy_error())),
            ("127.0.0.1:-2", Error::InvalidPortNumber(dummy_error())),
            ("127.0.0.1:100000", Error::InvalidPortNumber(dummy_error())),
            ("http:127.0.0.1:8080", Error::InvalidPortNumber(dummy_error())),
            ("::1:8080", Error::MalformedConfigString),
            ("user@pass:127.0.0.1:8080", Error::InvalidPortNumber(dummy_error())),
            ("proxy.example.com:7000", Error::InvalidHost(dummy_error())),
            (":@proxy.example.com:7000", Error::InvalidHost(dummy_error())),
            ("foo..bar:8080", Error::InvalidHost(dummy_error())),
            ("[::1}:7000", Error::InvalidHost(dummy_error())),
        ];

        for (input, expected_error) in data {
            let result = ProxyConfig::from_string(input);
            let actual_error = result.err().unwrap();
            match expected_error {
                Error::InvalidHost(_) => {
                    assert!(
                        matches!(actual_error, Error::InvalidHost(_)),
                        "Expected InvalidHost for input: {}, got {:?}",
                        input,
                        actual_error
                    );
                }
                Error::InvalidPortNumber(_) => {
                    assert!(
                        matches!(actual_error, Error::InvalidPortNumber(_)),
                        "Expected InvalidPortNumber for input: {}, got {:?}",
                        input,
                        actual_error
                    );
                }
                _ => {
                    assert_eq!(
                        actual_error.to_string(),
                        expected_error.to_string(),
                        "Expected {} for input: {}",
                        expected_error,
                        input
                    );
                }
            }
        }
    }
}
