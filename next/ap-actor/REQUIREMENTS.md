# Requirements: Access Point Actor

Ap-Actor - IEEE 802.11 AP, IEEE 802.1X/WPA/WPA2/EAP/RADIUS Authenticator

## 1. Introduction

### 1.1 Purpose

Ap-Actor is a user space microservice for access point and authentication
capabilities.

It implements IEEE 802.11 access point management, IEEE 802.1X/WPA/WPA2/EAP
Authenticators and RADIUS authentication server.

Ap-Actor is designed to be a component of the netsim network simulator, and is
not intended to be run as a standalone daemon.

### 1.2 Scope

The Ap-Actor component handles the logic for:

- IEEE 802.11 Management Frames (Beacons, Probe Responses, Association).
- IEEE 802.1X / EAPOL Authentication handling (WPA2-PSK, WPA3-SAE).
- Dynamic Configuration via `netsim` gRPC API (CRUD requests) and CLI.
- Bridging data frames between the emulated Wi-Fi radio and the virtual network
  (Slirp/Tap).

The following are **out of scope**:

- Physical RF simulation (Signal strength, interference, propagation delay).

- Full RADIUS server backend implementation (Basic EAP-Authenticator is
  provided).

### 1.3 Definitions and Acronyms (Glossary)

| Term      | Definition                                           |
| --------- | ---------------------------------------------------- |
| **AP**    | Access Point                                         |
| **STA**   | Station (Mobile Device / Client)                     |
| **BSSID** | Basic Service Set Identifier (MAC Address of the AP) |
| **SSID**  | Service Set Identifier (Network Name)                |
| **SAE**   | Simultaneous Authentication of Equals (WPA3)         |
| **FTM**   | Fine Timing Measurement (802.11mc)                   |
| **EAPOL** | Extensible Authentication Protocol over LAN          |

## 2. Overall Description

### 2.1 Product Perspective

Ap-Actor is a micro-service within the netsim network simulator. It receives
frames from WiFi-Actor which receives frames from Android Guests in Emulators
via the `mac80211_hwsim` driver. It supports CRUD requests through the netsim
gRPC API. It maintains state for multiple virtual Access Points simultaneously.

### 2.2 User Characteristics

- **App Developers (Emulator Users)**: Android Application developers using
  Android Emulators to run their applications on simulated Android devices with
  Wi-Fi. They typically do not interact directly with the Ap-Actor.
- **Platform & Framework Developers**: Developers (Android Auto, Finder,
  Location, RTT) working on features that require specific, complex Wi-Fi
  environments (e.g., 5GHz, multiple APs, RTT).
- **Test Engineers**: Use `netsim` CLI scripts or Mobly (python) to trigger
  disconnects, change channels, or simulate network failures to verify app
  behavior.˚

### 2.3 User Needs

- **UN-001**: Reliability: APs must stay up during long-running tests.
- **UN-002**: Controllability: Tests must be able to force specific states
  (e.g., deauthentication).
- **UN-003**: Observability: Logs must indicate why an association failed.

### 2.4 Constraints

- Must run in user-space without root privileges specific to hardware drivers.
- Must support cross-platform execution (Linux, macOS, Windows).
- Must process IEEE 802.11 packets received via a stream registered by the
  `WiFi-Actor`.
- Must support lifecycle management (registration) initiated by the
  `WiFi-Actor`.
- Must expose management via internal CRUD methods accessible to the `Ap-Actor`
  Client (enabling gRPC/CLI integration).

### 2.5 Assumptions and Dependencies

- The `netsim` daemon provides a reliable packet transport.
- The guest OS (Android) has a compliant 802.11 stack (wpa_supplicant).

## 3. Standards and References

- [IEEE 802.11-2020](https://ieeexplore.ieee.org/document/9363693): Wireless LAN
  Medium Access Control (MAC) and Physical Layer (PHY) Specifications.
  - Section 9.4.2 (Information Elements)
  - Section 11 (MLME)
  - Section 12 (Security)
- [RFC 3748](https://tools.ietf.org/html/rfc3748): Extensible Authentication
  Protocol (EAP).
- [RFC 7664](https://tools.ietf.org/html/rfc7664): Dragonfly Key Exchange (SAE).
- [RFC 2865](https://tools.ietf.org/html/rfc2865): Remote Authentication Dial In
  User Service (RADIUS).
- **hostapd**:
  - [Man Pages](https://manpages.debian.org/testing/hostapd/hostapd.8.en.html)
  - [AOSP Source (legacy)](https://cs.android.com/android/platform/superproject/+/main:external/wpa_supplicant_8/) -
    Reference implementation for behavior parity.
- [netsim CLI](https://android.googlesource.com/platform/tools/netsim/+/refs/heads/main/rust/cli/README.md)
- [mac80211_hwsim](https://docs.kernel.org/networking/mac80211_hwsim.html)

## 4. Functional Requirements

> [!IMPORTANT] All functional requirements listed below are mandatory (Priority
> P0) and must be implemented.

### 4.1 Management Plane

- **RQ-MGMT-01**: The system shall transmit Beacon frames at a configurable
  interval (default 200 TU).
- **RQ-MGMT-02**: The system shall respond to Probe Requests with Probe
  Responses if the SSID matches or is a wildcard, unless hidden.
- **RQ-MGMT-03**: The system shall handle Association Requests and transmit
  Association Responses with appropriate Status Codes.
- **RQ-MGMT-04**: The system shall support Fine Timing Measurement (FTM)
  Responder functionality to enable WiFi Round Trip Time (RTT) ranging.
- **RQ-MGMT-05**: The system shall process received Deauthentication and
  Disassociation frames by clearing the associated station state and valid
  sessions.

### 4.2 Security

- **RQ-SEC-01**: The system shall support Open System (No Auth).
- **RQ-SEC-02**: The system shall support WPA2-Personal (PSK) using 4-Way
  Handshake.
- **RQ-SEC-03**: The system shall support WPA3-Personal (SAE) using internal
  commit/confirm state machines.
- **RQ-SEC-04**: The system shall support ACLs (Allow/Deny lists) based on MAC
  address.
- **RQ-SEC-05**: The system shall support WPA2-Enterprise (802.1X) with basic
  EAP-Identity/Success flow.

### 4.3 Management Configuration

- **RQ-CONF-01**: Support configurable PHY modes (g, a, ad, ax).
- **RQ-CONF-02**: Support configurable Country Code (injecting Country IE).
- **RQ-CONF-03**: Support WMM (QoS) parameter advertising.
- **RQ-CONF-04**: Support configurable Hidden SSID (suppressing SSID in
  Beacons).
- **RQ-CONF-05**: Support configurable DTIM Period.

### 4.4 Control Interface

- **RQ-CTRL-01**: The system shall accept `netsim` CLI commands to dynamically
  update configuration.
- **RQ-CTRL-02**: Supported commands include: `netsim ap create`,
  `netsim ap update` (for channel/enable/disable), `netsim ap list`.
- **RQ-CTRL-03**: The system shall allow setting the AP position (x, y, z) via
  the `netsim` gRPC API to enable ranging scenarios.
  > **Note**: API definitions are located in `tools/netsim/next/proto`. While
  > AP-specific messages are pending, they will follow the existing CRUD pattern
  > found in other components (e.g., Bluetooth Beacon).
- **RQ-CTRL-04**: The system shall provide functional equivalents for standard
  `hostapd_cli` commands (e.g., `status`, `mib`, `sta`, `all_sta`,
  `deauthenticate`, `disassociate`, `chan_switch`) via the `netsim` API.

### 4.5 Observability

- **RQ-OBS-01**: All transmitted and received 802.11 frames shall be capturable
  via the standard `netsim` PCAP logging mechanism.
- **RQ-OBS-02**: The system shall log significant state transitions
  (Association, Authentication, Disconnection) to standard output/logcat for
  debugging.
- **RQ-OBS-03**: The system shall provide real-time telemetry (connected client
  count, bytes transferred, RSSI) via the `read_statistics` API.

## 5. Non-Functional Requirements

- **RQ-NFR-PERF**: The system shall support at least 16 simultaneous virtual
  Access Points per simulation instance.
- **RQ-NFR-LATENCY**: Handshake processing latency should be under 10ms to avoid
  guest timeouts.

## 6. Verification

Verification is performed via a hierarchical testing strategy:

- **Unit Tests**: Verify packet parsing, crypto primitives (SAE scalars), and
  state machine transitions.
- **Integration Tests**: BDD-style tests (Given/When/Then) for each feature
  verifying full handshake flows (Association -> Auth -> 4-Way) using simulated
  Stations in `tests/`.
- **CLI Tests**: Verify runtime reconfiguration using `netsim` CLI tests.
- **End-to-End Tests**: Manual or automated validation connecting a real Android
  Emulator (AVD) to the Ap-Actor. Verification includes listing connected
  devices, showing their `DeviceId` and MAC Address.

### 6.1 BDD Coverage Mapping

| Feature                             | Test File                               | Requirement                                                       |
| :---------------------------------- | :-------------------------------------- | :---------------------------------------------------------------- |
| **ACLs (Allow/Deny)**               | `acl_test.rs`                           | **RQ-SEC-04**                                                     |
| **FTM/RTT Ranging**                 | `ftm_test.rs`                           | **RQ-MGMT-04**                                                    |
| **WPA3-SAE**                        | `sae_handshake_test.rs`                 | **RQ-SEC-03**                                                     |
| **WPA2-Enterprise**                 | `eap_auth_test.rs`                      | **RQ-SEC-05**                                                     |
| **Hidden SSID**                     | `beacon_test.rs`, `creation_test.rs`    | **RQ-CONF-04**                                                    |
| **Country Code & DTIM**             | `beacon_test.rs`                        | **RQ-CONF-02, RQ-CONF-05**                                        |
| **WMM (QoS)**                       | `beacon_test.rs`                        | **RQ-CONF-03**                                                    |
| **Configurable PHY (ax)**           | `creation_test.rs`                      | **RQ-CONF-01**                                                    |
| **Control Interface (hostapd_cli)** | `ap_control_test.rs`                    | **RQ-CTRL-04** (Partial - covers `chan_switch`, `deauthenticate`) |
| **Basic Mgmt (Assoc, Probe)**       | `association_test.rs`, `beacon_test.rs` | **RQ-MGMT-01, RQ-MGMT-02, RQ-MGMT-03**                            |

### 6.2 BDD Coverage Gaps

| Requirement     | Description                                                   | Current Status                                                                                         |
| :-------------- | :------------------------------------------------------------ | :----------------------------------------------------------------------------------------------------- |
| **RQ-CTRL-04**  | `hostapd_cli` equivalents (`status`, `mib`, `sta`, `all_sta`) | **Partial**: Missing specific tests for status/mib queries.                                            |
| **RQ-OBS-03**   | Real-time Telemetry (`read_statistics`)                       | **Blocked**: `WiFi-Actor` strips `hwsim` metadata (RSSI) before forwarding. Needs architecture change. |
| **RQ-NFR-PERF** | 16 Simultaneous APs                                           | **Missing**: Scale test logic exists but is not running at full capacity (16+).                        |
