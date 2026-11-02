# Netsim (AOSP)

**Netsim** is a network simulation tool written in Rust, designed for multi-device and multi-radio use cases. It allows developers to run, visualize, test, debug, and analyze scenarios across virtual devices (such as Android Virtual Devices and Cuttlefish) and host applications.

Netsim ships as the **default network backplane** for both **Cuttlefish** and the **Android Emulator (Goldfish)**, routing and simulating wireless chips (Bluetooth, Wi-Fi, and UWB) hermetically. The primary user interface to interact with and control the running simulation is the **`netsim` CLI application**.

## Core Capabilities
*   **Default Emulator Backplane**: Integrates out-of-the-box with Goldfish and Cuttlefish.
*   **Packet Capture**: Supports real-time HCI packet capture (generating PCAP files) for debugging and analysis (compatible with Wireshark).
*   **Link Parameter Control**: Allows dynamic control of radio link parameters (such as distance, path loss, and attenuation) to simulate realistic or edge-case network conditions.
*   **Multi-Radio Support**: Simulates Bluetooth (BLE/Classic), Wi-Fi, and Ultra-Wideband (UWB) radios.
*   **User-Mode Wi-Fi Networking (`libslirp`)**: Integrates `libslirp` to provide user-mode NAT, DHCP, and DNS routing for virtual Wi-Fi chips. This allows emulators (such as Goldfish) to grant virtual devices internet connectivity hermetically without requiring root privileges or host TAP interfaces.

## Deep-Dive: Radio Emulation Features

Netsim leverages specialized backends to emulate wireless protocols with high fidelity:

### Bluetooth Emulation (Rootcanal Backend)
*   **Virtual Controller (HCI)**: Simulates standard Host Controller Interface commands and events.
*   **Link Layer Simulation**: Models packet exchange between virtual controllers (advertising, scanning, connections).
*   **Physics/Attenuator Hook**: Supports callbacks to simulate signal attenuation, path loss, and packet drops based on simulated distance.
*   **HCI Snoop Logging**: Outputs standard Bluetooth HCI btsnoop logs for deep analysis (viewable in Wireshark).

### Wi-Fi Emulation & Access Point Simulation
*   **Access Point (AP) Simulation**: Simulates a software-defined Access Point inside Netsim.
*   **Security (WPA2/WPA3)**: Emulates EAP, RSN (WPA2), and SAE (WPA3 Personal) handshakes hermetically.
*   **User-Mode Networking (`libslirp`)**: Connects virtual Wi-Fi chips to the host's network via user-mode NAT/DHCP (no root or host TAP interfaces required).
*   **Wi-Fi Ranging (RTT/FTM)**: Supports Fine Time Measurement (802.11mc) simulation to allow virtual devices to perform Wi-Fi distance estimation.

### Ultra-Wideband (UWB) Emulation (Pica Backend)
*   **UCI Protocol Handling**: Handles UWB Command Interface packets.
*   **Two-Way Ranging**: Calculates range, azimuth (AoA in horizontal plane), and elevation (AoA in vertical plane) between simulated devices.
*   **Peer AoA Estimation**: Simulates the angle of arrival from the perspective of both devices.
*   **Virtual Anchors**: Supports creating static, positionable UWB anchors in the virtual environment.
*   **UWB PCAPNG Logging**: Captures all UCI control and data traffic to standard PCAPNG files.

## Useful CLI Commands

Once built, you can run the `netsim` CLI to interact with the running daemon. Here are some interesting commands:

### Packet Capture & Sniffing
*   **HCI Capture (`netsim capture` / `netsim pcap`)**: Control and download packet captures.
    *   `netsim capture list`: List active capture sources.
    *   `netsim capture patch --id <id> --state [on|off]`: Toggle capture for a source.
    *   `netsim capture get --id <id>`: Download the PCAP/Snoop log.
*   **BLE Sniffer (`netsim ble sniff`)**: Start a baseband packet sniffer for BLE.

### Simulating Devices & Links
*   **BLE Beacon (`netsim beacon`)**: Manage virtual BLE beacons.
    *   `netsim beacon create`: Create a new beacon sending advertisements.
*   **Link Control (`netsim link`)**: Manage radio link properties dynamically.
    *   `netsim link list`: List current links.
    *   `netsim link patch --range <meters>`: Adjust simulated distance/attenuation.

### Other Commands
*   **Devices (`netsim devices`)**: List all devices currently connected to the simulation.

## Building & Testing in AOSP

In AOSP, Netsim is built using the standard Android build system (Soong) or Bazel when building the platform.

To build using Soong:
```bash
m netsim netsimd
```

To run tests:
```bash
# Run Rust unit tests
atest netsim_unit_tests
```

## Formatting & Style

This project uses `pre-commit` to ensure code style consistency. To set it up:
1. Install `pre-commit` (e.g. `pipx install pre-commit` or `brew install pre-commit`).
2. Run `pre-commit install` in the repository root.
