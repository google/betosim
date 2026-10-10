# Requirements: Link Actor

Link-Actor - Chip-to-Chip Link Management and Propagation Service

## 1. Introduction

### 1.1 Purpose

Link-Actor is a core microservice within `netsim` responsible for managing the
connectivity graph between emulated radio chips.

It serves as the source of truth for all links between chips, handling the
creation, modification, and deletion of these links. It synchronizes this state
across the system by processing lifecycle notifications from other actors and
propagating topology updates to relevant components.

### 1.2 Scope

The Link-Actor component handles:

- **Link Lifecycle**: Creation, Update, and Deletion of links.
- **Validation**: Ensuring links are only created between compatible chips
  (e.g., BLE to BLE).
- **State Propagation**: Pushing the current link state (peers and RSSI) to the
  relevant Chip Actors whenever a change occurs.
- **Chip Lifecycle Integration**: Automatically cleaning up links when a chip is
  removed.

The following are **out of scope**:

- **Packet Routing**: Link-Actor manages the _topology_, but the actual packet
  transmission is handled by the `PacketStream` or the Chip Actors themselves
  using the topology data provided by Link-Actor.
- **Physical Modeling**: Complex path loss models are computed externally;
  Link-Actor stores the resulting RSSI/Rx level.

### 1.3 Definitions and Acronyms

| Term     | Definition                                                                               |
| :------- | :--------------------------------------------------------------------------------------- |
| **Chip** | An emulated radio instance (e.g., Bluetooth Controller, Wi-Fi Interface).                |
| **Link** | A **directed** connection from a sender chip to a receiver chip with an associated RSSI. |
| **RSSI** | Received Signal Strength Indicator.                                                      |

## 2. Overall Description

### 2.1 Product Perspective

Link-Actor acts as the central topology manager in the `netsim-next`
architecture. It sits between the Frontend (Grpc, CLI, etc.) and the Chip
Actors.

It plays a critical role in the **Hybrid RSSI Determination** model:

1.  **Default (Distance-based)**: By default, RSSI is calculated based on the
    distance between devices (managed largely by the physical model/scene).
2.  **Explicit (Link-based)**: If a link exists in the `Link-Actor`, its
    configured RSSI **overrides** the default distance-based value.

This allows users to force specific connection states (e.g., perfect signal or
total disconnection) regardless of physical proximity.

### 2.2 User Characteristics

- **Test Engineers**: Use `netsim` CLI or scripts (Python/Mobly) to define
  network topology, control attenuation, and simulate mobility scenarios (e.g.,
  driving out of range).
- **Platform Developers**: Developers working on connectivity stacks (Bluetooth,
  UWB, Wi-Fi) who need to verify behavior under specific link conditions (RSSI
  changes, packet loss).
- **App Developers**: Indirect users who expect that emulated devices can
  discover and connect to each other in a realistic manner. They may also want
  to trigger multi-device events based on proximity or RSSI (e.g., Nearby Share,
  Digital Key).

### 2.3 User Needs

- **UN-001**: **Consistency**: Users need to know that if they set an RSSI, it
  is immediately reflected in the physical model.
- **UN-002**: **Safety**: Users should be prevented from creating invalid links
  (e.g., Bluetooth connected to Wi-Fi).

### 2.4 Constraints

- Must be non-blocking and asynchronous.
- Must handle dynamic arrival and departure of chips.

### 2.5 Assumptions and Dependencies

- Device Actor registers chip existence with Link-Actor via `NotifyChipAdded`.
- **Radio Actors** (Bluetooth, WiFi, UWB) must implement the generic
  [`ChipClient`](../model/src/chip.rs) interface to receive topology and RSSI
  updates from Link-Actor. Link-Actor is agnostic to the specific radio
  protocol.

## 3. Standards and References

- **[`netsim-model`](../model)**: Internal crate defining core types (`Link`,
  `Chip`, `ChipKind`).
- **[`netsim-proto`](../proto)**: gRPC/Protobuf definitions for the external
  API.
- **[`link-api`](../link-api)**: Internal crate defining the
  [`LinkClient`](../link-api/src/lib.rs) trait.

## 4. Functional Requirements

> [!IMPORTANT] All functional requirements listed below are mandatory (Priority
> P0).

### 4.1 Link Management

- **RQ-LINK-01**: The system shall allow creating a link between two valid,
  existing chips.
- **RQ-LINK-02**: The system shall prevent creating a link between chips of
  different kinds (e.g., mismatching `ChipKind`).
- **RQ-LINK-03**: The system shall prevent creating a self-loop (link to self).
- **RQ-LINK-04**: The system shall prevent creating a duplicate link if one
  already exists between the same pair.
- **RQ-LINK-05**: The system shall allow updating the RSSI of an existing link.
- **RQ-LINK-06**: The system shall allow deleting an existing link.
- **RQ-LINK-07**: The system shall treat links as **directed** (asymmetric). An
  RSSI update to A->B shall **not** affect B->A.

### 4.2 State Propagation ("The Push Model")

- **RQ-PROP-01**: Upon any change to a link (create, update, delete), the system
  shall push the **complete list of active peers** to the affected **sender
  chip** only.
  > **Note**: This "Full State Push" strategy ensures the chip always has an
  > authoritative view of its neighbors, avoiding state drift. It does **not**
  > update unaffected chips.
- **RQ-PROP-02**: The pushed update shall include the `ChipId` and `RSSI` for
  all current neighbors of the sender.

### 4.3 Lifecycle Management

- **RQ-LIFE-01**: The system shall track the existence of chips via
  `NotifyChipAdded` and `NotifyChipRemoved` events.
- **RQ-LIFE-02**: When a chip is removed, the system shall automatically delete
  all links where that chip is a sender or receiver.
- **RQ-LIFE-03**: When links are auto-deleted due to chip removal, the system
  shall propagate the updated state to the remaining **senders** (e.g., if A->B
  is removed because B died, A must be notified).

### 4.4 Observability and Stats

- **RQ-OBS-01**: The system shall track the total number of links created during
  the session.
- **RQ-OBS-02**: The system shall track the total number of RSSI updates
  (patches) performed.
- **RQ-OBS-03**: The system shall track the current number of active links.
- **RQ-OBS-04**: The system shall expose these statistics via a queryable
  interface for monitoring and debugging.

### 4.5 Querying

- **RQ-QUERY-01**: The system shall provide a `list` capability to retrieve all
  active links.

### 4.6 Frontend Integration

- **RQ-FRONT-01**: The system shall expose link listing capabilities via the
  gRPC Frontend API (`ListLink`).
- **RQ-FRONT-02**: The system shall expose link patching capabilities via the
  gRPC Frontend API (`PatchLink`).
- **RQ-FRONT-03**: The system shall expose link deletion capabilities via the
  gRPC Frontend API (`DeleteLink`).

## 5. Non-Functional Requirements

- **RQ-NFR-01**: **Atomicity**: Link updates must be atomic. A chip should never
  see a partial state.
- **RQ-NFR-02**: **Performance**: Link updates should be propagated to chip
  actors with minimal latency (sub-millisecond internal processing) to support
  real-time emulations.

## 6. Verification

Verification is performed via BDD-style Integration Tests in `tests/`.

### 6.1 BDD Coverage Mapping

| Requirement     | BDD Scenario                                   | Test Function                        | Test File                                                              |
| :-------------- | :--------------------------------------------- | :----------------------------------- | :--------------------------------------------------------------------- |
| **RQ-LINK-01**  | Successfully create a valid link               | `test_create_link_succeeds`          | [`link_create_test.rs`](tests/link_create_test.rs), [`link.feature`](../verify/runner/tests/features/link.feature) |
| **RQ-LINK-02**  | Fail to create link with mismatched chip kinds | `test_create_link_fails_mismatch`    | [`link_create_test.rs`](tests/link_create_test.rs)                     |
| **RQ-LINK-03**  | Fail to create loopback link                   | `test_create_link_fails_self`        | [`link_create_test.rs`](tests/link_create_test.rs)                     |
| **RQ-LINK-04**  | Fail to create duplicate link                  | `test_duplicate_create_fails`        | [`link_create_test.rs`](tests/link_create_test.rs)                     |
| **RQ-LINK-05**  | Successfully update link RSSI                  | `test_update_link_rssi`              | [`link_update_test.rs`](tests/link_update_test.rs), [`link.feature`](../verify/runner/tests/features/link.feature) |
| **RQ-LINK-06**  | Successfully delete an existing link           | `test_delete_link_succeeds`          | [`link_delete_test.rs`](tests/link_delete_test.rs), [`link.feature`](../verify/runner/tests/features/link.feature) |
| **RQ-PROP-01**  | Link updates are propagated to chip clients    | `test_link_propagation`              | [`link_propagation_test.rs`](tests/link_propagation_test.rs)           |
| **RQ-PROP-02**  | Link update (RSSI) is propagated               | `test_update_link_propagates_patch`  | [`link_propagation_test.rs`](tests/link_propagation_test.rs), [`link.feature`](../verify/runner/tests/features/link.feature) |
| **RQ-LIFE-03**  | Chip removal triggers link deletion patch      | `test_chip_removal_propagates_patch` | [`link_propagation_test.rs`](tests/link_propagation_test.rs)           |
| **RQ-FRONT-01** | List Links via gRPC                            | `test_link_wiring_grpc`              | [`grpc_integration_test.rs`](../daemon/tests/grpc_integration_test.rs), [`link.feature`](../verify/runner/tests/features/link.feature) |
| **RQ-FRONT-02** | Patch Link via gRPC                            | `test_link_wiring_grpc`              | [`grpc_integration_test.rs`](../daemon/tests/grpc_integration_test.rs), [`link.feature`](../verify/runner/tests/features/link.feature) |
| **RQ-FRONT-03** | Delete Link via gRPC                           | `test_link_wiring_grpc`              | [`grpc_integration_test.rs`](../daemon/tests/grpc_integration_test.rs), [`link.feature`](../verify/runner/tests/features/link.feature) |

### 6.2 BDD Coverage Gaps

| Requirement       | Description                                | Current Status                                                                                                          |
| :---------------- | :----------------------------------------- | :---------------------------------------------------------------------------------------------------------------------- |
| **RQ-QUERY-01**   | `list` capability                          | Covered by E2E tests in `link_cli.feature` and `link_grpc.feature`.                                                     |
| **RQ-NFR-01**     | Atomicity                                  | **Implicit**: Relying on Actor model single-threadedness. No explicit concurrency test.                                 |
| **RQ-NFR-02**     | 1ms Latency Target (Performance benchmark) | **Missing**: No explicit benchmark test suite for propagation latency.                                                  |
| **RQ-LIFE-01**    | Track Chip Existence                       | **Implicit**: verified via `create_link` success/failure, but no direct "List Chips" test.                              |
| **RQ-LIFE-02**    | Auto-delete links on chip removal          | **Partial**: Verified via propagation side-effect (RQ-LIFE-03), but state cleanup not explicitly asserted via `list()`. |
| **RQ-LINK-07**    | Asymmetric Link Behavior (A->B != B->A)    | **Missing**: No explicit test verifies that updating one direction leaves the other unchanged.                          |
| **RQ-OBS-01..04** | Observability (Stats)                      | **Missing**: No implementation for stat tracking exists yet.                                                            |

### 6.3 Unmapped Features

The following code paths exist in the implementation but do not map to a
specific functional requirement. They are primarily architectural artifacts or
client-side convenience helpers.

| Code Path                                 | Description                         | Justification                                                          |
| :---------------------------------------- | :---------------------------------- | :--------------------------------------------------------------------- |
| [`LinkActor::handle_get`](src/service.rs) | Retrieve a single link by `LinkId`. | Required by `ActorService` trait contract. Not used by business logic. |

## Appendix A: BDD Scenarios

### Link Creation

**Scenario: Successfully create a valid link**

- **Requirement**: **RQ-LINK-01**
- **Test**: [`test_create_link_succeeds`](tests/link_create_test.rs)

```gherkin
Given the link actor is running with initialized chips
When request to create a link between two existing chips
Then the link is created successfully
And the link appears in the list
```

**Scenario: Fail to create link with mismatched chip kinds**

- **Requirement**: **RQ-LINK-02**
- **Test**: [`test_create_link_fails_mismatch`](tests/link_create_test.rs)

```gherkin
Given the link actor is running with BLE and WIFI chips
When request to create a link between a BLE chip and a WIFI chip
Then the creation fails
```

**Scenario: Fail to create link with non-existent chip**

- **Requirement**: **RQ-LINK-01** (Negative Case)
- **Test**: [`test_create_link_fails_missing`](tests/link_create_test.rs)

```gherkin
Given the link actor is running
When request to create a link involving a non-existent chip ID
Then the creation fails
```

**Scenario: Fail to create duplicate link**

- **Requirement**: **RQ-LINK-04**
- **Test**: [`test_duplicate_create_fails`](tests/link_create_test.rs)

```gherkin
Given a link already exists between two chips
When request to create another link between the same chips
Then the creation fails
```

**Scenario: Fail to create loopback link**

- **Requirement**: **RQ-LINK-03**
- **Test**: [`test_create_link_fails_self`](tests/link_create_test.rs)

```gherkin
Given a chip
When request to create a link from the chip to itself
Then the creation fails
```

### Link Update

**Scenario: Successfully update link RSSI**

- **Requirement**: **RQ-LINK-05**
- **Test**: [`test_update_link_rssi`](tests/link_update_test.rs)

```gherkin
Given a link exists
When request to update the link's RSSI
Then the link reflects the new RSSI value
```

**Scenario: Fail to update non-existent link**

- **Requirement**: **RQ-LINK-05** (Negative Case)
- **Test**: [`test_update_missing_link_fails`](tests/link_update_test.rs)

```gherkin
Given the link actor is running
When request to update a non-existent link
Then the update fails
```

### Link Deletion

**Scenario: Successfully delete an existing link**

- **Requirement**: **RQ-LINK-06**
- **Test**: [`test_delete_link_succeeds`](tests/link_delete_test.rs)

```gherkin
Given a link exists between two chips
When request to delete the link
Then the link is removed from the list
```

**Scenario: Fail to delete non-existent link**

- **Requirement**: **RQ-LINK-06** (Negative Case)
- **Test**: [`test_delete_missing_link_fails`](tests/link_delete_test.rs)

```gherkin
Given the link actor is running
When request to delete a non-existent link ID
Then the deletion fails
```

### Link Propagation

**Scenario: Link updates are propagated to chip clients for creation and
deletion**

- **Requirement**: **RQ-PROP-01**
- **Test**: [`test_link_propagation`](tests/link_propagation_test.rs)

```gherkin
Given a mock chip client expecting updates
When a link is created and then deleted
Then the chip client receives updates reflecting these changes
```

**Scenario: Link update (RSSI) is propagated to chip clients**

- **Requirement**: **RQ-PROP-02**
- **Test**:
  [`test_update_link_propagates_patch`](tests/link_propagation_test.rs)

```gherkin
Given a link exists
When the link RSSI is updated
Then the chip client receives an update with the new RSSI
```

**Scenario: Chip removal triggers link deletion patch to peer**

- **Requirement**: **RQ-LIFE-03**
- **Test**:
  [`test_chip_removal_propagates_patch`](tests/link_propagation_test.rs)

```gherkin
Given a link exists between two chips
When one chip is removed
Then the peer chip receives an update removing the link
```

**Scenario: Scan Result with Configured RSSI**

- **Requirement**: **RQ-PROP-02**
- **Test**: [`link.feature`](../verify/runner/tests/features/link.feature)

```gherkin
Given @adb has 2 attached devices
When @netsim links @android:1 to @android:2 by bluetooth RSSI -60 as "link"
And @android:1 advertises with name "Netsim" and TxPower "HIGH"
And @android:2 starts scanning
Then @android:2 sees advertisement "Netsim" with RSSI "-60"
```

### Asymmetric Behavior

**Scenario: Asymmetric Link Update**

- **Requirement**: **RQ-LINK-07**
- **Test**: `(Missing)`

```gherkin
Given links exist from A to B and B to A
When I update the RSSI of the link A -> B
Then the link B -> A retains its original RSSI
```

### Querying

**Scenario: List all active links**

- **Requirement**: **RQ-QUERY-01**
- **Test**: `(Missing)`

```gherkin
Given multiple links exist between different chips
When I request a list of all links
Then I receive all created links with correct properties
```

### Lifecycle State

**Scenario: Track Chip Existence**

- **Requirement**: **RQ-LIFE-01**
- **Test**: `(Missing)`

```gherkin
Given no chips are registered
When I notify that Chip A exists
Then the link actor acknowledges Chip A
```

**Scenario: Links are removed from internal state upon chip removal**

- **Requirement**: **RQ-LIFE-02**
- **Test**: `(Missing)`

```gherkin
Given a link exists between Chip A and Chip B
When Chip A is removed
Then the link list should be empty
```

### Non-Functional

**Scenario: Latency Benchmark**

- **Requirement**: **RQ-NFR-02**
- **Test**: `(Missing)`

```gherkin
Given a steady state
When 1000 link updates are performed
Then the average propagation latency is under 1ms
```

**Scenario: Atomic Updates**

- **Requirement**: **RQ-NFR-01**
- **Test**: `(Missing/Implicit)`

```gherkin
Given a link exists with RSSI -50
When an update to -70 is processed
Then no observer sees a partial state or undefined RSSI during the transition
```

### Observability

**Scenario: Stats Tracking**

- **Requirement**: **RQ-OBS-01**, **RQ-OBS-02**, **RQ-OBS-03**
- **Test**: `(Missing)`

```gherkin
Given no links exist (clean state)
When I create 3 links and update 1 of them twice
Then the "Total Links Created" stat is 3
And the "Total RSSI Updates" stat is 2
And the "Active Links" stat is 3
```

**Scenario: Active Link Count Decrement**

- **Requirement**: **RQ-OBS-03**
- **Test**: `(Missing)`

```gherkin
Given 3 active links
When I delete 1 link
Then the "Active Links" stat becomes 2
```

**Scenario: Query Stats Interface**

- **Requirement**: **RQ-OBS-04**
- **Test**: `(Missing)`

```gherkin
Given the link actor has processed operations
When I query the stats interface
Then I receive a JSON/Proto response with current counters
```

### Frontend Integration

**Scenario: List Links via gRPC**

- **Requirement**: **RQ-FRONT-01**
- **Test**: [`test_link_wiring_grpc`](../daemon/tests/grpc_integration_test.rs)

```gherkin
Given multiple links exist
When I call the gRPC `ListLink` method
Then I receive a list of all active links
```

**Scenario: Patch Link via gRPC**

- **Requirement**: **RQ-FRONT-02**
- **Test**: [`test_link_wiring_grpc`](../daemon/tests/grpc_integration_test.rs)

```gherkin
Given a link exists
When I call the gRPC `PatchLink` method with a new RSSI
Then the link is updated
And the change is propagated to the relevant chips
```

**Scenario: Delete Link via gRPC**

- **Requirement**: **RQ-FRONT-03**
- **Test**: [`test_link_wiring_grpc`](../daemon/tests/grpc_integration_test.rs)

```gherkin
Given a link exists
When I call the gRPC `DeleteLink` method
Then the link is removed
And the deletion is propagated to the relevant chips
```
