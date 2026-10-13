# Requirements: WiFi Actor

WiFi-Actor - `mac80211_hwsim` Medium Simulator, Station Switch, and L2/L3 Gateway Router

## 1. Introduction

### 1.1 Purpose

WiFi-Actor is a user-space microservice in the `netsim` network simulator
responsible for orchestrating IEEE 802.11 wireless medium simulation and packet
routing for virtual Wi-Fi radios (`ChipKind::WIFI`).

It acts as the:

- **`mac80211_hwsim` Transport Endpoint**: Decodes incoming Generic Netlink
  `HwsimMsg` frames from guest Android Virtual Devices (AVDs), returns
  immediate `HWSIM_CMD_TX_INFO_FRAME` transmission acknowledgments, and
  encapsulates outbound IEEE 802.11 frames into `HwsimMsg` deliveries.
- **Wireless Medium (`Medium`)**: Tracks virtual stations (`Station`) by 802.11
  MAC address and hardware `hwsim` transmitter address, manages radio enablement
  state, decrypts/encrypts frames using session keys shared with `ap-actor`
  (`SharedKeyStore`), and routes frames across peer stations, virtual Access
  Points, and external gateways.
- **Infrastructure Gateway Bridge**: Converts infrastructure (`ToDS` / `FromDS`)
  IEEE 802.11 data frames to and from IEEE 802.3 Ethernet frames via a
  pluggable `GatewayTrait` backend (`SlirpGateway` by default, or `TapGateway`
  on Linux when `--wifi-tap` is configured).
- **Ranging & Discovery Offload**: Intercepts `HWSIM_CMD_START_PMSR` Peer
  Measurement requests and IEEE 802.11 Public Action Fine Timing Measurement
  (FTM) requests to synthesize distance-based Round-Trip Time (RTT) responses,
  and optionally reflects host IPv4 mDNS multicast traffic into the simulated
  medium.

### 1.2 Scope

The WiFi-Actor component handles the logic for:

- **Chip Lifecycle Management**: Creating, reading, updating (including radio
  enable/disable), resetting, listing, and deleting Wi-Fi chips via `WifiClient`
  (`ChipClient`).
- **`mac80211_hwsim` Framing & TX Acknowledgments**: Parsing Netlink attributes
  (`HWSIM_ATTR_FRAME`, `HWSIM_ATTR_ADDR_TRANSMITTER`, `HWSIM_ATTR_FREQ`,
  `HWSIM_ATTR_TX_INFO`, `HWSIM_ATTR_SIGNAL`), building TX status notifications,
  and wrapping delivered frames with receiver address, frequency, and signal
  attributes.
- **Station Table & Medium Switching**: Learning station MAC-to-chip mappings
  (including dual mapping of randomized 802.11 SA and hardware `hwsim` MAC),
  unicast/multicast/broadcast station-to-station delivery (Wi-Fi Direct / P2P
  and Wi-Fi Aware / NAN), AP reflection (`ToDS` to `FromDS`), and multicast-to-unicast
  (M2U) conversion for infrastructure mDNS traffic.
- **WPA2/WPA3 Data-Plane Crypto Integration**: Decrypting incoming protected
  frames and re-encrypting outgoing infrastructure frames per recipient station
  using PTK/GTK keys stored in `ap_actor::SharedKeyStore`.
- **Infrastructure Gateways**: Bridging `ToDS`/`FromDS` data frames to
  `slirp-actor` (`SlirpGateway`) or Linux `/dev/net/tun` (`TapGateway` in
  single-interface or range-pool mode).
- **Host mDNS Forwarder**: Joining IPv4 multicast group `224.0.0.251:5353` on
  the host and injecting wrapped Ethernet/802.11 mDNS frames into the wireless
  medium when `forward_host_mdns` is enabled.
- **Wi-Fi RTT (FTM & PMSR)**: Computing Time-of-Flight (ToF) and RTT from 3D
  device positions (`netsim_model::Position`) in `ftm.rs` and `pmsr.rs`.
- **Telemetry & Statistics**: Recording per-chip TX/RX and P2P frame counters,
  global `WifiIpcStats` counters, upload/download throughput Mbps, and
  protocol-level `WifiApi` action frame counters (NAN, P2P, RTT, Easy Connect /
  DPP).

The following are **out of scope**:

- **IEEE 802.11 AP Management & WPA Handshakes**: Beacon generation, Probe
  Responses, Association, and EAPOL/SAE key negotiation are owned by
  `ap-actor` (see `next/ap-actor/REQUIREMENTS.md`).
- **User-Space TCP/IP Stack & NAT/DHCP**: L3/L4 protocol processing, DHCP
  leases, DNS proxying, and NAT are owned by `slirp-actor` and `slirp`.
- **RF Physics & Channel Interference**: Detailed multipath fading, CSMA/CA
  backoff contention, and co-channel interference modeling.

### 1.3 Definitions and Acronyms (Glossary)

| Term | Definition |
| :--- | :--- |
| **`mac80211_hwsim`** | Linux kernel driver for simulating IEEE 802.11 radios over Generic Netlink. |
| **STA** | Station (emulated guest Wi-Fi client interface mapped to a `ChipId`). |
| **AP** | Access Point managed by `ap-actor`. |
| **BSSID** | Basic Service Set Identifier (MAC address of an Access Point). |
| **ToDS / FromDS** | IEEE 802.11 Frame Control direction bits (Station-to-AP vs. AP-to-Station). |
| **M2U** | Multicast-to-Unicast conversion on infrastructure links. |
| **FTM** | Fine Timing Measurement (IEEE 802.11mc / 802.11-2020 RTT ranging protocol). |
| **PMSR** | Peer Measurement (`NL80211_CMD_PEER_MEASUREMENT_*` / `HWSIM_CMD_START_PMSR`). |
| **NAN** | Neighbor Awareness Networking (Wi-Fi Aware). |
| **P2P** | Wi-Fi Direct Peer-to-Peer networking. |
| **DPP** | Device Provisioning Protocol (Wi-Fi Easy Connect). |
| **PTK / GTK** | Pairwise Transient Key (unicast) / Group Temporal Key (multicast/broadcast). |

## 2. Overall Description

### 2.1 Product Perspective

WiFi-Actor sits at the center of the Wi-Fi simulation subsystem inside `netsim`:

- **Upstream (`device-actor`)**: `DeviceActor` creates and controls Wi-Fi
  chips through `WifiClient` (implementing `netsim_model::ChipClient`) and
  receives `NotifyChipRemoved` callbacks when a chip is deleted or its guest
  packet transport closes.
- **Guest Transport (`PacketStream` / `PacketSink`)**: Connected AVDs exchange
  `HwsimMsg` packets with `WifiActor` over per-chip streams registered on
  creation.
- **Peer Control Plane (`ap-actor`)**: `WifiActor` registers a bidirectional
  frame channel with `ApClient` on startup (`AP_SUBSCRIPTION_ID`) and shares an
  `Arc<SharedKeyStore>` so `Medium` can validate active BSSIDs, map stations to
  associated BSSIDs, and decrypt/encrypt WPA2/WPA3 data frames.
- **Downstream Network Stack (`slirp-actor` or Linux TAP)**: Infrastructure data
  frames are converted to IEEE 802.3 Ethernet and forwarded through
  `GatewayTrait` to `SlirpGateway` (registered under `SLIRP_ID`) or
  `TapGateway`.

### 2.2 User Characteristics

- **App & System Developers (Emulator Users)**: Run Android applications and
  system services (`WifiManager`, `WifiP2pManager`, `WifiAwareManager`,
  `WifiRttManager`, `NsdManager`) on Cuttlefish or Android Emulator virtual
  devices expecting transparent internet access, multi-device discovery, and
  ranging.
- **Test Engineers**: Write automated integration and Mobly/BDD tests that
  toggle Wi-Fi chip state, move devices in 3D space, or inspect radio
  statistics via the `netsim` CLI, gRPC, or WebSocket APIs.

### 2.3 User Needs

- **UN-001 (Connectivity)**: Emulated Android devices must seamlessly connect
  to virtual APs and exchange IPv4/IPv6 traffic with the external network or
  peer devices.
- **UN-002 (Multi-Device Direct Discovery)**: Multiple AVDs must discover and
  communicate with each other via Wi-Fi Direct (P2P), Wi-Fi Aware (NAN), and
  mDNS service discovery.
- **UN-003 (Spatial Ranging)**: Moving devices in `netsim` must produce
  deterministic, distance-accurate RTT ranging results over both FTM Action
  frames and `mac80211_hwsim` PMSR commands.
- **UN-004 (Controllability & Observability)**: Tests must be able to enable,
  disable, or reset individual Wi-Fi chips and query detailed frame, throughput,
  and Android Wi-Fi API telemetry.

### 2.4 Constraints

- Must execute asynchronously within `actor-framework` without blocking the
  actor event loop on guest sink backpressure or network I/O.
- `TapGateway` uses Linux-specific `/dev/net/tun` ioctls (`TUNSETIFF`) and
  `AsyncFd`; non-Linux targets (`macos`, `windows`) must compile cleanly and
  fall back to `SlirpGateway`.
- Must handle MAC address randomization and `mac80211_hwsim` transmitter
  addresses where the 802.11 source MAC (`Addr2`) differs from
  `HWSIM_ATTR_ADDR_TRANSMITTER`.

### 2.5 Assumptions and Dependencies

- Guest virtual devices use a `mac80211_hwsim`-compatible framing format.
- `ap-actor` populates `SharedKeyStore` with active AP BSSIDs and negotiated
  PTK/GTK cipher sessions during station association and 4-Way Handshake.
- Device 3D positions (`pose.position`) are maintained and updated via
  `ChipUpdate` from `device-actor`.

## 3. Standards and References

- **IEEE 802.11-2020**: Wireless LAN Medium Access Control (MAC) and Physical
  Layer (PHY) Specifications (Frame formats, ToDS/FromDS addressing, Fine
  Timing Measurement).
- **Linux `mac80211_hwsim` & `nl80211`**:
  [Kernel Documentation](https://docs.kernel.org/networking/mac80211_hwsim.html)
- **RFC 6762**: Multicast DNS (`224.0.0.251:5353`, MAC `01:00:5E:00:00:FB`).
- **Wi-Fi Alliance Specifications**: Wi-Fi Direct (P2P), Wi-Fi Aware (NAN), and
  Wi-Fi Easy Connect (DPP) Public Action Vendor-Specific frame formats (OUI
  `50:6F:9A`).

## 4. Functional Requirements

> [!IMPORTANT] All functional requirements listed below are mandatory (Priority
> P0) and implemented in `next/wifi-actor/`.

### 4.1 Chip Lifecycle (CRUD)

- **RQ-LIFE-01 (Chip Creation)**: The system shall create a Wi-Fi chip with an
  explicit `ChipId`, requiring both a `PacketStream` and `PacketSink`, spawning
  a dedicated outbound sink task, registering the inbound stream with the actor
  context, registering the client in `Medium`, and notifying the active
  gateway (`on_chip_create`). Creating a duplicate `ChipId` or omitting the ID
  or transport channels shall return `WifiError::Internal`.
- **RQ-LIFE-02 (Chip Read & List)**: The system shall support reading a single
  chip by `ChipId` (returning `ChipError::ChipNotFound` via `WifiClient` when
  absent) and listing all active Wi-Fi chips.
- **RQ-LIFE-03 (Chip Update & Radio State Sync)**: The system shall apply
  `ChipUpdate` patches (including pose/position and radio state) and synchronize
  `Medium` client enablement (`set_enabled`) with both `ChipUpdate.enabled` and
  `WifiUpdate.radio.state`.
- **RQ-LIFE-04 (Chip Reset)**: The system shall support resetting a chip
  (`WifiReq::Reset`), restoring its `Chip` model to its initial creation state
  and resetting its `Medium` station entries, enablement flag (`true`), and
  per-chip TX/RX counters to zero.
- **RQ-LIFE-05 (Chip Deletion & Teardown)**: When a chip is deleted via
  `handle_delete` or when its guest `PacketStream` or `PacketSink` task closes,
  the system shall remove the chip and its stations from `Medium`, notify the
  gateway (`on_chip_remove`), and asynchronously notify `DeviceActor` via
  `DeviceClient::notify_chip_removed`.

### 4.2 `mac80211_hwsim` Transport & Frame Processing

- **RQ-HWSIM-01 (Frame Validation & Parsing)**: The system shall parse incoming
  `HwsimMsg` frames (`HwsimCmd::Frame`), extracting the Netlink attributes
  (`HWSIM_ATTR_FRAME`, `HWSIM_ATTR_ADDR_TRANSMITTER`, `HWSIM_ATTR_FREQ`,
  `HWSIM_ATTR_RX_RATE`, `HWSIM_ATTR_SIGNAL`, `HWSIM_ATTR_COOKIE`,
  `HWSIM_ATTR_TX_INFO`, `HWSIM_ATTR_TX_INFO_FLAGS`) and decoding the embedded
  IEEE 802.11 header (`Ieee80211`).
- **RQ-HWSIM-02 (Immediate TX Info Acknowledgment)**: For every valid incoming
  `HwsimMsg` frame, the system shall construct and enqueue an immediate
  `HwsimCmd::TxInfoFrame` acknowledgment (`ack_frame`) back to the transmitting
  chip with `HWSIM_TX_STAT_ACK` set and the first TX rate count populated, and
  increment the sender's TX counter.
- **RQ-HWSIM-03 (Outbound `HwsimMsg` Encapsulation)**: When delivering an IEEE
  802.11 frame to a target station, the system shall encapsulate it into a
  `HwsimCmd::Frame` message populated with `HWSIM_ATTR_ADDR_RECEIVER` (set to
  the target station's `hwsim_addr`), `HWSIM_ATTR_FREQ` (defaulting to
  2412 MHz if unset), `HWSIM_ATTR_RX_RATE`, and `HWSIM_ATTR_SIGNAL` (defaulting
  to `-50` dBm).

### 4.3 Wireless Medium, Station Management & Routing

- **RQ-MED-01 (Station Learning & Dual MAC Mapping)**: Upon receiving a valid
  frame from a chip, `Medium` shall upsert a `Station` entry keyed by the
  802.11 source MAC (`Addr2`) recording `client_id`, `hwsim_addr`
  (`HWSIM_ATTR_ADDR_TRANSMITTER`), and channel frequency, and shall map both the
  802.11 source MAC and `hwsim_addr` to the target BSSID in `SharedKeyStore` so
  downlink responses (such as DHCP replies addressed to `hwsim_addr`) resolve
  the station's BSSID.
- **RQ-MED-02 (WPA2/WPA3 Frame Decryption & Re-Encryption)**: On uplink (`rx`),
  `Medium` shall attempt to decrypt protected 802.11 frames via
  `SharedKeyStore::try_decrypt` prior to routing. On downlink/delivery (`tx`),
  `Medium` shall encrypt infrastructure frames for the target station via
  `SharedKeyStore::try_encrypt` (falling back to the original encrypted frame
  for GTK multicast traffic when keys are not directly re-encrypted).
- **RQ-MED-03 (Peer-to-Peer & Multicast Station Routing)**: Frames addressed to
  a known station MAC or a multicast/broadcast MAC shall be delivered across
  `Medium` to matching enabled stations (`transmit`), incrementing P2P TX/RX
  counters for non-infrastructure payload data frames between distinct chips.
- **RQ-MED-04 (AP Management & EAPOL Routing)**: Management/control frames
  addressed to an active AP BSSID (or broadcast/multicast) and EAPOL data
  frames (`is_eapol`) shall be routed to `ap-actor` (`InfraTarget::Ap`).
  Downlink frames emitted by `ap-actor` on `AP_SUBSCRIPTION_ID` shall be
  broadcast or unicast to stations via `transmit_from_infra`.
- **RQ-MED-05 (AP Reflection & Infrastructure M2U)**: When
  `simulate_ap_reflection` is enabled, `ToDS` frames on an infrastructure BSSID
  that also target stations shall be reflected as `FromDS` frames. Multicast
  mDNS frames (`01:00:5E:00:00:FB` / `33:33:00:00:00:FB`) and unknown-unicast
  flooded frames on infrastructure networks shall undergo Multicast-to-Unicast
  (M2U) destination MAC rewriting per recipient station so protected networks
  encrypt each copy with the recipient's pairwise key (PTK).
- **RQ-MED-06 (Unknown Unicast Flooding & `hwsim_addr` Fallback)**: When an
  infrastructure frame targets an unknown unicast MAC, `resolve_targets` shall
  first fall back to matching `station.hwsim_addr` (preserving DHCP replies)
  and, if still unmatched in `route_infra_packet`, flood the frame across all
  known enabled stations via M2U.
- **RQ-MED-07 (Disabled Chip Filtering)**: When a chip is disabled in `Medium`
  (`enabled == false`), station-to-station transmissions from or to that chip
  shall be dropped, and infrastructure downlink deliveries to that chip shall
  be suppressed.

### 4.4 External Network Gateways (Slirp & TAP)

- **RQ-GW-01 (Pluggable Gateway Selection)**: `WifiActor` shall route
  non-EAPOL, payload-bearing `ToDS` data frames (`InfraTarget::Slirp`) through
  the configured `GatewayTrait` implementation (`TapGateway` on Linux when
  `wifi_tap` is provided; `SlirpGateway` otherwise, with automatic fallback to
  `SlirpGateway` on non-Linux platforms). QoS Null/NoData frames
  (`is_qos_nodata`) shall be silently dropped without error.
- **RQ-GW-02 (`SlirpGateway` Uplink & Downlink Bridging)**: `SlirpGateway`
  shall register a single L2 port (`SLIRP_ID`) with `SlirpActor` on startup,
  convert uplink 802.11 data frames to IEEE 802.3 Ethernet frames
  (`to_ieee8023`), and convert downlink Ethernet frames back into `FromAp` QoS
  802.11 data frames (`from_ieee8023_qos`) using the destination station's
  associated BSSID from `SharedKeyStore` (or all active BSSIDs for
  broadcast/multicast).
- **RQ-GW-03 (`TapGateway` Single & Pool Allocation)**: On Linux, `TapGateway`
  shall parse `--wifi-tap` configuration via `TapGateway::parse_config` as
  either the `"cvd-etap"` shortcut (expanding to pattern `"cvd-etap-%02d"` over
  pool range `6..=10`), a format pattern containing `%` (`%02d` or `%d`,
  allocating one TAP interface per chip from pool range `6..=10` in
  `on_chip_create` and releasing it in `on_chip_remove`), or a static single
  interface name shared across chips.
- **RQ-GW-04 (`TapGateway` Non-Blocking `/dev/net/tun` I/O)**: On Linux,
  `TapGateway` shall open `/dev/net/tun` with `IFF_TAP | IFF_NO_PI` and
  `O_NONBLOCK`, wrap the descriptor in `tokio::io::unix::AsyncFd`, stream
  inbound Ethernet frames into `WifiActor` (`TAP_FLAG | chip_id`), and write
  outbound IEEE 802.3 frames converted from guest `ToDS` 802.11 packets.

### 4.5 Host mDNS Multicast Reflection

- **RQ-MDNS-01 (Host Multicast Socket Subscription)**: When
  `forward_host_mdns` is true, `WifiActor` shall spawn `run_mdns_forwarder` on
  startup (`MDNS_SUBSCRIPTION_ID`), binding a UDP socket with `SO_REUSEADDR` /
  `SO_REUSEPORT` to `0.0.0.0:5353`, joining IPv4 multicast group `224.0.0.251`,
  and disabling IPv4 multicast loopback (`set_multicast_loop_v4(false)`).
- **RQ-MDNS-02 (mDNS Frame Synthesis & 802.11 Injection)**: For each IPv4 UDP
  packet received from the host multicast socket, the forwarder shall
  synthesize an IEEE 802.3 Ethernet + IPv4 (with valid header checksum) + UDP
  (`5353 -> 5353`) frame addressed to `01:00:5E:00:00:FB` and forward it via
  `MDNS_SUBSCRIPTION_ID` into `WifiActor`, which shall convert the Ethernet
  frame into `FromAp` QoS IEEE 802.11 frames (`Medium::process_ieee8023_response`)
  for delivery to simulated stations without attributing the packet to
  `hostapd_frames_rx` AP telemetry.

### 4.6 Ranging: FTM & PMSR

- **RQ-RNG-01 (IEEE 802.11mc FTM Action Frame Ranging)**: When a station
  transmits a Public Action `FTM_REQUEST` frame (`category::PUBLIC`,
  `public_action::FTM_REQUEST`) to a known peer station MAC, `WifiActor` shall
  intercept the request, compute the 3D Euclidean distance between the
  initiator and responder chip positions (`pose.position`), calculate
  `RTT = 2 * (distance / c)` in picoseconds, and enqueue two `FINE_TIMING_MEASUREMENT`
  Action response frames (an initial zero-timestamp frame followed by a
  measurement frame carrying 48-bit TOD/TOA timestamps).
- **RQ-RNG-02 (`mac80211_hwsim` PMSR Command Ranging)**: When a guest sends a
  `HwsimCmd::StartPmsr` command (`HWSIM_CMD_START_PMSR`), `WifiActor` shall
  intercept it on the fast path before medium resolution, compute the distance
  and RTT to a peer chip in `active_chips`, and return a Netlink-formatted
  `HwsimCmd::ReportPmsr` message populated with nested `NL80211_ATTR_PEER_MEASUREMENTS`
  attributes (`NL80211_PMSR_FTM_RESP_ATTR_RTT_AVG` in picoseconds and
  `NL80211_PMSR_FTM_RESP_ATTR_DIST_AVG` in millimeters).

### 4.7 Observability & Telemetry

- **RQ-OBS-01 (Per-Chip Radio Statistics)**: `WifiReq::GetStatistics` shall
  return a `NetsimRadioStats` entry (`RadioKind::Wifi`) for every active chip
  containing `tx_count`, `rx_count`, `p2p_tx_count`, and `p2p_rx_count`.
- **RQ-OBS-02 (Global IPC, Error & Frame Counters)**: `WifiStats`
  (`WifiReq::GetGlobalStats`) shall track and serialize to `ProtoWifiIpcStats`:
  `hwsim_frames_rx`, `hwsim_frames_tx`, `network_packets_tx`,
  `network_packets_rx`, `hostapd_frames_tx`, `hostapd_frames_rx`,
  `wmedium_frames_tx`, `wmedium_unicast_frames_tx`, `mgmt_frames_rx`,
  `mdns_packet_count`, and categorized error counts (`hostapd_errors`,
  `network_errors`, `client_errors`, `frame_errors`, `tx_errors`,
  `other_errors`), saturating `u64` counters at `i32::MAX`.
- **RQ-OBS-03 (Upload & Download Throughput Windowing)**: `WifiStats` shall
  accumulate upload and download payload bytes over 5,000 ms sliding windows
  driven by an injectable `Clock` trait (`SystemClock` in production,
  `MockClock` in tests) and report `throughput_upload_mbps` and
  `throughput_download_mbps`.
- **RQ-OBS-04 (Protocol Action Frame API Telemetry)**: `Medium::parse_action_frame`
  shall inspect management Action frames (`NAN`, `P2P`, `DPP`, `FTM`, `GAS`)
  and increment the 18 `WifiApi` counters mapped into `WifiApiStats`
  (`WifiAwareSessionStats`, `WifiP2pManagerStats`, `WifiRttManagerStats`, and
  `WifiEasyConnectStats`), tracking active P2P group owners to distinguish
  `CreateGroup` from `Connect`.

## 5. Non-Functional Requirements

- **RQ-NFR-PERF (Fast-Path Packet Processing)**: Standard data frames shall
  avoid unnecessary full Netlink PDL decoding on the PMSR fast path by checking
  the `hwsim_cmd` byte at offset 16 first, and reuse `WifiActor.out_queue` to
  minimize heap allocations per packet.
- **RQ-NFR-ISOLATION (Non-Blocking Per-Chip Sinks)**: Each chip's `PacketSink`
  shall be driven by an isolated spawned task fed by an unbounded channel so a
  slow or stalled guest cannot block packet routing for other chips.
- **RQ-NFR-PORTABILITY (Cross-Platform Compilation)**: `wifi-actor` shall build
  and pass unit and integration tests on Linux and macOS, gating `/dev/net/tun`
  and `nix`/`libc` dependencies behind `#[cfg(target_os = "linux")]`.

## 6. Verification

### 6.1 Automated Tests

Unit and integration tests are located in `next/wifi-actor/src/` and
`next/wifi-actor/tests/`:

- **Integration Test Suite (`next/wifi-actor/tests/`)**:
  - `mod.rs`: End-to-end chip creation, peer unicast delivery, radio disablement
    packet drop, chip deletion, and `DeviceAction::NotifyChipRemoved`
    verification (`test_full_lifecycle_and_messaging`).
  - `lifecycle_test.rs`: Chip creation, deletion, stream/sink closure
    notifications, `ChipClient` trait conformance, and radio state updates.
  - `packet_routing_test.rs`: P2P unicast, broadcast, infrastructure routing,
    Slirp uplink/downlink, mDNS peer and infrastructure discovery, management
    frame forwarding to AP, AP reflection, disabled client packet dropping,
    unknown unicast flooding, DHCP M2U race prevention, P2P transparent proxy,
    open/secure network mDNS M2U conversion, and open/secure network DHCP.
  - `gateway_test.rs`: Pluggable `GatewayTrait` injection and `ToDS` data frame
    routing to a custom gateway (`test_gateway_routing`).
  - `slirp_dual_mapping_test.rs`: Dual MAC registration (`Addr2` and
    `hwsim_addr`) ensuring Slirp downlink frames addressed to `hwsim_addr` are
    not dropped (`test_slirp_does_not_drop_with_dual_mapping`).
  - `ftm_test.rs`: IEEE 802.11mc FTM request parsing, 3D distance/RTT
    calculation, and two-frame FTM response generation (`test_handle_ftm_request`).
  - `pmsr_test.rs`: `HWSIM_CMD_START_PMSR` interception and
    `HWSIM_CMD_REPORT_PMSR` Netlink response verification
    (`test_pmsr_interception`).
  - `stats_test.rs`: Client/TX error counters, hostapd/network TX counters,
    upload/download throughput window calculation with `MockClock`, and P2P
    action frame telemetry in routing.
- **Inline Unit Tests (`next/wifi-actor/src/`)**:
  - `src/medium/core.rs`: P2P per-chip counter increments and reset
    (`test_p2p_telemetry_counters`), and NAN/P2P/DPP/FTM/GAS action frame
    telemetry parsing (`test_parse_action_frame`).
  - `src/mdns_forwarder.rs`: IPv4 header checksum calculation
    (`test_ipv4_header_checksum_calculation`) and Ethernet/IPv4/UDP mDNS frame
    synthesis (`test_create_mdns_frame`).
  - `src/pmsr.rs`: `handle_start_pmsr` response generation and missing-peer
    handling (`test_handle_start_pmsr`).
  - `src/tap_gateway.rs` (Linux): Single vs. range-pool config parsing,
    zero-padded interface name formatting, pool allocation/release, and pool
    exhaustion error handling.

### 6.2 Traceability & BDD Coverage Mapping

| Requirement ID | Requirement Summary | Automated Unit / Integration Tests (`next/wifi-actor/`) | Related Epic & BDD Feature Files (`next/reqs/`) | Coverage |
| :--- | :--- | :--- | :--- | :--- |
| **RQ-LIFE-01** | Chip Creation & Stream Registration | `lifecycle_test.rs::test_create_chip`, `mod.rs::test_full_lifecycle_and_messaging` | `bdd/wifi/WifiManager/wifi_state.feature` | **Full** |
| **RQ-LIFE-02** | Chip Read & List | `lifecycle_test.rs::test_create_chip`, `lifecycle_test.rs::test_wifi_client_implements_chip_client` | `bdd/core/observables.feature` | **Partial** (Read tested; List untested) |
| **RQ-LIFE-03** | Chip Update & Radio State Sync | `lifecycle_test.rs::test_update_chip_state`, `mod.rs::test_full_lifecycle_and_messaging` | `bdd/wifi/WifiManager/wifi_state.feature` (`REQ-WIFI-01`) | **Full** |
| **RQ-LIFE-04** | Chip Reset | `src/medium/core.rs::test_p2p_telemetry_counters` (`Medium::reset`) | `bdd/wifi/WifiManager/wifi_state.feature` | **Partial** (`Medium::reset` tested; `WifiClient::reset` actor path untested) |
| **RQ-LIFE-05** | Chip Deletion & Disconnect Notification | `lifecycle_test.rs::test_delete_chip`, `lifecycle_test.rs::test_sink_closure_notification`, `mod.rs::test_full_lifecycle_and_messaging` | `bdd/wifi/WifiManager/wifi_state.feature` | **Full** |
| **RQ-HWSIM-01** | `HwsimMsg` Validation & Parsing | `stats_test.rs::test_client_error_stat`, `packet_routing_test.rs::test_p2p_wifi_direct_flow` | `epics/wifi.md` | **Full** |
| **RQ-HWSIM-02** | Immediate `TxInfoFrame` Ack | `packet_routing_test.rs::test_p2p_wifi_direct_flow`, `stats_test.rs::test_tx_error_stat` | `epics/wifi.md` | **Full** |
| **RQ-HWSIM-03** | Outbound `HwsimMsg` Encapsulation | `packet_routing_test.rs::test_p2p_wifi_direct_flow`, `packet_routing_test.rs::test_infra_flow_real` | `epics/wifi.md` | **Full** |
| **RQ-MED-01** | Station Learning & Dual MAC Mapping | `slirp_dual_mapping_test.rs::test_slirp_does_not_drop_with_dual_mapping` | `bdd/wifi/WifiManager/wifi_ap.feature` | **Full** |
| **RQ-MED-02** | WPA2/WPA3 Decryption & Re-Encryption | `packet_routing_test.rs::test_secure_network_dhcp_broadcast`, `packet_routing_test.rs::test_dhcp_m2u_race_condition` | `bdd/wifi/WifiManager/wifi_auth.feature` (`REQ-WIFI-03`, `REQ-WIFI-04`) | **Full** |
| **RQ-MED-03** | P2P & Multicast Station Routing | `packet_routing_test.rs::test_p2p_wifi_direct_flow`, `packet_routing_test.rs::test_broadcast_flow_real`, `packet_routing_test.rs::test_p2p_transparent_proxy` | `bdd/wifi/WifiP2pManager/wifi_p2p.feature`, `bdd/wifi/WifiAwareManager/wifi_aware.feature` (`REQ-WIFI-08`..`10`), `bdd/core/p2p.feature` | **Full** |
| **RQ-MED-04** | AP Management & EAPOL Routing | `packet_routing_test.rs::test_station_to_ap_mgmt_stub`, `packet_routing_test.rs::test_infra_flow_real`, `stats_test.rs::test_hostapd_and_network_tx_stats` | `bdd/wifi/WifiManager/wifi_ap.feature`, `wifi_scanning.feature` (`REQ-WIFI-01`, `REQ-WIFI-02`) | **Full** |
| **RQ-MED-05** | AP Reflection & Infrastructure M2U | `packet_routing_test.rs::test_ap_reflection_flow_stub`, `test_open_network_mdns_m2u`, `test_open_network_non_mdns_remains_broadcast`, `test_mdns_discovery_infrastructure_flow` | `bdd/wifi/NsdManager/wifi_service_discovery.feature` (`REQ-WIFI-07`), `bdd/network_services/nsd.feature` | **Full** |
| **RQ-MED-06** | Unknown Unicast Flooding & DHCP Fallback | `packet_routing_test.rs::test_unknown_unicast_flooding`, `test_open_network_dhcp`, `test_secure_network_dhcp_broadcast` | `bdd/wifi/WifiManager/wifi_ap.feature` | **Full** |
| **RQ-MED-07** | Disabled Chip Filtering | `packet_routing_test.rs::test_disabled_client_drop_stub`, `mod.rs::test_full_lifecycle_and_messaging` | `bdd/wifi/WifiManager/wifi_state.feature` | **Full** |
| **RQ-GW-01** | Pluggable Gateway Selection & QoS NoData Drop | `gateway_test.rs::test_gateway_routing`, `packet_routing_test.rs::test_uplink_data_flow_stub` | `bdd/performance/gateway_performance.feature` | **Full** |
| **RQ-GW-02** | `SlirpGateway` 802.11 <-> 802.3 Bridging | `packet_routing_test.rs::test_uplink_data_flow_stub`, `test_uplink_multicast_flow`, `test_downlink_data_flow_stub`, `slirp_dual_mapping_test.rs` | `bdd/wifi/WifiManager/wifi_ap.feature` | **Full** |
| **RQ-GW-03** | `TapGateway` Single & Pool Allocation | `src/tap_gateway.rs::test_new_config_parsing`, `test_format_name`, `test_allocate_next_tap`, `test_allocate_exhausted_pool`, `test_allocate_single_mode` | `N/A` | **Full** |
| **RQ-GW-04** | `TapGateway` `/dev/net/tun` Data Plane | `tests/veth_test.rs::test_udp_guest_to_host` (currently disabled in `tests/mod.rs`) | `bdd/performance/gateway_performance.feature` | **Gap** (Requires `CAP_NET_ADMIN` / root privileges) |
| **RQ-MDNS-01** | Host Multicast Socket Subscription | `N/A` (Live UDP multicast socket loop in `run_mdns_forwarder`) | `bdd/network_services/forward_host_mdns.feature` | **Partial** (Covered by E2E BDD scenario) |
| **RQ-MDNS-02** | Host mDNS Frame Synthesis & 802.11 Injection | `src/mdns_forwarder.rs::test_ipv4_header_checksum_calculation`, `test_create_mdns_frame`, `packet_routing_test.rs::test_mdns_discovery_flow` | `bdd/network_services/forward_host_mdns.feature`, `bdd/wifi/NsdManager/wifi_service_discovery.feature` | **Partial** (Frame synthesis tested; `on_typed_stream` 802.3-to-802.11 bridge has implementation gap) |
| **RQ-RNG-01** | IEEE 802.11mc FTM Action Frame Ranging | `ftm_test.rs::test_handle_ftm_request` | `bdd/wifi/WifiRttManager/wifi_ranging.feature` | **Full** |
| **RQ-RNG-02** | `mac80211_hwsim` PMSR Command Ranging | `pmsr_test.rs::test_pmsr_interception`, `src/pmsr.rs::test_handle_start_pmsr` | `bdd/wifi/WifiRttManager/wifi_ranging.feature` | **Full** |
| **RQ-OBS-01** | Per-Chip Radio Statistics (`GetStatistics`) | `stats_test.rs::test_p2p_telemetry_in_routing`, `src/medium/core.rs::test_p2p_telemetry_counters` | `bdd/core/netsim_observables.feature`, `bdd/core/observables.feature` | **Full** |
| **RQ-OBS-02** | Global IPC, Error & Frame Counters | `stats_test.rs::test_client_error_stat`, `test_tx_error_stat`, `test_hostapd_and_network_tx_stats` | `bdd/core/netsim_observables.feature` | **Full** |
| **RQ-OBS-03** | Upload & Download Throughput Windowing | `stats_test.rs::test_throughput_stats` | `bdd/core/netsim_observables.feature` | **Full** |
| **RQ-OBS-04** | Protocol Action Frame API Telemetry (`WifiApi`) | `src/medium/core.rs::test_parse_action_frame`, `stats_test.rs::test_p2p_telemetry_in_routing` | `bdd/core/netsim_observables.feature` | **Full** |

### 6.3 Coverage Gaps

- **RQ-GW-04 (Linux TAP Data-Plane Integration Test)**: `tests/veth_test.rs`
  (`test_udp_guest_to_host`) is currently commented out in `tests/mod.rs`
  because opening `/dev/net/tun` and configuring `veth`/`tap` interfaces
  requires `CAP_NET_ADMIN` privileges unavailable in unprivileged hermetic test
  sandboxes. Only the pure configuration and pool-allocation logic in
  `tap_gateway.rs` runs in standard unit tests.
- **RQ-MDNS-02 (Host mDNS 802.3-to-802.11 Dispatch in `on_typed_stream`)**:
  `run_mdns_forwarder` in `src/mdns_forwarder.rs` synthesizes an IEEE 802.3
  Ethernet frame on `MDNS_SUBSCRIPTION_ID`, but `on_typed_stream` in
  `src/lifecycle.rs` currently dispatches both `AP_SUBSCRIPTION_ID` and
  `MDNS_SUBSCRIPTION_ID` to `process_ap_packet`, which increments
  `hostapd_frames_rx` and passes the raw bytes to `Medium::transmit_from_infra`
  (expecting an IEEE 802.11 frame) instead of `Medium::process_ieee8023_response`.
- **RQ-LIFE-02 & RQ-LIFE-04 (`WifiClient::reset` and `handle_list` Actor Round-Trip)**:
  While `Medium::reset` is unit-tested in `src/medium/core.rs`, there is no
  integration test in `lifecycle_test.rs` exercising `WifiClient::reset`
  (verifying restoration of `initial_chips` metadata after a `ChipUpdate`) or
  `read_count_for_testing` (`handle_list`) end-to-end.
- **RQ-RNG-02 (Multi-Peer PMSR Target Selection)**: `handle_start_pmsr` in
  `src/pmsr.rs` currently selects the first peer chip in `active_chips` with
  `id != initiator_id` rather than parsing the target peer MAC address from the
  incoming `HWSIM_CMD_START_PMSR` Netlink attributes (tracked by `TODO` in
  `src/pmsr.rs`).
- **Distance-Based RSSI Attenuation & Roaming (`REQ-WIFI-02`, `REQ-WIFI-05`, `REQ-WIFI-06`)**:
  Outbound `HwsimMsg` frames currently default to a static `-50` dBm signal
  attribute (`create_hwsim_msg_from_frame` in `src/medium/utils.rs`) rather
  than computing path-loss RSSI from 3D `Position` distance. Consequently,
  distance-based scan filtering and RSSI-driven roaming scenarios in
  `wifi_scanning.feature` and `wifi_roaming.feature` remain tagged `@nyi`.
