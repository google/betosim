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

//! This module defines the Proxy error types.

use std::fmt;
use std::io;
use std::net::SocketAddr;

/// Represents all possible errors that can occur in the http-proxy crate.
#[derive(Debug)]
pub enum Error {
    /// An I/O error occurred.
    IoError(io::Error),
    /// The HTTP request-line is malformed.
    MalformedRequestLine(String),
    /// The 'Host' header is missing.
    MissingHostHeader,
    /// The proxy server returned an error.
    ConnectionError(SocketAddr, String),
    /// The proxy configuration string is malformed.
    MalformedConfigString,
    /// The port number in the proxy configuration is invalid.
    InvalidPortNumber,
    /// The host in the proxy configuration is invalid.
    InvalidHost,
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::IoError(a), Self::IoError(b)) => a.kind() == b.kind(),
            (Self::MalformedRequestLine(a), Self::MalformedRequestLine(b)) => a == b,
            (Self::MissingHostHeader, Self::MissingHostHeader) => true,
            (Self::ConnectionError(a, b), Self::ConnectionError(c, d)) => a == c && b == d,
            (Self::MalformedConfigString, Self::MalformedConfigString) => true,
            (Self::InvalidPortNumber, Self::InvalidPortNumber) => true,
            (Self::InvalidHost, Self::InvalidHost) => true,
            _ => false,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::IoError(err) => write!(f, "I/O error: {}", err),
            Error::MalformedRequestLine(line) => {
                write!(f, "Malformed request line: '{}'", line)
            }
            Error::MissingHostHeader => write!(f, "Mandatory 'Host' header is missing"),
            Error::ConnectionError(addr, msg) => {
                write!(f, "Connection to {} failed: {}", addr, msg)
            }
            Error::MalformedConfigString => write!(f, "Malformed configuration string"),
            Error::InvalidPortNumber => write!(f, "Invalid port number"),
            Error::InvalidHost => write!(f, "Invalid host"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::IoError(err)
    }
}

/// A type alias for `Result` where the error type is this crate's `Error`.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_error_chaining() {
        let inner_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let outer_error = Error::IoError(inner_error);

        assert!(outer_error.to_string().contains("file not found"));
    }
}
