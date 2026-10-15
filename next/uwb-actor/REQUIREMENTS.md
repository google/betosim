# Requirements: UWB Actor

UWB-Actor - UCI Bridge and Pica Orchestrator

## 1. Introduction

### 1.1 Purpose

UWB-Actor is a user-space microservice responsible for orchestrating the Ultra-Wideband (UWB) simulation. It bridges the generic Chip-Actor framework to the **Pica** UWB simulation library.

It acts as the:

- **UCI Transport Layer**: Transports UCI (UWB Command Interface) packets between the Guest (AVD) and Pica.
- **Chip Lifecycle Manager**: Manages the creation and destruction of Pica "Devices" (Chips).
- **Ranging Facilitator**: Propagates spatial updates (position/orientation) to Pica to enable Time-of-Flight (ToF) and Angle-of-Arrival (AoA) calculations.

### 1.2 Scope

The UWB-Actor component handles the logic for:

- **Lifecycle Management**: Creates UWB Chips in the simulation.
- **UCI Bridging**: Forwards UCI commands, responses, and notifications.
- **Pica Integration**: Interfaces with the Pica Rust crate.
- **Telemetry**: Reports Tx/Rx statistics.
- **Spatial Updates**: Propagates position and orientation data to the Pica ranging estimator.

The following are **out of scope**:

- **Ranging Logic**: The physics of ToF/AoA are calculated by **Pica**.
- **MAC/PHY Logic**: UWB frames and scheduling are handled by **Pica**.
- **Fault Injection**: Simulating hardware failures or corrupted packets

### 1.3 Definitions and Acronyms (Glossary)

| Term           | Definition                                                                      |
| -------------- | ------------------------------------------------------------------------------- |
| **UCI**        | UWB Command Interface (Communication protocol between Host and UWB Chip).       |
| **Pica**       | Open-source UWB MAC/PHY simulator used by Netsim.                               |
| **Ranging**    | The process of measuring the distance and angle between two UWB Chips.          |
| **Controller** | Synonymous with **Chip** in this document; the simulated radio entity.          |

## 2. Overall Description

### 2.1 Product Perspective

UWB-Actor sits between the **Device-Actor** (upstream) and **Pica**
(downstream).

- It is a **ChipClient** of the Device-Actor.
- It manages a **Pica** Handle/Session.
- It provides the input/output pipes that connect the User's External Device to
  the internal Pica simulation.

### 2.2 User Characteristics

- **Emulator Users**: Use AVDs that connect via gRPC. They perceive the UWB-Actor as a "real" UWB Chip via the UCI interface.
- **Test Engineers**: Use Python scripts (e.g., Mobly) to inject UCI commands directly to verify behavior.

### 2.3 User Needs

- **UN-001**: **Fidelity**: The Chip must behave like standard-compliant FiRa/CCC UWB hardware.
- **UN-002**: **Ranging**: The simulation must support realistic ranging
  scenarios based on spatial positioning.
- **UN-003**: **Observability**: Users must be able to see packet counts and
  connection status.

### 2.4 Constraints

- **Pica Dependency**: The actor relies entirely on Pica for protocol
  correctness and ranging physics.
- **Async Architecture**: Must handle UCI streams without blocking the main
  actor loop.

### 2.5 Assumptions and Dependencies

- **Pica** library is available and linked.
- **Device-Actor** is responsible for assigning ChipIDs.

## 3. Standards and References

- **FiRa UCI Technical Specification v2.0.0**: Defines UCI.
- **Pica Project**: [Source Code](https://github.com/google/pica)
- **Android UWB**:
  [Android UWB HAL Interface](https://source.android.com/docs/core/connect/uwb-hal-interface)

## 4. Functional Requirements

> [!IMPORTANT] All functional requirements listed below are mandatory.

### 4.1 Chip Lifecycle (CRUD)

- **RQ-LIFE-01**: The system shall provision a UWB Chip with a unique `ChipId` provided by the Device-Actor.
- **RQ-LIFE-02**: The system shall allow deleting a chip, which must clean up
  the associated Pica handle/state.
- **RQ-LIFE-03**: The system shall provide a `List` API returning all active UWB chips.
- **RQ-LIFE-04**: The system shall instantiate a single global `Pica` instance (Singleton) that supports multiple concurrent UWB Chips (mapped to distinct Pica Handles).
- **RQ-LIFE-05**: The system shall support a `Reset` operation (Soft Reset) that clears the internal state of a specific UWB chip without destroying the handle.

### 4.2 UCI Transport

- **RQ-UCI-01**: The system shall bridge UCI commands and responses between a `PacketStream`/`PacketSink` (AVD/User) and a Pica Chip.
- **RQ-UCI-02**: The system shall handle `PacketSink` backpressure or disconnection gracefully.
- **RQ-UCI-03**: The system shall transparently pass through UCI commands to Pica.

### 4.3 Signal Propagation & Ranging

- **RQ-SPATIAL-01**: The system shall apply **Topology Updates** (Position, Orientation) to a thread-safe shared state.
- **RQ-SPATIAL-02**: The system shall provide a custom ranging estimator to Pica, enabling queries with minimal overhead.
- **RQ-SPATIAL-03**: The system shall support creating and destroying Pica Anchors (virtual devices) modeled as chips (`UwbMode::Anchor`), enabling simulated chips to range with virtual anchors.

### 4.4 Observability

- **RQ-OBS-01**: **Statistics**: The system shall report `tx_bytes` and `rx_bytes` (UCI packets) for each chip via the `GetStatistics` action.
- **RQ-OBS-02**: **PCAP**: The system shall support writing a PCAP-compatible packet capture (e.g., generic linktype) for all UCI traffic when packet capture is enabled.

## 5. Non-Functional Requirements

- **RQ-NFR-LATENCY**: UCI Command->Response round trip should be minimized.
- **RQ-NFR-CONCURRENCY**: The system shall support multiple concurrent UWB Chips.

## 6. Verification

### 6.1 Automated Tests

- **Unit/Integration Tests**:
  - `chip_create_test.rs`: Verify Create operations.
  - `chip_lifecycle_test.rs`: Verify Delete/Get operations.
  - `stats_test.rs`: Verify Statistics reporting.

### 6.2 BDD Coverage Mapping

| Requirement ID    | Requirement Description        | Test File                | Test Scenario                                                            | Coverage |
| :---------------- | :----------------------------- | :----------------------- | :----------------------------------------------------------------------- | :------- |
| **RQ-LIFE-01**    | Create UWB Chip             | `chip_create_test.rs`    | `test_create_and_get_chip`                                               | **Full** |
| **RQ-LIFE-02**    | Delete Chip                    | `chip_lifecycle_test.rs` | `test_delete_chip`                                                       | **Full** |
| **RQ-LIFE-03**    | List Chips                     | `N/A`                    | **Missing**                                                              | **None** |
| **RQ-LIFE-04**    | Singleton Pica Instance        | `N/A`                    | **Missing**                                                              | **None** |
| **RQ-LIFE-05**    | Chip Reset                     | `chip_lifecycle_test.rs` | `test_reset_chip`                                                        | **Full** |
| **RQ-UCI-01**     | Bridge UCI (Bi-directional)    | `N/A`                    | **Missing** (Requires Pica integration test)                             | **None** |
| **RQ-UCI-02**     | Backpressure Handling          | `N/A`                    | **Missing**                                                              | **None** |
| **RQ-UCI-03**     | Transparent Passthrough        | `N/A`                    | **Missing**                                                              | **None** |
| **RQ-SPATIAL-01** | Propagate Position/Orientation | `N/A`                    | **Missing**                                                              | **None** |
| **RQ-SPATIAL-02** | Ranging Updates                | `N/A`                    | **Missing**                                                              | **None** |
| **RQ-SPATIAL-03** | Anchor CRUD & Ranging          | `ranging_test.rs`        | `test_ranging_between_chip_and_anchor`, `test_anchor_crud_errors`        | **Full** |
| **RQ-OBS-01**     | Get Statistics                 | `stats_test.rs`          | `test_read_statistics`                                                   | **Full** |
| **RQ-OBS-02**     | PCAP Capture                   | `pcap_test.rs`           | `test_pcap_capture`                                                      | **Full** |

### 6.3 Coverage Gaps

- **RQ-UCI-01/02 (Transport)**: No integration tests yet verify actual UCI
  packet flow to/from Pica (Pica might not be fully linked in `next` yet).
- **RQ-SPATIAL-01/02 (Ranging)**: No tests verify that position updates are
  successfully passed to the underlying Pica instance.
- **RQ-LIFE-03 (List)**: No explicit test for listing all chips.
