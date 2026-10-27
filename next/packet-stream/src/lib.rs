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
//=============================================================================
// src/lib.rs - Public API exports
//=============================================================================

//! # PacketStream
//!
//! High-performance, cross-platform packet streaming framework for virtualized
//! environments. Transports network packets (802.11, 802.3, UWB UCI, Bluetooth
//! HCI) between guest and host systems with automatic device information
//! exchange.
//!
//! ## Features
//!
//! - **Zero-copy operations** - `Bytes`-based reference counting with
//!   allocation tracking
//! - **Cross-platform** - Unix Domain Sockets, Named Pipes, VSOCK, TCP with
//!   auto-detection
//! - **Transport-agnostic** - Unified `Streams` API across all transport types
//! - **Performance** - >1.3 Gbps throughput, <12μs latency on modern hardware
//!
//! ## Quick Start
//!
//!
//! ## Wire Format
//!
//! Simple length-prefixed binary protocol: `[4-byte length][payload]`.
//! Uses native byte order for local IPC efficiency.
//!
//! ## Performance
//!
//! Benchmarked on Apple M2 Mac mini:
//!
//! | Transport | Bandwidth | Latency | Use Case |
//! |-----------|-----------|---------|----------|
//! | **UDS** | 1.3+ Gbps | 11.7μs | Local IPC |
//! | **TCP** | 800+ Mbps | 15-20μs | Network fallback |
//!
//! Supports 802.11, 802.3, Bluetooth HCI, and UWB UCI packet sizes efficiently.
//!
//! ## Platform Support
//!
//! - **Unix/Linux/macOS**: Unix Domain Sockets (best performance)
//! - **Windows**: Named Pipes with TCP fallback
//! - **VMs**: VSOCK for guest-host communication
//! - **Containers**: Volume mounts or port forwarding

// Benchmark module moved to packetstream-bins crate
//!
//! Netsim Packet Stream
//!
//! This crate provides the core transport and streaming abstractions for
//! Netsim. It handles the connection handshake, packet framing, and
//! multiplexing of packet streams over various transports (TCP, UDS, gRPC,
//! etc.).
//!
//! The main entry points are:
//! - `CrossPlatformListener`: For listening for incoming connections.
//! - `CrossPlatformStream`: For connecting to a remote endpoint.
//! - `PacketStream` / `PacketSink`: Traits for reading and writing packets.

pub mod error; // Error types
mod streams; // High-level connection management
pub mod transport; // Transport implementations (includes socket)
mod types; // Core data types

// Core public API - only expose what users actually need
pub use error::PacketStreamError;
// Streams architecture re-exports - Streams is the only public entry point
pub use streams::{InitInfo, Streams};
// Core streaming types - public API only (transport internals hidden)
pub use transport::TransportType;
#[cfg(all(target_os = "linux", feature = "cuttlefish"))]
pub use transport::dual_fd::{DualFdConfig, DualFdListener};
pub use transport::{
    CrossPlatformListener, CrossPlatformStream, ListenerConfig, SocketConfig, SocketType,
};
pub use types::{BtControllerProperties, Chip, ChipInfo, ChipKind, DeviceInfo, StreamAddress};
