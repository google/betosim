# PacketStream

High-performance, cross-platform packet streaming library for virtualized environments.

## Overview

PacketStream provides a transport-agnostic API for streaming packets between systems, commonly used in virtualized or containerized environments. It simplifies communication by handling different transport types through a unified interface.

## Features

- **Transport Agnostic** - Unified `Streams` API across TCP, Unix Domain Sockets, and file descriptors.
- **Init_Info Protocol** - Automatic chip and device information exchange on connection.
- **Zero-Copy Operations** - `Bytes`-based for efficient data handling.
- **Cross-Platform** - Supports Unix Domain Sockets on Unix-like systems and Named Pipes on Windows.
- **High Performance** - Capable of >1.3 Gbps throughput and <12μs latency on modern hardware with UDS.

## Quick Start

Add this to your `Cargo.toml`:
```toml
[dependencies]
packet-stream = "0.1.0"
```

A simple echo server and client:
```rust,no_run
use packet_stream::{Streams, TransportType, ChipInfo, Chip, ChipKind, DeviceInfo};
use bytes::Bytes;

fn create_test_chip_info() -> ChipInfo {
    ChipInfo {
        name: "test-chip".to_string(),
        chip: Some(Chip {
            kind: ChipKind::Wifi,
            id: "test-chip".to_string(),
            name: "Test WiFi Chip".to_string(),
            manufacturer: "Test Corp".to_string(),
            product_name: "Test WiFi Chip".to_string(),
        }),
        device_info: Some(DeviceInfo {
            name: "test-device".to_string(),
            id: "test-device".to_string(),
        }),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Server: Start a listener
    let mut streams = Streams::new();
    streams.start_listener("test_socket", TransportType::uds("test.sock")).await?;

    // Client: Connect to the listener
    let chip_info = create_test_chip_info();
    let mut client_stream = streams.connect(TransportType::uds("test.sock"), chip_info).await?;

    // Server: Accept the connection
    let (_listener_name, mut server_stream) = streams.accept_any().await?;
    println!("Server accepted connection from: {}", server_stream.chip_info().device_name());

    // Client: Send a packet
    client_stream.send_packet(b"hello").await?;

    // Server: Receive and echo the packet
    let received = server_stream.recv_packet().await?;
    println!("Server received: {:?}", received.as_slice());
    server_stream.send_packet(received.as_slice()).await?;

    // Client: Receive the echo
    let echoed = client_stream.recv_packet().await?;
    println!("Client received echo: {:?}", echoed.as_slice());

    Ok(())
}
```

## Architecture

PacketStream provides a transport-agnostic API for streaming packets, primarily designed for virtualized or containerized environments. The architecture abstracts transport details behind a unified API.

### Core Components

#### Streams Manager

The `Streams` struct is the primary entry point, managing multiple transport listeners and enforcing the `init_info` protocol.

```rust
pub struct Streams {
    listener_tasks: HashMap<String, JoinHandle<()>>,
    // ...
}
```

**Key Responsibilities:**
- Manages multiple transport listeners (e.g., TCP, UDS) simultaneously.
- Enforces the `init_info` protocol for automatic `ChipInfo` exchange on new connections.
- Provides a unified `accept_any()` method to receive connections from any active listener.
- Provides a unified `connect()` method to establish outbound connections.

#### Stream Trait

The `Stream` trait is the public interface for a fully initialized packet stream.

```rust
#[async_trait]
pub trait Stream: Send + Sync {
    async fn send_packet(&mut self, data: &[u8]) -> Result<()>;
    async fn send_packet_bytes(&mut self, data: Bytes) -> Result<()>;
    async fn recv_packet(&mut self) -> Result<Message>;
    fn chip_info(&self) -> &ChipInfo;
    // ...
}
```

**Key Features:**
- **Guaranteed `chip_info`**: Every `Stream` instance has validated `ChipInfo` from the `init_info` handshake.
- **Zero-copy Operations**: `send_packet_bytes()` uses `Bytes` for efficient, reference-counted data handling.

### Connection Flow (Init_Info Protocol)

1.  **Client Connects**: A client initiates a connection using `Streams::connect()`.
2.  **`init_info` Sent**: The client serializes an `InitInfo` struct (containing its `ChipInfo`) and sends it as the first message.
3.  **Server Accepts**: The `TransportListener` on the server side accepts the raw connection.
4.  **`init_info` Received**: The listener reads the first message and deserializes it into an `InitInfo` struct.
5.  **Stream Creation**: The listener returns a tuple of the raw `Transport` and the received `ChipInfo`.
6.  **`accept_any()` Returns**: The `Streams` manager receives this tuple and wraps it in a `StreamImpl`, which is returned to the application as a `Box<dyn Stream>`.

This flow ensures that any `Stream` obtained by the application has guaranteed `ChipInfo` available.

### Memory Management

PacketStream uses `bytes::Bytes` for efficient, reference-counted data sharing, minimizing copies for packet payloads.

```rust
// Zero-copy sending (preferred)
stream.send_packet_bytes(bytes_data).await?;

// Convenience method with a copy
stream.send_packet(&slice_data).await?;
```

## Transport Types

PacketStream automatically selects the optimal transport for each platform:

| Transport | Platform | Use Case |
|-----------|----------|----------|
| **Unix Domain Sockets** | Unix/Linux/macOS | Host-container communication |
| **Named Pipes** | Windows | Cross-process communication |
| **VSOCK** | VM environments | Guest-host packet transport |
| **TCP** | All platforms | Network fallback, development |
| **DualFd** | Unix fork() | Process integration |

```rust,no_run
use packet_stream::{Streams, TransportType};
# async {
let mut streams = Streams::new();
// Specific transport configuration
streams.start_listener("tcp", TransportType::tcp("0.0.0.0:8080")).await.unwrap();
streams.start_listener("uds", TransportType::uds("/tmp/app.sock")).await.unwrap();

#[cfg(unix)]
{
    // DualFd is a special listener that takes configuration, not a path
    // let config = packet_stream::transport::DualFdConfig::from_json("{}").unwrap();
    // streams.start_listener("cuttlefish", TransportType::DualFd(config)).await.unwrap();
}
# };
```

### Transport Abstraction

The `Transport` and `TransportListener` traits define the abstraction layer that allows the `Streams` manager to be transport-agnostic.

```rust
#[async_trait]
pub trait TransportListener: Send + Sync {
    async fn accept(&mut self) -> Result<(Box<dyn Transport>, ChipInfo)>;
    fn local_addr(&self) -> Result<StreamAddress>;
    // ...
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send_packet(&mut self, data: &[u8]) -> Result<()>;
    async fn recv_packet(&mut self) -> Result<Message>;
    // ...
}
```
- `TransportListener` implementations (e.g., `TcpTransportListener`) are responsible for accepting raw connections and performing the `init_info` handshake to produce a `ChipInfo`.
- `Transport` implementations handle the low-level, length-prefixed packet framing.

## Performance

Benchmarked on Apple M2 Mac mini:

| Transport | Bandwidth | Latency | Use Case |
|-----------|-----------|---------|----------|
| **UDS** | 1.3+ Gbps | 11.7μs | Local IPC |
| **TCP** | 800+ Mbps | 15-20μs | Network fallback |

Supports 802.11, 802.3, Bluetooth HCI, and UWB UCI packet sizes efficiently.

## Wire Protocol

### Framing Format

```text
+------------------+------------------------+
| Length (4 bytes) | Payload (Length bytes) |
| Native Byte Order|      Application Data  |
+------------------+------------------------+
```

**Design Decisions:**
- **Native byte order**: Optimized for local IPC where both processes share the same endianness.
- **Simple framing**: Minimal overhead suitable for high-frequency packet streams.

### Message Types

1.  **`InitInfo` Message**: The first message on every connection (except `DualFd`).
    - Contains `ChipInfo` with device/chip specifications.
    - Handled automatically by the transport listeners and `Streams::connect`.
2.  **Application Packets**: All subsequent messages.
    - Raw packet data with the length prefix.

## Error Handling

The crate defines a main `PacketStreamError` enum with variants for different failure modes:

```rust
pub enum PacketStreamError {
    Protocol(ProtocolError),    // e.g., init_info failures
    Socket(SocketError),        // Platform transport errors
    Config(ConfigError),        // Configuration validation
    Io(std::io::Error),
    // ...
}
```
This allows for granular error handling by the application.

## Platform Support

- **Unix/Linux/macOS**: Unix Domain Sockets (best performance)
- **Windows**: Named Pipes with TCP fallback
- **VMs**: VSOCK for guest-host communication
- **Containers**: Volume mounts or port forwarding