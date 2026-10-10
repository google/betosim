# Epic: WiFi Functionality

## Overview
This epic covers the simulation of WiFi capabilities in `netsim`, including
Access Point (AP) behavior, authentication, roaming, and service discovery. The
goal is to provide a realistic WiFi environment for testing Android devices in
simulated scenarios.

These features are closely tied to the Android WiFi APIs, which serve as the
source of truth for expected behaviors:
- [WifiManager](https://developer.android.com/reference/android/net/wifi/WifiManager): Core WiFi management and Soft AP.
- [WifiRttManager](https://developer.android.com/reference/android/net/wifi/rtt/WifiRttManager): RTT ranging capabilities.
- [WifiP2pManager](https://developer.android.com/reference/android/net/wifi/p2p/WifiP2pManager): Peer-to-Peer (Wi-Fi Direct) functionality.
- [WifiAwareManager](https://developer.android.com/reference/android/net/wifi/aware/WifiAwareManager): Neighbor Awareness Networking (NAN).

## Requirements (EARS Notation)

### Core AP Behavior
- **REQ-WIFI-01 (Ubiquitous)**: The system shall simulate at least one WiFi Access Point by default.
- **REQ-WIFI-02 (Event-driven)**: When a device scans for networks, the system shall return the list of available APs based on distance and signal strength.

### Authentication
- **REQ-WIFI-03 (Event-driven)**: When a device attempts to connect to a secure AP, the system shall perform the appropriate authentication handshake (e.g., WPA2/WPA3).
- **REQ-WIFI-04 (Unwanted behavior)**: If authentication fails, then the system shall deny access and report the failure to the device.

### Roaming
- **REQ-WIFI-05 (State-driven)**: While a device is connected to an AP and moving, the system shall continuously update the signal strength.
- **REQ-WIFI-06 (Event-driven)**: When a device detects a stronger AP and initiates roaming, the system shall transfer the connection seamlessly if supported.

### Service Discovery
- **REQ-WIFI-07 (Optional)**: Where WiFi Service Discovery is enabled, the system shall allow devices to discover services advertised by other devices on the same network.

### WiFi Aware (Neighbor Awareness Networking)
- **REQ-WIFI-08 (Optional)**: Where WiFi Aware is supported, the system shall allow devices to discover each other directly without an AP.
- **REQ-WIFI-09 (Event-driven)**: When a device publishes a service over WiFi Aware, the system shall make it discoverable to nearby subscribed devices.
- **REQ-WIFI-10 (State-driven)**: While a WiFi Aware session is active, the system shall maintain the data path between devices as requested.

## System Behaviors & Non-API Features
These are behaviors that are not exposed directly via a specific public API but are critical for realistic simulation in `netsim`:

### Access Point (Soft AP) / Hotspot
- **Tethering Behavior**: Simulating the Android device acting as an AP, routing traffic for connected clients.
- **Client Management**: Handling DHCP assignment and connection limits for simulated clients.

### Advanced Roaming & Network Selection
- **802.11k/v/r Support**: Simulating BSS Transition Management and Fast Roaming if supported by the simulated environment.
- **Firmware-level Roaming Decisions**: Simulating the internal logic that decides when to switch APs based on RSSI and noise.

### Coexistence & Interference
- **BT/WiFi Coexistence**: Simulating performance degradation when both Bluetooth and WiFi are active on the same device or frequency band.
- **Medium Contention**: Simulating CSMA/CA behavior when multiple simulated devices attempt to transmit simultaneously.

## Linked Features (BDD Scenarios)
The following BDD scenarios verify this epic:
- [wifi_ap.feature](../bdd/wifi/WifiManager/wifi_ap.feature)
- [wifi_auth.feature](../bdd/wifi/WifiManager/wifi_auth.feature)
- [wifi_passpoint.feature](../bdd/wifi/WifiManager/wifi_passpoint.feature)
- [wifi_roaming.feature](../bdd/wifi/WifiManager/wifi_roaming.feature)
- [wifi_scanning.feature](../bdd/wifi/WifiManager/wifi_scanning.feature)
- [wifi_soft_ap.feature](../bdd/wifi/WifiManager/wifi_soft_ap.feature)
- [wifi_state.feature](../bdd/wifi/WifiManager/wifi_state.feature)
- [wifi_suggestions.feature](../bdd/wifi/WifiManager/wifi_suggestions.feature)
- [wifi_ui_restricted.feature](../bdd/wifi/WifiManager/wifi_ui_restricted.feature)
- [wifi_aware.feature](../bdd/wifi/WifiAwareManager/wifi_aware.feature)
- [wifi_p2p.feature](../bdd/wifi/WifiP2pManager/wifi_p2p.feature)
- [wifi_service_discovery.feature](../bdd/wifi/NsdManager/wifi_service_discovery.feature)
- [wifi_ranging.feature](../bdd/wifi/WifiRttManager/wifi_ranging.feature)
