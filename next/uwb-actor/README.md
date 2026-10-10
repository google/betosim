# Netsim Uwb Service

This crate implements the `UwbServer` actor, responsible for managing simulated uwb controllers, interfacing with a uwb emulator backend (`pica`).

## Features

-   Manages multiple uwb controller instances, each associated with a `ChipId`.
-   Handles `ChipRequest` messages for creating, deleting, and reading chip information.
-   Integrates with the `netsim` device server for chip lifecycle management.
-   Utilizes `PacketStream` and `PacketSink` from `netsim-api` for communication channels.

## Interaction

Interaction with the `UwbServer` is done via the `ChipClient` from the `netsim-api` crate, sending `ChipRequest` messages. The `UwbServer` is typically spawned and run within the main `netsimd` process.

## Building and Testing

```bash
# Build
bazel build //next/uwb-actor

# Test
bazel test //next/uwb-actor:tests
```
