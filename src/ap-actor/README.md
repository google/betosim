# ApActor

`ap-actor` is a lightweight, native Rust implementation of a WiFi Access Point (AP) for the Netsim ecosystem. It is designed to replace the legacy `hostapd-rs` wrapper, providing a more efficient, single-process architecture for simulating Android WiFi connectivity.

## Overview

The `ApActor` manages the lifecycle of simulated Access Points, handling:

- **802.11 Management**: Beacons, Probe Responses, Association/Authentication.
- **Security**: WPA2-PSK (CCMP), WPA3-SAE (Dragonfly), and WPA-Enterprise (EAP) via native Rust implementations.
- **Data Plane**: Encapsulation/Decapsulation of Ethernet frames to/from 802.11 Data frames.
- **Ranging**: 802.11mc FTM Responder simulation for Location/RTT testing.

## Supported Features (BDD Scenarios)

The following features are verified via Behavior-Driven Development (BDD) integration tests in `tests/`:

### Core Connectivity

- **Lifecycle Management**: Create and Destroy APs dynamically. (`lifecycle_test.rs`)
- **Open Authentication**: Open System Auth and Association. (`association_test.rs`)
- **WPA2-Personal (PSK)**: 4-Way Handshake with CCMP encryption. (`association_test.rs`)
- **Deauthentication**: Correct handling of Deauth frames from stations. (`deauth_test.rs`)

### Advanced Security

- **WPA3-Personal (SAE)**: Simultaneous Authentication of Equals (Dragonfly) handshake, including Commit/Confirm exchanges and P-256 ECC crypto. (`sae_handshake_test.rs`)
- **WPA2-Enterprise (802.1X)**: EAP-MD5 / Mock EAP authentication flow (Identity -> Success). (`eap_auth_test.rs`)
- **MAC Address ACL**: Allow/Deny List enforcement for Authentication/Association frames. (`acl_test.rs`)

### 802.11 Protocols & Extensions

- **802.11mc FTM Ranging**: Fine Timing Measurement Responder, simulating RTT (Round Trip Time) exchanges based on configured `position` or fixed distance. (`ftm_test.rs`)
- **WMM (QoS)**: Wi-Fi Multimedia Parameter Element advertisement (in Beacons/Assoc Resp) for QoS compliance. (`beacon_test.rs`)
- **Regulatory**: Country Code (802.11d) and Power Constraint advertisement. (`beacon_test.rs`)
- **Extended Capabilities**: Advertisement of supported capabilities (FTM, SAE, etc.).

## Feature Parity with `hostapd-rs`

`hostapd-rs` was a valid FFI wrapper around the upstream C `hostapd` daemon. `ap-actor` is a clean-room Rust implementation.

| Feature              | ap-actor (New)              | hostapd-rs (Old)  | Status      |
| :------------------- | :-------------------------- | :---------------- | :---------- |
| **Architecture**     | Native Rust Actor           | C Daemon Wrapper  | ✅ Improved |
| **WPA2-PSK**         | ✅ Native (CCMP)            | ✅ Via Daemon     | ✅ Parity   |
| **Open Auth**        | ✅ Supported                | ✅ Supported      | ✅ Parity   |
| **WPA3 / SAE**       | ✅ Native (SaeStateMachine) | ✅ Supported      | ✅ Parity   |
| **Enterprise (EAP)** | ✅ Mock (Identity->Success) | ✅ Supported      | ✅ Parity   |
| **FTM Ranging**      | ✅ Native (Responder)       | ❌ Manual Config  | ✅ Specific |
| **Key Mgmt**         | ✅ Internal                 | ❌ External (FFI) | ✅ Native   |
| **Regulatory**       | ✅ Supported (Country IE)   | ✅ Full           | ✅ Parity   |
| **QoS / WMM**        | ✅ Native (Basic)           | ✅ Full           | ✅ Parity   |
| **ACL (MAC)**        | ✅ Native (Allow/Deny)      | ✅ Full           | ✅ Parity   |
| **Hidden SSID**      | ✅ Supported                | ✅ Supported      | ✅ Parity   |
| **Client Isolation** | ❌ Not Implemented          | ✅ Supported      | ⚠️ Gap      |
| **WPS**              | ❌ Not Implemented          | ✅ Supported      | ⚠️ Gap      |
| **Legacy WEP**       | ❌ Not Implemented          | ✅ Supported      | ⚪ WontFix  |

### Missing Features Details

1.  **Advanced PHY Parameters**: `hostapd` supports granular control over `rts_threshold`, `fragm_threshold`, etc. `ap-actor` supports `dtim_period` but uses defaults for others.
2.  **Multi-BSSID**: Architecture supports multiple APs, but `SharedKeyStore` is currently optimized for a single BSSID context.
3.  **Client Isolation**: No support for `ap_isolate`.
4.  **WPS**: No Wi-Fi Protected Setup support.
5.  **Legacy WEP**: Intentional omission (deprecated).

## Usage

```rust
use ap_actor::{ApConfig, Position};

let config = ApConfig {
    ssid: "AndroidWifi".to_string(),
    bssid: parse_mac("00:11:22:33:44:55"),
    channel: 6,
    hw_mode: "ax".to_string(),
    wpa_passphrase: Some("password123".to_string()),
    sae: true,                            // WPA3-SAE
    ftm_responder_enabled: true,          // 802.11mc FTM
    beacon_interval: 100,
    country_code: Some("US".to_string()),
    dtim_period: 2,
    position: Position::default(),        // For RTT calculation
    ..Default::default()
};

let ap_id = client.create_ap(config).await?;
```
