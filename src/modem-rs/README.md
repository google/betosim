# Modem-RS

`modem-rs` is a library that provides a flexible and extensible cellular modem
simulator. It is designed to be used as a backend for testing and development of
applications that interact with cellular modems via standard AT commands.

## Documentation Structure

This directory contains several documentation files serving different purposes:

- **[Development Roadmap & Status (ROADMAP.md)](ROADMAP.md)**: Detailed tracking
  of the rewrite plan, active tasks, and feature parity checklist against the
  legacy C++ implementation.
- **[Modem Derive Macro Stats](modem-derive/README.md)**: Documentation for the
  `modem-derive` procedural macro crate used for automatic AT command parsing.

## Feature Implementation Status

The table below summarizes the current implementation state of various modem
services as of Feb 2026. This serves as a quick reference for developers and
testers.

| Service             | Implemented & Tested                      | Missing / Stubbed                                              |
| :------------------ | :---------------------------------------- | :------------------------------------------------------------- |
| **Call Service**    | Dial, Answer, Hangup, DTMF, Mute, Hold    | Multiparty logic is basic                                      |
| **Data Service**    | Context definition, QoS, Activate, Attach | Real packet flow (PDP data plane)                              |
| **Misc Service**    | Vol, Echo, Identity (ATI, GMI...), Config | Complex char framing logic                                     |
| **Network Service** | Signal (CSQ), Operator Query              | **Operator Selection (`AT+COPS=`), Registration (`AT+CREG=`)** |
| **SIM Service**     | PIN, IO, Channels, Auth                   | Full filesystem emulation (EF/DF structure)                    |
| **SMS Service**     | Send, Read, Delete, Storage               | PDU Mode low-level details                                     |
| **STK Service**     | Envelope (Menu/Input)                     | **Terminal Response (`AT+CUSATT`)**                            |
| **Sup Service**     | CLIR, USSD, Call Forwarding               | **CLIP Query (`AT+CLIP?`)**                                    |

---

## Architecture

The `modem-rs` library is built around a central `CellularNetworkSimulator`
struct, which manages the state of the simulated cellular network and all the
modem instances within it. Each modem instance dispatches AT commands to various
service modules. Each service module is responsible for handling a specific
aspect of the modem's functionality, such as call control, SMS, or network
registration.

### Core Components

- **`CellularNetworkSimulator`**: The main public entry point for the library.
  It is responsible for creating, managing, and communicating with modem
  instances.
- **`Modem`**: The main struct that represents a single simulated modem. It
  contains the modem's state, including the SIM card, network registration
  status, and active calls.
- **Services**: Each service is a separate module that implements a specific set
  of AT commands. The services are part of a `Modem` instance and are called
  upon to handle incoming commands.
  - `CallService`: Manages voice calls, including dialing, answering, and
    hanging up.
  - `SmsService`: Handles sending and receiving SMS messages.
  - `NetworkService`: Manages network registration and signal quality.
  - `SimService`: Simulates the SIM card and handles PIN authentication.
  - `SupService`: Handles supplementary services like call forwarding and caller
    ID.
  - `StkService`: Manages SIM Toolkit (STK) functionality.
  - `DataService`: Manages packet data connections.
- **`Parser`**: The `parser` module is responsible for parsing incoming AT
  command strings into structured `Command` enums. It uses the `modem-derive`
  crate to automatically generate parsing logic for each command.
- **Event Loop & Time**: The `CellularNetworkSimulator` runs an event loop that
  processes time-based events (like call timeouts). To make this testable, the
  simulator uses a `Clock` trait, allowing a `MockClock` to be injected during
  tests to control time, while the real `SystemClock` is used in production.

### State Management

The state of the simulation is managed by the `CellularNetworkSimulator` and the
various `Modem` instances and their service modules. The
`CellularNetworkSimulator` holds the top-level state, while each `Modem` manages
its own internal state, which is further delegated to the individual services.
The state is updated in response to AT commands and other events.

## Example Usage

Here is an example of how to instantiate and use the `CellularNetworkSimulator`
struct:

```rust
use modem_rs::{CellularNetworkSimulator, NetworkCallbacks, Callbacks, ModemId};
use std::sync::Arc;

// Define mock callback handlers
struct MockNetworkCallbacks;
impl NetworkCallbacks for MockNetworkCallbacks {
    fn on_new_remote_connection(&self, _modem_id: ModemId, _destination: String) {}
    fn on_modem_hanged_up(&self, _modem_id: ModemId) {}
}

struct MockModemCallbacks;
impl Callbacks for MockModemCallbacks {
    fn send_at_response(&self, _modem_id: ModemId, response: &[u8]) {
        println!("Modem response: {}", String::from_utf8_lossy(response));
    }
}

// Create a new network simulator. This will use the real system clock.
let network_callbacks = Arc::new(MockNetworkCallbacks);
let simulator = CellularNetworkSimulator::new(network_callbacks);

// Create a new modem instance
let modem_id = 1;
let modem_callbacks = Arc::new(MockModemCallbacks);
simulator.new_modem(modem_id, modem_callbacks).unwrap();

// Send an AT command to the modem
simulator.send_at_command(modem_id, b"AT+CPIN?\r\n");

```
