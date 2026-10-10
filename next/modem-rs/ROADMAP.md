# Modem Simulator Roadmap

The Rust multi-device modem simulator is a replacement for the existing C++
[Unisoc library](#source-implementations).

The Unisoc simulator uses a process per emulated device. Testing multi-device
interactions like calls or SMS requires manual console commands or complex TCP
configuration to bridge these isolated processes.

In contrast, the new Rust implementation runs as a single service within Netsim
that manages multiple modem instances. This architecture enables sophisticated,
zero-configuration network-level interactions between simulated modem devices,
treating them as peers on a shared virtual telephony network.

## Scenarios

### Primary Scenarios

These scenarios are part of the initial release of the multi-device modem
simulator.

- **Device-to-Device (D2D) Call Signaling** [IMPLEMENTED]:
  - **Direct Dialing**: Emulator instances can dial each other using their
    simulated phone numbers.
  - **State Synchronization**: Ringing(`RING`), Answering(`ATA`), and
    Hangup(`ATH`) events are synchronized between the caller and callee.
- **Device-to-Device SMS** [IMPLEMENTED]:
  - **Direct Delivery**: SMS messages sent via `AT+CMGS` are instantly routed to
    the target instance and delivered as `+CMT` notifications.
- **Conference Calling** [IMPLEMENTED]: Support for multi-party voice calls
  involving more than two AVDs.
- **Simultaneous Services** [PLANNED]: Handling active voice calls concurrent
  with active data sessions.

### Secondary Scenarios

These scenarios represent advanced simulation capabilities that will be
implemented after the core required scenarios are stable.

- **Realistic Radio Layer & Network Simulation**:
  - **Signal Modeling**: Simulation of PHY/MAC layer aspects including signal
    strength, interference, path loss, and noise.
  - **Network Diversity**: Simulating characteristics of different network
    standards (GSM, LTE, 5G NR).
  - **Network Congestion**: Modeling resource contention and degraded
    performance with multiple active devices.
  - **Dynamic Environment**: API for real-time control of signal strength,
    registration status, and operator availability.

- **Data Plane & LAN Bridging**:
  - **LAN Gateway**: Forwarding data packets to the host network (simulating a
    gateway to the internet/LAN).
  - **Packet Routing**: Proper routing of IP packets between the simulated modem
    and the Netsim environment.

- **Mobility & Dynamic Conditions**:
  - **Handovers**: Simulating device movement and handovers between cells within
    a managed topology.
  - **Coverage Transitions**: Simulating devices entering/leaving coverage or
    transitioning into weak signal zones.

- **Advanced Telephony Features**:
  - **Group Messaging**: Simulation of Group SMS and MMS exchanges across
    multiple devices.
  - **RCS Simulation**: Support for Rich Communication Services (chat, file
    transfer).
  - **Emergency Services**: Simulation of E911 calls with location reporting.
  - **Carrier-Specific Behaviors**: Modeling specific network restrictions,
    dynamic profile loading, and SIM lock states.
  - **OTA Updates**: Simulation of Over-the-Air updates to test diverse carrier
    scenarios.
  - **Complex Scenario Generation**: Scriptable test scenarios for multi-call
    flows, SMS/MMS failure injection, and data stall recovery.

- **Observability & Debugging**:
  - **State Reporting**: Structured query interface for real-time insight into
    internal modem state.
  - **Event Bus**: Pub/Sub mechanism for tracking internal state changes for
    validation.

- **Hardware Integration**:
  - **AT Command Proxy**: Bridging commands from Android guest to physical
    hardware modems for Hardware-in-the-Loop (HIL) testing.

## User Feature Requests

The following features have been identified from user feedback and requirements
docs (e.g., Google Fi, Pixel) but are not yet scheduled in specific phases.

- **Multiple SIM / Dual SIM Support**:
  - Requirement: Support for testing dual SIM scenarios (DSDS) and multiple
    active subscriptions.
  - Source: Google Fi Team.
- **eSIM & eUICC Profile Lifecycle Management (GSMA SGP.22)**:
  - Requirement: Support GSMA SGP.22 eSIM profile lifecycle operations (download,
    activation, deactivation, deletion, and dynamic UICC filesystem swapping) to
    support Android LPA and CTS testing.
  - Source: Android Telephony / LPA & Emulator Teams.
- **IPv6 Support**:
  - Requirement: Proper IPv6 address allocation (SLAAC/DHCPv6) for mobile data
    to support modern network requirements.
  - Source: Pixel / Core Networking.
- **Advanced Network Simulation**:
  - Requirement: Ability to simulate packet loss, variable latency, and dynamic
    signal strength changes via Netsim API.
  - Source: Integration Teams.
- **Extended PDP Context Support**:
  - Requirement: Reliable support for at least 3 concurrent PDP contexts with
    independent network interfaces.
  - Source: Pixel Team.

## Completed Milestones

- **Architectural Refactoring & Unification**: Completed.
- **Event Loop & Timers**: Implemented.
- **Initial Test Coverage**: Expanded to over 100 passing tests across all
  services (Call, Data, Network, SIM, SMS, STK, Sup).

## Android Emulator Console Support

Required features to support the standard `emulator -console` telephony
commands. These commands allow users (and Android Studio) to inject events into
the modem.

- [x] **Incoming Call (`gsm call`)**:
  - [x] Implement logic to trigger an incoming call (`RING`, `+CLIP`) from an
        external event.
- [x] **Remote Answer (`gsm accept`)**:
  - [x] Implement logic to simulate remote party answering (Active).
- [x] **Remote Hold (`gsm hold`)**:
  - [x] Implement logic to simulate remote party calling hold/resume.
- [x] **Call Termination (`gsm cancel`)**:
  - [x] Implement logic to terminate a call from the network side (transitions
        to `SimState::Ready` w/ `NO CARRIER` etc).
- [x] **Data/Voice State (`gsm data`, `gsm voice`)**:
  - [x] Allow external control of `+CREG` and `+CGREG` states (Roaming,
        Searching, Denied, etc.).
  - [ ] Support "Metered" network status if applicable.
- [x] **Signal Strength (`gsm signal`)**:
  - [x] Allow external updates to signal strength (RSSI) and BER.
- [x] **Network Time (`updateClock`)**:
  - [x] Support external injection of Network Identity and Time Zone (NITZ)
        updates.
- [x] **Incoming SMS (`sms send`, `sms pdu`)**:
  - [x] Implement injection of text-mode SMS (`+CMT`).
  - [x] Implement injection of PDU-mode SMS (`+CMT`).

## Cuttlefish Drop-in Compatibility

Ensure the binary functions as a direct drop-in replacement for the legacy modem
simulator used by Cuttlefish (and eventually Netsim). Focus on simplifying the
execution model to a single binary where possible, ensuring strictly compatible
CLI arguments and FD handling.

- [x] **Verify CLI Compatibility**: Ensure arguments like `--server-fds` match
      legacy behavior.
- [ ] **Simplify Execution Model**: Evaluate removing the "Daemon/Proxy" split
      in favor of a monolithic run loop for Cuttlefish.
- [ ] **Integration Test**: Verify the binary runs correctly within a Cuttlefish
      instance.

## Feature Parity with C++ Implementation

Based on a detailed comparison of the C++ and Rust source code, the following
features are missing from the Rust implementation. This plan outlines the work
required to achieve feature parity.

### Call Service (`src/call_service.rs`)

**Context:** The Rust `call_service.rs` is a good starting point, but it is
missing several key features and the detailed logic that is present in the C++
`call_service.cpp`.

**Tasks:**

- [ ] **Implement full `ATD` command handling:**
  - [ ] Add support for emergency number dialing with categories and CLIR
        (`ATDnumber@[category],#[clir];`); syntactic parsing is implemented.
  - [x] Integrate with `SimService` to perform FDN (Fixed Dialing Number)
        checks.
- [x] **Expand `AT+CHLD` command handling:**
  - [x] Implement all modes (0, 1, 2, 3, 4) of the `AT+CHLD` command.
- [ ] **Improve error handling:**
  - [ ] Use specific CME (Cellular Messaging Entity) error codes.
- [ ] **Enhance state management:**
  - [ ] Add `is_international`, `can_present_number`, and `timeout_serial`
        fields to the `CallStatus` struct.
- [ ] **Clarify dependencies:**
  - [ ] Make the dependencies on `SimService` and `NetworkService` more
        explicit.

### Data Service (`src/data_service.rs`)

**Context:** The Rust `data_service.rs` is a mix of being more and less
feature-complete than the C++ `data_service.cpp`. The Rust version has better
support for QoS commands, but it is missing the detailed logic for handling PDP
contexts, data call activation, and physical channel configurations.

**Tasks:**

- [x] **Implement `AT+CGACT?` (Query Data Call List):**
  - [x] Add a function to return a list of active PDP contexts.
- [ ] **Enhance `AT+CGDCONT` (Define PDP Context):**
  - [ ] Modify the `handle_define_pdp_context` function to get the IP address,
        DNS servers, and gateways from a configuration source.
- [x] **Improve `AT+CGDATA` (Enter Data State):**
  - [x] Add a check to the `handle_enter_data_state` function to ensure the
        specified PDP context is active.
- [x] **Implement `AT+CGCONTRDP` (Read Dynamic Parameters):**
  - [x] Modify the `handle_read_dynamic_param` function to return the correct
        values for the specified PDP context.
- [ ] **Add Physical Channel Configuration:**
  - [ ] Implement the logic for updating and sending physical channel
        configuration updates (`%CGFPCCFG`).

### Misc Service (`src/misc_service.rs`)

**Context:** The Rust `misc_service.rs` is significantly less feature-complete
than the C++ `misc_service.cpp`. The C++ version has much more advanced logic
for handling time, time zones, and initialization commands.

**Tasks:**

- [x] **Implement full `AT+CGSN` command handling:**
  - [x] Update `parser.rs` to support optional parameters.
  - [x] Add support for the `snt` parameter to return different types of
        identification information (IMEI, SVN, etc.).
- [x] **Add time and time zone support:**
  - [x] Implement logic for parsing the time zone from the system.
  - [x] Implement logic for calculating the time zone offset.
  - [x] Implement the `%CTZV` unsolicited response for time updates.
- [x] **Add initialization commands:**
  - [x] Implement the missing initialization commands (e.g., `E0Q0V1`, `S0=0`,
        `+CMEE=1`).

### Network Service (`src/network_service.rs`)

**Context:** The Rust `network_service.rs` is a skeleton of a service and needs
a massive amount of work to reach feature parity with the C++
`network_service.cpp`.

**Tasks:**

- [x] **Implement Radio Power Management (`AT+CFUN`):**
  - [x] Implement `AT+CFUN` command in `parser.rs`.
  - [x] Add support for setting and querying the radio power state.
- [x] **Implement Network Selection (`AT+COPS`):**
  - [x] Add support for querying and setting the network selection mode.
  - [x] Add support for querying available networks.
  - [x] Add support for requesting the current operator.
- [x] **Implement Network Registration (`AT+CREG`, `AT+CGREG`, `AT+CEREG`):**
  - [x] Implement `AT+CREG`, `AT+CGREG`, `AT+CEREG` set/query commands in
        `parser.rs`.
  - [x] Add support for voice and data network registration.
  - [x] Implement unsolicited registration status updates.
- [ ] **Implement Signal Strength (`AT+CSQ`):**
  - [x] Implement the Cuttlefish-specific `+CSQ` command with a detailed
        `AidlSignalStrength` struct.
  - [ ] Implement a loop to continuously update the signal strength.
- [x] **Implement Preferred Network Type (`AT+CTEC`):**
  - [x] Add support for getting and setting the preferred network type.
- [ ] **Integrate with NVRAM Configuration:**
  - [ ] Use a configuration management system to store and retrieve
        network-related settings.
- [x] **Integrate with `SimService`:**
  - [x] Use the `SimService` to initialize the network operator.

### SIM Service (`src/sim_service.rs`)

**Context:** The Rust `sim_service.rs` is a very basic implementation that is
missing most of the features and complexity of the C++ `sim_service.cpp`.

**Tasks:**

- [x] **Implement a robust SIM File System:**
  - [x] Create a hierarchical file system model (MF, DF, EF).
  - [x] Implement a parser for the XML-based SIM profile.
  - [x] Implement full `AT+CRSM` command handling (fallback mappings and clean response formatting).
  - [x] Implement full `AT+CSIM` command handling.
- [x] **Implement full PIN/PUK Management (`AT+CPIN`):**
  - [x] Handle all SIM states (ABSENT, NOT_READY, READY, PIN, PUK).
  - [x] Handle all PIN/PUK operations correctly.
- [x] **Implement Facility Lock (`AT+CLCK`):**
  - [x] Add support for locking, unlocking, and querying all facilities.
- [x] **Enhance Logical Channel Support (`AT+CCHO`, `AT+CCHC`, `AT+CGLA`)**: Implemented basic open/close channel lifecycle and transmit APDU mocking.
  - [x] Add support for Application Identifiers (AIDs).
- [ ] **eSIM & eUICC Support (GSMA SGP.22 & GlobalPlatform Card Spec)**:
  - [ ] **Phase 1: ISD-R Logical Channel & Basic SGP.22 ES10**:
    - [ ] Add `STORE DATA` (`0xE2`) APDU instruction handling and block chaining.
    - [ ] Add zero-dependency ASN.1 BER-TLV decoder and encoder for SGP.22 tags.
    - [ ] Auto-register standard GSMA ISD-R AID (`A0000005591010FFFFFFFF8900000100`) on `AT+CCHO`.
    - [ ] Implement basic ES10 info queries: `GetEUICCInfo1` (`0xBF20`), `GetEUICCInfo2` (`0xBF22`), `GetEUICCChallenge` (`0xBF2E`), and `GetConfiguredAddresses` (`0xBF3C`).
  - [ ] **Phase 2: Profile Storage & SGP.22 Profile Lifecycle Management**:
    - [ ] Implement multi-profile `ProfileStore` holding `EsimProfile` structures with embedded UICC filesystems.
    - [ ] Support profile lifecycle commands: `GetProfilesInfo` (`0xBF2D`), `EnableProfile` (`0xBF31`), `DisableProfile` (`0xBF32`), `DeleteProfile` (`0xBF33`), `SetNickname` (`0xBF29`), `ResetMemory` (`0xBF34`).
    - [x] Implement dynamic active `FileSystem` swapping and SIM Refresh notifications (`+CUSATP: 01` / reset URC).
  - [ ] **Phase 3: eSIM In-Band BPP & Out-of-Band Provisioning**:
    - [ ] Handle `AuthenticateServer` (`0xBF38`) and `PrepareDownload` (`0xBF21`).
    - [ ] Parse in-band Bound Profile Packages via `LoadBoundProfilePackage` (`0xBF36` / `0xBF37`).
    - [ ] Provide out-of-band programmatic API / CLI fixtures for injecting test eSIM profiles.
  - [ ] **Phase 4: Goldfish HAL Alignment & CTS Verification**:
    - [ ] Update Goldfish `RadioConfig::getSimTypeInfo` to report `SimType::ESIM` when EID is present.
    - [ ] Validate end-to-end with Android LPA UI and CTS test suites (`CtsCarrierApiTestCases`, `CtsTelephonyTestCases`).
- [x] **Implement CDMA Features (`AT+CCSS`, `AT+WRMP`):**
  - [x] Implement the CDMA-specific commands.
- [x] **Implement SIM Authentication (`^MBAU`):**
  - [x] Implement the `^MBAU` command.
- [x] **Implement Phone Number Management:**
  - [x] Implement functions for getting and setting the phone number from the
        SIM file system.
- [x] **Implement FDN (Fixed Dialing Number):**
  - [x] Implement the FDN check.

### SMS Service (`src/sms_service.rs`)

**Context:** The Rust `sms_service.rs` and C++ `sms_service.cpp` have different
strengths and weaknesses. The focus should be on implementing the missing
features in the Rust version.

**Tasks:**

- [x] **Implement PDU Parsing**:
  - [x] Add a PDU parser to handle SMS messages in PDU mode.
- [x] **Implement SMS Status Reports:**
  - [x] Add the logic for generating and sending SMS status reports.
- [x] **Improve Error Handling:**
  - [x] Use more specific CMS (Cellular Messaging Service) error codes.

### STK Service (`src/stk_service.rs`)

**Context:** The Rust `stk_service.rs` is a skeleton of a service and needs a
complete rewrite to achieve feature parity with the C++ `stk_service.cpp`.

**Tasks:**

- [x] **Implement an STK Profile Parser:**
  - [x] Create a parser for the XML-based STK profile.
- [ ] **Implement Proactive Command Handling:**
  - [ ] Handle all proactive commands defined in the STK profile (SETUP MENU,
        SELECT ITEM, and DISPLAY TEXT are implemented).
- [x] **Implement Terminal Response Handling (`AT+CUSATT`):**
  - [x] Process terminal responses and trigger the correct unsolicited commands.
- [x] **Implement Menu Navigation:**
  - [x] Create a state management system for navigating the STK menu.
- [ ] **Integrate with `SimService`:**
  - [ ] Use the `SimService` to access the ICC profile (SIM card presence check
        is implemented).

### Supplementary Service (`src/sup_service.rs`)

**Context:** The Rust `sup_service.rs` is a very basic implementation that is
missing most of the features and complexity of the C++ `sup_service.cpp`.

**Tasks:**

- [ ] **Implement full USSD handling (`AT+CUSD`):**
  - [ ] Add logic for managing USSD sessions.
  - [ ] Implement the `AT+CUSD=` command for canceling USSD sessions
        (`AT+CUSD=2`).
- [x] **Implement full CLIR handling (`AT+CLIR`):**
  - [x] Add the `ClirStatusInfo` struct and logic for setting and querying the
        CLIR status.
- [x] **Implement full CLIP handling (`AT+CLIP`):**
  - [x] Add logic for setting and querying the CLIP status.
- [x] **Implement full Call Waiting handling (`AT+CCWA`):**
  - [x] Add the `CallWaitingInfo` struct and logic for setting and querying the
        call waiting status.
- [x] **Enhance Call Forwarding handling (`AT+CCFC`, `AT+CCFCU`):**
  - [x] Full compliance with 3GPP TS 27.007 §7.11 for AT+CCFC and §7.35 for AT+CCFCU.
  - [x] Distinguish query responses (`+CCFC:` with 27.007 field order vs `+CCFCU:`).
  - [x] `CallForwardTon` dual-encoding support (TS 24.008 TON 0..=7 vs address octet 128..=255), echoed verbatim for `+CCFCU` number destinations and rendered as an address octet for `+CCFC`; URI destinations render `<ton>` as 0.
  - [x] §7.35 cross-field validation rejecting `<numbertype>=0` with a non-empty number and `<numbertype>=1` with a non-zero TON (`+CME ERROR: 50`).
  - [x] `CallForwardNumberType` 3-variant enum (0=NoValidInfo, 1=Uri, 2=Number) with default Number.
  - [x] Atomic registration validation requiring non-empty number for fresh registrations.
  - [x] Update number, ton, and numbertype on Enable and Disable when a non-empty number is supplied.
  - [x] Bounded `CallForwardTime` (1..=30) validation.
  - [x] Slot-aligned `Option<T>` parsing in `modem-rs-derive`.
- [ ] **Implement Supplementary Service Notifications (`AT+CSSN`):**
  - [ ] Add logic for handling supplementary service notifications.

## Async Architecture

Modernize the codebase to use Tokio and async/await patterns for integration
with Netsim Next.

- [x] **Async Channels**: Replace legacy callbacks with `tokio::sync::mpsc`.
- [x] **Cell Actor Refactor**: Remove `Mutex` and `unsafe` blocks, align with
      actor model.
- [ ] **Async Native**: Convert `tick()` based loops to `await` driven event
      loops where appropriate.

## References & Source Material

The development of this modem simulator is guided by the following standards and
reference implementations.

### Standards

- **3GPP TS 27.007**: _AT command set for User Equipment (UE)_.
  - Primary reference for general modem control, network service, and call
    handling commands.
- **3GPP TS 27.005**: _Use of Data Terminal Equipment - Data Circuit terminating
  Equipment (DTE - DCE) interface for Short Message Service (SMS) and Cell
  Broadcast Service (CBS)_.
  - Reference for all SMS-related commands (`AT+CMGS`, `AT+CMGR`, etc.).
- **3GPP2 C.S0023-D**: _Removable User Identity Module for Spread Spectrum
  Systems_.
  - Reference for CDMA-specific commands (`AT+CCSS`, `AT+WRMP`).
- **3GPP TS 31.111 / 11.14**: _USIM Application Toolkit (USAT)_.
  - Reference for STK (SIM Toolkit) commands and envelope structures.
- **GSMA SGP.22**: _RSP (Remote SIM Provisioning) Technical Specification v2.2+_.
  - Reference for ES10 interface (LPA to eUICC), ISD-R AID, ASN.1 BER-TLV definitions, and profile lifecycle procedures.
- **GlobalPlatform Card Specification v2.3**: _Card Specification & Amendment D (Secure Channel Protocol)_.
  - Reference for `STORE DATA` APDU command structure and logical channel management.

### Source Implementations

- **Unisoc Modem Simulator (C++)**:
  - Location: AOSP `device/google/cuttlefish/host/commands/modem_simulator`.
  - Origin: Attributable to a contribution by **Unisoc** circa 2020.
  - Shared Usage: Also wrapped by Goldfish in
    `external/qemu/android/third_party/modem-simulator`.
  - Status: The legacy implementation against which feature parity is being
    measured (Phase 16).
  - Capabilities: Included a bespoke TCP protocol (`AT+REMOTECALL`) for ad-hoc
    device-to-device communication, requiring manual port configuration.
