# Netsim Cell Service

This crate implements the `CellServer` actor, responsible for managing simulated
cellular controllers, interfacing with a modem emulator backend (like
`modem-rs`).

## Features

- Manages multiple cellular controller instances, each associated with a
  `ChipId`.
- Handles `ChipRequest` messages for creating, deleting, and reading chip
  information.
- Uses a callback mechanism (`CellularCallbacks`) for the modem emulator to send
  data and events back to the host.
- Integrates with the `netsim` device server for chip lifecycle management.
- Utilizes `PacketStream` and `PacketSink` from `netsim-api` for communication
  channels.

## Interaction

Interaction with the `CellServer` is done via the `ChipClient` from the
`netsim-api` crate, sending `ChipRequest` messages. The `CellServer` is
typically spawned and run within the main `netsimd` process.

## Stub Implementation

The crate includes a `StubCellularController` that implements the
`CellularControllerInterface`. This is used for testing the `CellServer` logic
independently of the actual `modem-rs` implementation. It currently echoes back
any data sent to it.

## Building and Testing

```bash
# Build
bazel build //next/cell-actor

# Test
bazel test //next/cell-actor:integration_test
```
