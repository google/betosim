# Netsim Bluetooth Crate

This crate is the reference implementation for the `netsim-next` architecture, a pure-Rust system for managing emulated chips. It provides a simulation tool for multi-device Bluetooth use cases, offering radio-level control and HCI tracing.

## Architecture

The crate's design is based on a clear separation between a message-passing **Control Plane** and a transport-agnostic **Data Plane**.

### Control Plane: Message Passing

All component lifecycle and configuration is handled through a message-passing system, inspired by the actor model. This eliminates complex locking and prevents deadlocks.

-   **`BluetoothManager`**: The central component, acting as a "technology service." It runs in its own concurrent task and owns all state related to the Bluetooth simulation (e.g., the list of active chips).
-   **`BluetoothCommand`**: Clients interact with the manager by sending messages from this enum (e.g., `CreateChip`, `DeleteChip`) over a channel. The manager processes these commands sequentially in its event loop.
-   **Worker Components**: For each chip, the `BluetoothManager` spawns a dedicated concurrent task (a "worker") that manages the chip's specific logic.

### Data Plane: Agnostic Packet Streaming

All high-frequency packet I/O is handled through a generic `PacketStreamerApi` trait, which abstracts the underlying transport protocol (e.g., gRPC, file descriptors).

-   Each worker component (`VirtualDeviceChip`, `ScannerChip`) takes ownership of a `Box<dyn PacketStreamerApi>`.
-   This gives each chip a completely isolated data path, meaning a failure in one chip's stream does not affect any other.
-   The worker's main loop is a simple `tokio::select!` that shuttles packets between the client stream and the `rootcanal` backend.

## Component Overview

-   **`BluetoothManager`**: The central coordinator for all Bluetooth simulation. It is the main entry point for the crate.
-   **`VirtualDeviceChip`**: A worker that represents the full Host Controller Interface (HCI) for a virtualized Android device. It bridges the client's packet stream with the `rootcanal` simulation backend.
-   **`BeaconChip`**: A worker that emulates a simplified Bluetooth Low Energy (BLE) beacon, which only requires sending HCI commands to `rootcanal` to configure its advertising state.
-   **`ScannerChip`**: A worker that captures all nearby Bluetooth traffic and forwards it to the client over its packet stream.

## Building the Crate

You can build the Rust library using Bazel:

```bash
bazel build //next/bluetooth
```

## Running Tests

To run all unit tests and linter checks, use the following Bazel command:

```bash
bazel test //next/bluetooth:tests
```

### Test-Only Functions

This crate includes functions intended only for testing, which are exposed under the `test-utils` feature flag. This feature is automatically enabled when running the tests through Bazel.

## Generating Documentation

To generate and view the crate's documentation, run the following command:

```bash
bazel build //next/bluetooth:doc
```

The generated documentation can be found in the `bazel-bin/rust/bluetooth/doc.rustdoc/bluetooth/index.html` file.
