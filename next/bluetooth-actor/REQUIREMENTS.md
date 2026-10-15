# Requirements: Bluetooth Actor

Bluetooth-Actor - HCI Bridge and RootCanal Orchestrator

## 1. Introduction

### 1.1 Purpose

Bluetooth-Actor is a user-space microservice responsible for orchestrating the
Bluetooth controller simulation. It bridges virtual devices (AVD, Bumble,
Beacons) to the underlying **RootCanal** Bluetooth PHY simulation engine.

It acts as the:

- **HCI Transport Layer**: Moving HCI packets between the Guest (AVD) and
  RootCanal.
- **Controller Lifecycle Manager**: Creating/Destroying `RootCanal` controllers.
- **Simulation Facade**: Exposing high-level constructs like "Beacons" and
  "Scanners" that abstract complex RootCanal configuration.

### 1.2 Scope

The Bluetooth-Actor component handles the logic for:

- **Lifecycle Management**: Creating/Deleting Bluetooth chips.
- **HCI Bridging**: Forwarding HCI commands/events between the External Device
  stream and the RootCanal controller.
- **Specialized Chip Types**:
  - **External Device**: Standard HCI controller for AVD/Bumble.
  - **Beacon**: Simplified BLE Beacon (Tx only).
  - **Scanner**: Passive packet capture (Rx only).
- **Telemetry**: Reporting Tx/Rx statistics from RootCanal.
- **Spatial Updates**: Propagating position changes to RootCanal.

The following are **out of scope**:

- **Link Layer Logic**: State machines (Advertising, Scanning, Connecting) are
  handled by **RootCanal**.
- **RF Physics Model**: Path loss and interference are calculated by
  **RootCanal** (or the Link-Actor, depending on the architecture - currently
  RootCanal handles its own PHY).

### 1.3 Definitions and Acronyms (Glossary)

| Term                | Definition                                                                                          |
| ------------------- | --------------------------------------------------------------------------------------------------- |
| **HCI**             | Host Controller Interface (Communication protocol between Host Stack and Controller).               |
| **RootCanal**       | Google's reference Bluetooth Controller emulation library (simulates Link Layer and PHY).           |
| **Beacon**          | A Bluetooth Low Energy (BLE) device that broadcasts advertisements but does not accept connections. |
| **Scanner**         | A passive device that listens to Over-the-Air (OTA) traffic for debugging/verification.             |
| **Controller**      | The simulated Bluetooth radio implementation (RootCanal instance).                                  |
| **AVD**             | Android Virtual Device (Emulator).                                                                  |
| **Bumble**          | Python-based Bluetooth Host stack used for testing.                                                 |
| **External Device** | An entity (e.g. AVD) running a Host Stack that connects to Netsim's HCI port.                       |

## 2. Overall Description

### 2.1 Product Perspective

Bluetooth-Actor sits between the **Device-Actor** (upstream) and **RootCanal**
(downstream).

- It is a **ChipClient** of the Device-Actor.
- It owns the **RootCanal** FFI/Rust instance.
- It provides the input/output pipes (Packet Stream) that connect the User's
  External Device to the internal simulation.

### 2.2 User Characteristics

- **Emulator Users**: Use AVDs that connect via gRPC. They perceive the
  Bluetooth-Actor as a "real" Bluetooth controller.
- **Test Engineers (Bumble)**: Use Python scripts to inject HCI commands
  directly to verify behavior.
- **Simulation Operators**: Create specialized entities (Beacons) via CLI to
  test scanning scenarios.

### 2.3 User Needs

- **UN-001**: **Fidelity**: The controller must behave like a standard compliant
  Bluetooth 4.2/5.x hardware.
- **UN-002**: **Performance**: HCI transport must introduce minimal latency
  (<2ms) to prevent timeouts in the Host Stack.
- **UN-003**: **Observability**: Users must be able to see packet counts and
  inspect OTA traffic via Scanners.

### 2.4 Constraints

- **RootCanal Dependency**: The actor relies entirely on RootCanal for protocol
  correctness.
- **Async Architecture**: Must handle high-throughput HCI streams without
  blocking the main actor loop.

### 2.5 Assumptions and Dependencies

- **RootCanal** library is linked and available.
- **Device-Actor** is responsible for assigning ChipIDs.

## 3. Standards and References

- **Bluetooth Core Specification (5.x)**: Defines HCI and Link Layer behavior.
- **RootCanal**:
  [Source Code](https://cs.android.com/android/platform/superproject/+/main:packages/modules/Bluetooth/tools/rootcanal/)
- **ITU-R P.525**: [Free Space Path Loss](https://www.itu.int/rec/R-REC-P.525/en) - Calculation of free-space attenuation (Standard reference for FSPL).
- **Wikipedia**: [Free-space path loss](https://en.wikipedia.org/wiki/Free-space_path_loss).

## 4. Functional Requirements

> [!IMPORTANT] All functional requirements listed below are mandatory (Priority
> P0).

### 4.1 Chip Lifecycle (CRUD)

- **RQ-LIFE-01**: The system shall allow provisioning a Bluetooth Chip as one of
  three distinct **types**:
  - **External Device**: Full HCI Controller (requires Packet Stream).
  - **Beacon**: Tx-only LE Advertiser (Configuration driven).
  - **Scanner**: Rx-only Packet Dump (Requires Sink).
- **RQ-LIFE-02**: The system shall assign a unique `ChipId` provided by the
  Device-Actor.
- **RQ-LIFE-03**: The system shall allow deleting a chip, which must clean up
  the associated RootCanal controller instance.
- **RQ-LIFE-04**: The system shall provide a `List` API returning all active
  Bluetooth chips.

### 4.2 HCI Transport

- **RQ-HCI-01**: The system shall bridge the incoming `PacketStream` (from
  AVD/User) to the RootCanal Controller's Input.
- **RQ-HCI-02**: The system shall bridge the RootCanal Controller's Output
  (Events/ACL) to the `PacketSink` (to AVD/User).
- **RQ-HCI-03**: The system shall handle `PacketSink` backpressure or
  disconnection gracefully (dropping packets if necessary, avoiding panics).

### 4.3 Signal Propagation & RSSI

- **RQ-SIG-01 (Hybrid RSSI)**: The system shall calculate Received Signal Strength (RSSI) using the following precedence:
    1. **Fixed Link Override**: If a static link exists (via `Link-Actor`) between two chips, use the configured RSSI values (Rx/Tx).
    2. **Spatial Calculation**: If no link override exists, calculate path loss using the **Free Space Path Loss (FSPL)** model based on the Euclidean distance between chips.
- **RQ-SIG-02**: The system shall accept **Topology Updates** (Position, Orientation, **Path Loss Overrides**) and propagate them to `RootCanal`. Specifically, Path Loss/RSSI overrides received via the `Link-Actor` must take immediate effect, triggering the precedence logic defined in RQ-SIG-01.

### 4.4 Observability

- **RQ-OBS-01**: **Statistics**: The system shall report `tx_bytes` and
  `rx_bytes` (Link Layer packets) for each chip via the `GetStatistics` action.
- **RQ-OBS-02**: **Scanner (Advertising)**: The system shall run in a passive
  HCI Scanning mode to capture Over-the-Air (OTA) advertising packets and
  forward them as HCI events to the `PacketSink`.
- **RQ-OBS-03** (Priority P2): **Scanner (Link Layer)**: The system shall
  capture raw Link Layer packets from `RootCanal` (Promiscuous Mode).
- **RQ-OBS-04** (Priority P2): **Packet Conversion**: The system shall convert
  internal `RootCanal` Link Layer packets into a standard PCAP-friendly format
  (e.g., Nordic BLE Sniffer format) before writing to the `PacketSink`.

### 4.5 Configuration

- **RQ-CONF-01**: **Beacon Config**: The system shall accept parameters for
  Beacon configuration, specifically:
  - **Advertising Interval**: Configurable (min/max).
  - **Advertising Data**: Custom payload (Manufacturer Data, Service UUIDs).
  - **Tx Power**: Configurable output power.
  - **Own Address Type**: Support for Random/Public addresses.

## 5. Non-Functional Requirements

- **RQ-NFR-LATENCY**: HCI Command->Event round trip should be minimized (target
  < 5ms internal processing).
- **RQ-NFR-CONCURRENCY**: Must support at least 32 concurrent Bluetooth
  controllers.
- **RQ-NFR-STABILITY**: The system shall handle malformed HCI packets from the
  Packet Stream without crashing, ensuring isolation between External Devices
  and the core simulation.

## 6. Verification

### 6.1 Automated Tests

- **Unit Tests**:
  - `lifecycle_tests.rs`: Verify Create/Delete/List operations.
  - `update_chip_test.rs`: Verify position updates are applied.
- **Integration Tests**:
  - `beacon_tests.rs`: Verify Beacon creation.
  - `scanner_tests.rs`: Verify Scanner receives advertisements from a Beacon.

### 6.2 BDD Coverage Mapping

| Requirement ID  | Requirement Description                      | Test File             | Test Scenario                                                            | Coverage    |
| :-------------- | :------------------------------------------- | :-------------------- | :----------------------------------------------------------------------- | :---------- |
| **RQ-LIFE-01**  | Create Chip (External Device, Beacon, Sniff) | `lifecycle_tests.rs`  | `test_add_chip` (External Device), `test_add_beacon`, `test_add_sniffer` | **Full**    |
| **RQ-LIFE-02**  | Unique ChipId                                | `lifecycle_tests.rs`  | Indirectly verified by managing multiple chips                           | **Full**    |
| **RQ-LIFE-03**  | Delete Chip                                  | `lifecycle_tests.rs`  | `test_remove_chip`                                                       | **Full**    |
| **RQ-HCI-01**   | Bridge Input Stream (Host->Ctrl)    | `lifecycle_tests.rs`  | `test_hci_reset_command` (Sends HCI Reset)                      | **Full**    |
| **RQ-HCI-02**   | Bridge Output Sink (Ctrl->Host)     | `lifecycle_tests.rs`  | `test_hci_reset_command` (Receives Command Complete)            | **Full**    |
| **RQ-SIG-01**   | Hybrid RSSI (Link > Spatial)        | `update_chip_test.rs` | `test_update_chip` (Verifies properties passed)                 | **PARTIAL** |
| **RQ-SIG-02**   | Propagate Position                  | `update_chip_test.rs` | `test_update_chip`                                              | **Full**    |
| **RQ-OBS-01**   | Get Statistics                      | `N/A`                 | **Missing**                                                     | **None**    |
| **RQ-OBS-02**   | Scanner (Advertising)                        | `scanner_tests.rs`    | `test_scanner_receives_advertisement`                                    | **Full**    |
| **RQ-OBS-03**   | Scanner (Link Layer - P2)                    | `N/A`                 | **Missing**                                                              | **None**    |
| **RQ-OBS-04**   | Packet Conversion (P2)                       | `N/A`                 | **Missing**                                                              | **None**    |
| **RQ-CONF-01**  | Beacon Configuration                         | `beacon_tests.rs`     | `test_add_chip` (Only verifies creation, not parameters)                 | **PARTIAL** |

### 6.3 Coverage Gaps

- **RQ-OBS-01 (Statistics)**: `handle_action(GetStatistics)` is implemented in
  `service.rs` but there is no explicit test verifying it.
- **RQ-CONF-01 (Beacon Config)**: Requirements for Scan Response, Interval, and
  Tx Power are not currently implemented in `beacon.rs` (hardcoded values).
- **Scanner Configuration**: `scanner.rs` hardcodes scanning parameters; no
  support for filtering or specific channel scanning.
- **RQ-OBS-03/04 (Link Layer Scanner - P2)**: The advanced promiscuous mode and
  PCAP conversion logic are not currently implemented or tested.
- **RQ-SIG-01 (Hybrid RSSI)**: Tests verify position propagation, but do not
  explicitly verify the *precedence* logic where a static link overrides the spatial calculation.
