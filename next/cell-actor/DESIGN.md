# Cell Actor Architecture

## Overview

The `cell` actor uses the standard `ResourceActor` pattern (`CellActor` implementing `ActorService` + `ActorLifecycle`), aligning with other technology actors like `uwb-actor` and `bluetooth-actor`. This ensures consistent lifecycle management, concurrency control, and type safety across the Netsim codebase.

## Architecture

The `CellActor` is built upon the `ResourceActor<T>` abstraction, utilizing the `ActorService` trait to handle messages and the `ActorLifecycle` trait to manage streams and background tasks.

### Core Components

1.  **`CellActor` Struct**: Holds the state of the cellular network simulator.
    ```rust
    pub struct CellActor {
        pub device_client: DeviceClient,
        pub controller: Box<dyn ModemNetworkInterface>,
        pub active_chips: HashMap<ChipId, ChipState>,
        // ...
    }
    ```

2.  **`ActorService` Implementation**: Handles CRUD operations for simulated chips (`ChipRequest`).
    - `handle_create`: Initializes a new modem instance in the `ModemNetworkInterface`.
    - `handle_delete`: Removes the modem instance and cleans up associated state.

3.  **`ActorLifecycle` Implementation**: Manages data flow and task events.
    - `on_stream`: Receives packets from the virtual chip (guest) and forwards them to the modem simulator.
    - `on_stream_closed`: Handles the disconnection of a guest chip.

### Stream & Sink Management

Unlike the legacy manual implementation, `CellActor` leverages the `actor-framework` for stream multiplexing:
- **Packet Stream**: Incoming data from guest devices is delivered via `on_stream`.
- **Packet Sink**: Outgoing data to guest devices is written directly by the `ModemNetworkInterface` (which holds a `PacketSink`).

## Key Benefits

- **Consistency**: Aligns `cell` with `uwb`, `bluetooth`, and `wifi` actors.
- **Maintainability**: Reduced boilerplate by utilizing the shared `actor-framework`.
- **Robustness**: Leverages the tested `ResourceActor` state machine and error handling.

