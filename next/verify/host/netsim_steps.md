# Netsim Host Steps Reference

This file documents the available host-side steps for controlling the Netsim simulation environment in `verify` tests.

## Mobility

### Move Device
Moves a device to a specific 3D position.

```gherkin
@netsim moves @actor to x, y, z
```
- **actor**: The label of the actor (e.g., `@avd:1`) or the name of a built-in device (e.g., `Beacon1`).
- **x, y, z**: Floating point coordinates in meters.

Example:
```gherkin
@netsim moves @avd:1 to 10.0, 20.0, 0.0
```

## Access Point Management

### Create Access Point
Creates a new Wi-Fi Access Point.

```gherkin
Netsim creates Wi-Fi Access Point "{ssid}" with protocol "{protocol}"
```
- **ssid**: The SSID of the AP.
- **protocol**: The hardware mode (e.g., `a`, `b`, `g`, `n`, `ac`, `ax`).

Example:
```gherkin
Netsim creates Wi-Fi Access Point "Guest-WiFi" with protocol "ax"
```

### Create Secured Access Point
Creates a new Wi-Fi Access Point with WPA password.

```gherkin
Netsim creates Wi-Fi Access Point "{ssid}" with protocol "{protocol}" and password "{password}"
```

Example:
```gherkin
Netsim creates Wi-Fi Access Point "Secure-WiFi" with protocol "ax" and password "12345678"
```

### Remove Access Point
Removes an existing Wi-Fi Access Point by SSID.

```gherkin
Netsim removes Wi-Fi Access Point "{ssid}"
```

Example:
```gherkin
Netsim removes Wi-Fi Access Point "Guest-WiFi"
```

## Beacon Management

### Create Beacon
Creates a new BLE Beacon at a specific position.

```gherkin
Netsim creates BLE Beacon "{name}" at {x}, {y}, {z}
```

Example:
```gherkin
Netsim creates BLE Beacon "MyBeacon" at 5.0, 5.0, 0.0
```

### Create Beacon with Address
Creates a new BLE Beacon with a specific MAC address.

```gherkin
Netsim creates BLE Beacon "{name}" with address "{address}" at {x}, {y}, {z}
```

Example:
```gherkin
Netsim creates BLE Beacon "MyBeacon" with address "11:22:33:44:55:66" at 5.0, 5.0, 0.0
```

### Create Beacon with Tx Power
Creates a new BLE Beacon with a specific Tx Power level.

```gherkin
Netsim creates BLE Beacon "{name}" with Tx Power "{tx_power}" at {x}, {y}, {z}
```
- **tx_power**: One of `UltraLow`, `Low`, `Medium`, `High`.

Example:
```gherkin
Netsim creates BLE Beacon "MyBeacon" with Tx Power "High" at 5.0, 5.0, 0.0
```
## Verification

### Verify Device Position
Asserts that a device is at a specific position in Netsim, optionally within a delta.

```gherkin
Device "{name}" in netsim is at {x}, {y}, {z}[ with delta {delta}]
```
- **delta**: Optional floating point value for precision tolerance. Default is `0.001`.

Example:
```gherkin
Device "MyBeacon" in netsim is at 5.0, 5.0, 0.0 with delta 0.01
```

### Verify Access Point Protocol
Asserts that an AP has a specific protocol.

```gherkin
Wi-Fi Access Point "{ssid}" in netsim has protocol "{protocol}"
```

### Verify Access Point Existence
Asserts that an AP exists in Netsim.

```gherkin
Wi-Fi Access Point "{ssid}" exists in netsim
```

### Verify Netsim Version
Asserts that the Netsim version matches the expected value.

```gherkin
Netsim version is "{version}"
```

Example:
```gherkin
Netsim version is "1.0"
```

### Verify Device Count
Asserts that Netsim has a specific number of devices.

```gherkin
Netsim has {count} devices
```

Example:
```gherkin
Netsim has 3 devices
```
