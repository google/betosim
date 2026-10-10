// Copyright (C) 2025 The Android Open Source Project
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

use std::net::AddrParseError;

/// Main error type for the PacketStream framework
#[derive(thiserror::Error, Debug)]
pub enum PacketStreamError {
    /// Socket-related errors
    #[error("Socket error: {0}")]
    Socket(#[from] SocketError),
    /// Protocol-related errors
    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("Invalid configuration: {0}")]
    /// Configuration-related errors
    InvalidConfig(String),
    /// IO errors from underlying streams
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Connection was closed unexpectedly
    #[error("Connection closed")]
    ConnectionClosed,
    /// Operation timed out
    #[error("Operation timed out")]
    Timeout,
}

#[derive(thiserror::Error, Debug)]
pub enum SocketError {
    /// Socket type not supported on this platform
    #[error("`{0}` sockets not supported on this platform")]
    UnsupportedPlatform(String),
    /// Invalid socket address format
    #[error("Invalid socket address: {0}")]
    InvalidAddress(#[from] AddrParseError),
    /// Socket accept failed
    #[error("Accept failed: {0}")]
    AcceptFailed(String),
}

#[derive(thiserror::Error, Debug)]
pub enum ProtocolError {
    /// Invalid message length (too large or zero)
    #[error("Invalid message length: {0}")]
    InvalidLength(u32),
    /// Message size exceeds maximum allowed
    #[error("Message too large: {0} bytes")]
    MessageTooLarge(usize),
    /// Incomplete message received
    #[error("Incomplete message received")]
    IncompleteMessage,
    /// Invalid message format
    #[error("Invalid message format: {0}")]
    InvalidFormat(String),
    /// Unknown VMM identifier
    #[error("Unknown VMM identifier: {0}")]
    UnknownVmm(String),
}

impl From<std::num::ParseIntError> for PacketStreamError {
    fn from(err: std::num::ParseIntError) -> Self {
        PacketStreamError::InvalidConfig(err.to_string())
    }
}

impl From<tokio::time::error::Elapsed> for PacketStreamError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        PacketStreamError::Timeout
    }
}

impl From<tokio::task::JoinError> for PacketStreamError {
    fn from(err: tokio::task::JoinError) -> Self {
        PacketStreamError::Io(std::io::Error::other(err.to_string()))
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for PacketStreamError {
    fn from(err: tokio::sync::mpsc::error::SendError<T>) -> Self {
        PacketStreamError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, err.to_string()))
    }
}

// Note: PacketStreamError implements std::error::Error for compatibility with
// error handling libraries

/// Result type alias for PacketStream operations
pub type Result<T> = std::result::Result<T, PacketStreamError>;
