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
// including protocol, address, username, and password. The
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

use crate::{Error, Result};
use regex::Regex;
use std::net::{SocketAddr, ToSocketAddrs};
#[cfg(unix)]
use std::os::fd::IntoRawFd;
#[cfg(windows)]
use std::os::windows::io::IntoRawSocket;
use tokio::net::TcpStream;

pub type RawDescriptor = i32;

/// Proxy configuration
pub struct ProxyConfig {
    pub protocol: String,
    pub addr: SocketAddr,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl ProxyConfig {
    /// Parses a proxy configuration string and returns a `ProxyConfig` struct.
    ///
    /// The function expects the proxy configuration string to be in the following format:
    ///
    /// [protocol://][username:password@]host:port
    /// [protocol://][username:password@]/[host/]:port
    ///
    /// where:
    ///
    /// * `protocol`: The network protocol (e.g., `http`, `https`,
    /// `socks5`). If not provided, defaults to `http`.
    /// * `username`: and `password` are optional credentials for authentication.
    /// * `host`: The hostname or IP address of the proxy server. If
    /// it's an IPv6 address, it should be enclosed in square brackets
    /// (e.g., "[::1]").
    /// * `port`: The port number on which the proxy server is listening.
    ///
    /// # Errors
    /// Returns a `Error` if the input string is not in a
    /// valid format or if the hostname/port resolution fails.
    ///
    /// # Limitations
    /// * Usernames and passwords cannot contain `@` or `:`.
    pub fn from_string(config_string: &str) -> Result<ProxyConfig> {
        let re = Regex::new(r"^(?:(?P<protocol>\w+)://)?(?:(?P<user>\w+):(?P<pass>\w+)@)?(?P<host>(?:[\w\.-]+|\[[^\]]+\])):(?P<port>\d+)$").unwrap();
        let caps = re.captures(config_string).ok_or(Error::MalformedConfigString)?;

        let protocol =
            caps.name("protocol").map_or_else(|| "http".to_string(), |m| m.as_str().to_string());
        let username = caps.name("user").map(|m| m.as_str().to_string());
        let password = caps.name("pass").map(|m| m.as_str().to_string());

        // Extract host, removing surrounding brackets if present
        let hostname = caps
            .name("host")
            .ok_or(Error::MalformedConfigString)?
            .as_str()
            .trim_matches(|c| c == '[' || c == ']')
            .to_string();

        let port = caps
            .name("port")
            .ok_or(Error::MalformedConfigString)?
            .as_str()
            .parse::<u16>()
            .map_err(|_| Error::InvalidPortNumber)?;

        let host = (hostname, port)
            .to_socket_addrs()
            .map_err(|_| Error::InvalidHost)?
            .next() // Take the first resolved address
            .ok_or(Error::InvalidHost)?
            .ip();

        Ok(ProxyConfig { protocol, username, password, addr: SocketAddr::from((host, port)) })
    }
}

/// Convert TcpStream to RawDescriptor (i32)
pub fn into_raw_descriptor(stream: TcpStream) -> RawDescriptor {
    let std_stream = stream.into_std().expect("into_raw_descriptor's into_std() failed");

    std_stream.set_nonblocking(false).expect("non-blocking");

    // Use into_raw_fd for Unix to pass raw file descriptor to C
    #[cfg(unix)]
    return std_stream.into_raw_fd();

    // Use into_raw_socket for Windows to pass raw socket to C
    #[cfg(windows)]
    std_stream.into_raw_socket().try_into().expect("Failed to convert Raw Socket value into i32")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn parse_configuration_string_success() {
        // Test data
        let data = [
            (
                "127.0.0.1:8080",
                ProxyConfig {
                    protocol: "http".to_owned(),
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)),
                    username: None,
                    password: None,
                },
            ),
            (
                "http://127.0.0.1:8080",
                ProxyConfig {
                    protocol: "http".to_owned(),
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)),
                    username: None,
                    password: None,
                },
            ),
            (
                "https://127.0.0.1:8080",
                ProxyConfig {
                    protocol: "https".to_owned(),
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)),
                    username: None,
                    password: None,
                },
            ),
            (
                "sock5://127.0.0.1:8080",
                ProxyConfig {
                    protocol: "sock5".to_owned(),
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)),
                    username: None,
                    password: None,
                },
            ),
            (
                "user:pass@192.168.0.18:3128",
                ProxyConfig {
                    protocol: "http".to_owned(),
                    addr: SocketAddr::from((IpAddr::V4(Ipv4Addr::new(192, 168, 0, 18)), 3128)),
                    username: Some("user".to_string()),
                    password: Some("pass".to_string()),
                },
            ),
            (
                "https://[::1]:7000",
                ProxyConfig {
                    protocol: "https".to_owned(),
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
                    protocol: "http".to_owned(),
                    addr: SocketAddr::from((
                        IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                        7000,
                    )),
                    username: None,
                    password: None,
                },
            ),
        ];

        // TODO: Mock DNS server to to test hostname. e.g. "proxy.example.com:3000".
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
        let data = [
            ("http://", Error::MalformedConfigString),
            ("", Error::MalformedConfigString),
            ("256.0.0.1:8080", Error::InvalidHost),
            ("127.0.0.1:foo", Error::MalformedConfigString),
            ("127.0.0.1:-2", Error::MalformedConfigString),
            ("127.0.0.1:100000", Error::InvalidPortNumber),
            ("127.0.0.1", Error::MalformedConfigString),
            ("http:127.0.0.1:8080", Error::MalformedConfigString),
            ("::1:8080", Error::MalformedConfigString),
            ("user@pass:127.0.0.1:8080", Error::MalformedConfigString),
            ("user@127.0.0.1:8080", Error::MalformedConfigString),
            ("proxy.example.com:7000", Error::InvalidHost),
            ("[::1}:7000", Error::MalformedConfigString),
        ];

        for (input, expected_error) in data {
            let result = ProxyConfig::from_string(input);
            assert_eq!(
                result.err().unwrap().to_string(),
                expected_error.to_string(),
                "Expected an error for input: {}",
                input
            );
        }
    }
}
