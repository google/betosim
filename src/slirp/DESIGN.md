# libslirp-rs: The Design of a Modern User-Mode Networking Library

**Version: 8.0**

## 1. Introduction: User-Mode Networking in Virtualization

In virtualization environments like QEMU, **user-mode networking** provides a simple and secure way for a guest (a virtual machine or container) to access the network without requiring elevated privileges on the host. Instead of bridging the guest to a physical network interface (which requires root), the guest's entire network stack is handled within the virtualization process itself.

`libslirp-rs` is a modern, from-scratch rewrite of the classic `libslirp` C library, designed to be a performant, safe, and extensible foundation for user-mode networking.

---

## 2. Two Approaches: NAT vs. Passthrough

There are two primary models for implementing user-mode networking, each with distinct trade-offs.

| Feature / Aspect | `libslirp` (NAT Model) | `passt` (Passthrough Model) |
| :--- | :--- | :--- |
| **Core Model** | Implements a full, in-process TCP/IP stack. It **terminates** guest connections and creates **new** connections from the host (Network Address Translation). | Acts as a simple packet forwarder. It **pastes** the guest's socket options onto a new host socket and then shuttles payloads back and forth. |
| **Performance** | Lower. Every packet must be parsed, processed by the internal TCP/IP state machine, and potentially re-constructed. | Near-native. After initial setup, it's a direct pipe between the guest and the host socket, bypassing most of the host's network stack. |
| **Protocol Support** | Excellent. Can handle any IP-based protocol (TCP, UDP, ICMP, etc.) because it operates a full network stack. | Limited. Typically only supports TCP and UDP, as it doesn't have the logic to handle protocols like ICMP (e.g., `ping`). |
| **Compatibility** | Very high. Works in almost any environment because it makes simple, standard outgoing connections. It's compatible with complex network setups like VPNs on the host. | Lower. Can run into issues with certain VPN configurations and requires more specific kernel features on the host. |
| **Complexity** | High. Implementing a full TCP/IP stack is a complex and error-prone task. | Low. The core logic is much simpler, as it's primarily forwarding data between sockets. |
| **Primary Example** | The default `-netdev user` in QEMU. | The standalone `passt` tool and QEMU's `slirpnetstack`. |

---

## 3. Clarifying the Layers: How `libslirp-rs` Operates

The term "NAT" can be ambiguous. `libslirp-rs` is not a simple IP-to-IP translator; it's a full network stack that operates across multiple network layers to provide connectivity. What it implements is more formally known as **NAPT (Network Address Port Translation)**.

Here’s a brief overview of the relevant layers and what `libslirp-rs` does at each one.

*   **Layer 2 (Data Link):** This is the layer of **MAC addresses** and local network segments (e.g., Ethernet).
    *   `libslirp-rs` **simulates** a virtual network card for the guest. It receives complete L2 Ethernet frames from the guest and must construct new L2 frames to send back to it. It uses ARP to resolve the guest's IP to a virtual MAC address.

*   **Layer 3 (Network):** This is the layer of **IP addresses** and routing between networks.
    *   This is the "Address Translation" part of NAPT. When a guest sends a packet from its private IP (e.g., `10.0.2.15`), `libslirp-rs` decapsulates it and creates a **new** IP packet. This new packet's source is the host's IP address. It maintains a mapping table to route replies back to the correct guest.

*   **Layer 4 (Transport):** This is the layer of **ports** (e.g., TCP and UDP), which directs traffic to specific applications.
    *   This is the "Port Translation" part of NAPT. When the guest sends a packet from a source port (e.g., `54321`), `libslirp-rs` opens a *new, ephemeral port* on the host (e.g., `61234`). It adds this port mapping to its state table. This is essential because multiple guests (or the host itself) might want to use the same source port.

The following table summarizes the process for an outbound packet from the guest:

| Layer | Guest Packet (Outbound) | `libslirp-rs` Action | Host Packet (Outbound) |
| :--- | :--- | :--- | :--- |
| **L4 (TCP)** | `src_port: 54321`, `dst_port: 80` | **Translate & Track:** Opens a new host port. | `src_port: 61234`, `dst_port: 80` |
| **L3 (IP)** | `src_ip: 10.0.2.15`, `dst_ip: 8.8.8.8` | **Translate & Track:** Replaces guest IP with host IP. | `src_ip: 192.168.1.100`, `dst_ip: 8.8.8.8` |
| **L2 (Eth)** | `src_mac: 52:54:..`, `dst_mac: 0A:0B:..` | **Decapsulate/Recapsulate:** Strips the guest's L2 header. The host's physical NIC adds a new one for the physical network. | *(Handled by Host OS/Hardware)* |

Because `libslirp-rs` terminates the entire TCP connection and creates a new one, it is fundamentally a **Layer 4 NAT**.

---

## 4. Architecture Refactoring: The Actor Model

The initial architecture of `libslirp-rs` was based on an `async-trait` `Host` object that was passed mutably into the `Slirp` core. While this provided a clean separation of concerns, it introduced fundamental concurrency problems that led to deadlocks and severe performance degradation under load. The core issue was the lack of a clear ownership model for the network state, leading to complex and unsafe lock-sharing between the `Slirp` logic and the `Host` I/O implementation.

To solve this, the library is being refactored to a pure **Actor Model**, inspired by high-performance network stacks like Fuchsia's Netstack3. This model enforces a strict separation between state and I/O, eliminating the possibility of deadlocks and creating a clear, single-threaded path for all state mutations.

### 4.1. The New Architecture: Two Actors, Two Channels

The new design consists of two primary components (actors) that communicate exclusively through asynchronous message-passing channels:

1.  **The `Slirp` Actor (Control Plane):**
    *   **Synchronous and Single-Threaded:** The `Slirp` struct is now a pure, synchronous state machine. It performs no I/O and has no `async` code.
    *   **Owner of State:** It owns *all* networking state: the TCP connection table, UDP flow table, ARP cache, DHCP leases, etc. Because it is single-threaded, it can access this state without any locks, completely eliminating race conditions.
    *   **Message-Driven:** It runs in its own dedicated thread (or a pinned `tokio` task), processing a stream of `SlirpRequest` messages from an MPSC (multi-producer, single-consumer) channel.
    *   **Emits Responses:** After processing a request, it emits one or more `SlirpResponse` messages to an output channel.

2.  **The `TokioHost` Actor (I/O Plane):**
    *   **Asynchronous and Multi-Threaded:** This actor is responsible for all I/O operations. It manages the guest TAP device, TCP streams, UDP sockets, and timers, using the `tokio` runtime to handle thousands of concurrent operations.
    *   **Stateless (Mostly):** It owns the I/O resources (sockets, file descriptors) but holds no high-level networking state. Its primary job is to translate I/O events into `SlirpRequest` messages and `SlirpResponse` messages into I/O actions.
    *   **Event-Driven:** It reacts to events from three sources:
        1.  Packets arriving from the guest TAP device.
        2.  Data arriving from host TCP/UDP sockets.
        3.  `SlirpResponse` messages arriving from the `Slirp` actor.

### 4.2. Visualizing the Actor Model

```
                                  +--------------------------------+
                                  |      TokioHost (I/O Actor)     |
                                  | (Async, Multi-Threaded)        |
                                  +--------------------------------+
                                     ^   |                 ^   |
                                     |   |                 |   |
+------------------+   SlirpResponse |   | SlirpRequest    |   |
| Guest TAP Device | <---------------|---|-----------------|---> | Host Sockets (TCP/UDP)
+------------------+   (e.g. Packet) |   | (e.g. Packet)   |   | (e.g. Data)
                                     |   v                 |   v
+--------------------------------------------------------------------------------------+
|                                                                                      |
|   mpsc::Receiver<SlirpRequest>      +-------------------------+    mpsc::Sender<SlirpResponse>   |
| <-----------------------------------|   Slirp (Control Actor)   |----------------------------------> |
|                                     | (Sync, Single-Threaded) |                                    |
|                                     +-------------------------+                                    |
|                                                                                      |
+--------------------------------------------------------------------------------------+
```

### 4.3. The Message Protocol: `SlirpRequest` and `SlirpResponse`

The communication between the actors is strictly defined by two enums:

*   `SlirpRequest`: A message sent *from* the `TokioHost` *to* `Slirp`.
    *   `Packet(Bytes)`: A raw Ethernet frame from the guest.
    *   `Timer`: A notification that a timer set by `Slirp` has fired.
    *   `ConnectionClosed { id }`: Notification that a host-side TCP connection was closed.
    *   `Data { id, data }`: Data received from a host-side TCP connection.

*   `SlirpResponse`: A message sent *from* `Slirp` *to* the `TokioHost`.
    *   `Packet(Bytes)`: A raw Ethernet frame to be sent to the guest.
    *   `SetTimer { duration }`: A request to the `TokioHost` to set a new timer.
    *   `EstablishConnection { id, protocol, dest }`: A request to open a new TCP/UDP connection on the host.
    *   `CloseConnection { id }`: A request to close a host-side connection.
    *   `SendData { id, data }`: A request to send data on a host-side connection.

### 4.4. Step-by-Step Refactoring Plan

This section details the concrete steps to fully implement the actor model.

**1. [DONE] Make `Slirp` and its Submodules Synchronous:**
    *   Remove all `async` keywords from `lib/src/lib.rs`, `tcp/`, `udp/`, etc.
    *   Remove the `async-trait` dependency and the `Host` trait definition.
    *   All methods that previously returned `io::Result<()>` and performed I/O via the `Host` trait will now return a `Vec<SlirpResponse>`.

**2. [DONE] Define the Message-Passing API:**
    *   Define the `SlirpRequest` and `SlirpResponse` enums in `lib/src/lib.rs`.
    *   Create a new `Slirp::handle_request(&mut self, req: SlirpRequest) -> Vec<SlirpResponse>` method. This will be the single entry point for all interactions with the `Slirp` core.
    *   The existing `Slirp::handle_packet` and `Slirp::handle_timer` methods will be wrapped by `handle_request`.

**3. [DONE] Rewrite `TokioHost` as a Self-Contained Actor:**
    *   Create a `TokioHost` struct that contains the `mpsc` channels for communicating with `Slirp`.
    *   Implement a `run()` method that spawns the main tasks:
        *   **Guest Packet Handler:** A task that reads packets from the TAP device and sends them as `SlirpRequest::Packet` messages.
        *   **Host I/O Manager:** A task that manages all host-side sockets. It will use a `select!` loop to listen for incoming data on all active sockets and for new `SlirpResponse` messages from `Slirp`.
        *   **`Slirp` Runner:** A task that runs the `Slirp` actor itself, pulling requests from the request channel and pushing responses to the response channel. This can be a blocking task using `tokio::task::spawn_blocking`.

**4. [DONE] Implement the `TokioHost` I/O Logic:**
    *   When the `Host I/O Manager` receives a `SlirpResponse::EstablishConnection`, it will create a new `tokio::net::TcpStream` or `UdpSocket`. It will store the socket in a `HashMap` keyed by the connection ID provided by `Slirp`. It will then spawn a *new* dedicated task to read data from this socket and send it to `Slirp` as `SlirpRequest::Data` messages.
    *   When it receives `SlirpResponse::SendData`, it looks up the connection's `TcpStream` in its map and writes the data.
    *   When it receives `SlirpResponse::Packet`, it writes the packet to the TAP device.

**5. [DONE] Update the `perf` Crate Test Harness:**
    *   The performance test harness in the `perf` crate will be updated to drive the new actor system.
    *   It will create the `Slirp` and `TokioHost` actors and their channels.
    *   It will simulate a guest by sending raw packets into the `SlirpRequest` channel and asserting that the correct `SlirpResponse` packets are produced.

This refactoring will result in a more robust, performant, and maintainable library, completely eliminating the concurrency issues of the previous design and setting a solid foundation for future features like the Fast Path bypass.

---

## 5. Usage in Containerized Environments

A primary use case for user-mode networking is running a virtualized environment (like QEMU) inside a container (like Docker). This setup provides excellent isolation and reproducibility but introduces a "network within a network" scenario that requires special consideration.

### 5.1. The Challenge: Double Port Forwarding

When a `libslirp`-powered VM runs inside a container, there are two layers of network address translation. To expose a service from the VM to the outside world, a port must be mapped through both layers.

**Example Scenario:** Exposing a web server running on port 80 inside the guest VM.

1.  **The `libslirp` Layer:** The application using `libslirp-rs` (e.g., QEMU) must be configured to forward a port from its host (the container) to the guest.
    *   `hostfwd = tcp::8080-:80`
    *   This tells `libslirp-rs` to listen on port `8080` on all network interfaces *inside the container* and forward any incoming traffic to the guest's IP on port `80`.

2.  **The Container Runtime Layer:** The container itself must be started with a port mapping instruction.
    *   `docker run -p 127.0.0.1:8080:8080 ...`
    *   This tells Docker to listen on port `8080` on the *physical host's* loopback interface and forward any incoming traffic to port `8080` *inside the container*.

The complete path for a network packet is:
`Physical Host (127.0.0.1:8080)` -> `Container (:8080)` -> `libslirp-rs` -> `Guest VM (:80)`

### 5.2. Address Considerations

*   **Guest Network:** `libslirp-rs` creates a private virtual network for the guest (e.g., `10.0.2.0/24`).
*   **Container Network:** The container has its own IP address within its own network (e.g., `172.17.0.2`).
*   **Host Perspective:** From the perspective of `libslirp-rs`, the "host" is the container. When it makes outgoing connections, they will originate from the container's IP (`172.17.0.2`), not the physical host's IP.

This is a key feature, as it ensures guest traffic is properly isolated and firewalled by the container's network policies.

### 5.3. The Localhost Problem

A related challenge is when a guest tries to connect to `127.0.0.1`. This address refers to the container's loopback interface, not the physical host's. If a service is running on the physical host's localhost, the guest cannot reach it. The `LocalhostMappingHost` decorator, described in section 4.2, is designed specifically to solve this problem by transparently redirecting such connections to the container's host gateway or a specific external IP.

---

## 6. References

*   **QEMU Networking Documentation:** [Official QEMU Docs](https://www.qemu.org/docs/master/system/net.html)
*   **Original C `libslirp`:** [GitLab Repository](https://gitlab.com/qemu-project/libslirp)
*   **`passt` (Passthrough Networking):** [Project Homepage](https://passt.top/passt/)

---

## 8. Implementation Progress

### Phase 0: Prerequisite: Enhance `packets-zc`
- [x] Implement `IcmpEchoBuilder`
- [x] Implement `Ipv4Builder`
- [x] Implement `EthernetFrameBuilder`
- [x] Implement `ArpPacketBuilder`
- [x] Define `ArpPacket` struct
- [x] Define `NdpPacket` structs
- [x] Implement `NdpPacketBuilder`

### Phase 1: Foundation & Scaffolding (`libslirp-rs`)
- [x] Create the `libslirp-rs` crate.
- [x] Define core `Slirp`, `Host`, and `Config` structs.
- [x] Implement the `TimerManager`.

### Phase 2: Packet Dispatch & L3 (IP/ICMP)
- [x] Implement Ethernet frame parsing and dispatch.
- [x] Implement IPv4 packet parsing and dispatch.
- [x] Implement ICMP Echo Request/Reply logic.
- [x] **Implement ICMP Error Message Generation:**
  - [x] Implement ICMP error packet generation logic in `IcmpManager` (Host Unreachable, Port Unreachable, Time Exceeded).
  - [x] Trigger ICMP "Host Unreachable" when a TCP connection fails.
  - [x] Trigger ICMP "Port Unreachable" when a UDP packet is sent to a closed gateway port.
  - [x] Trigger ICMP "Time Exceeded" when a routed packet's TTL expires (TTL <= 1).
- [x] **Implement IP Fragmentation and Reassembly:**
  - [x] Implement logic to fragment outgoing IP packets that exceed the MTU.
  - [x] Implement logic to reassemble incoming IP packet fragments.

### Phase 3: L2 Address Resolution (ARP)
- [x] Implement `ArpTable` and ARP request handling.

### Phase 4: L4 Protocols (UDP & TCP)
- [x] Implement UDP forwarding.
- [x] Implement TCP SYN handling.
- [x] Implement TCP SYN-ACK handling.
- [x] Implement TCP data transfer.
- [x] Implement TCP FIN handling.
- [x] **Implement TCP RST Generation:**
  - [x] Send a TCP `RST` packet when a non-`SYN` packet is received for a non-existent connection.
- [x] **Phase 4.1: Implement Robust Connection Management**
  - [x] Implement connection timeout in `SYN_SENT` state.
  - [x] Implement max retransmission limit for data packets.
  - [x] Refine `TIME_WAIT` state handling to prevent immediate 4-tuple reuse.
- [x] **Phase 4.2: Implement Advanced TCP Features**
  - [x] Implement parsing for Maximum Segment Size (MSS) option.
  - [x] Implement dynamic Retransmission Timeout (RTO) calculation.
  - [x] Implement a sliding window for flow control.
  - [x] Implement enhanced congestion control (Fast Retransmit/Recovery).
  - [x] Implement Window Scaling (RFC 1323).
  - [x] Implement Selective ACKs (SACK).

### Phase 5: Application-Level Protocols & Host Integration

- [x] **DHCP Server**
  - [x] Handle DHCP Discover/Request and send Offer/Ack.
  - [x] Manage a pool of IP address leases.
  - [x] Support common DHCP options (`boot_file`, `domain_name`, etc.).
  - [x] Send `DHCPNAK` for invalid requests.
  - [x] Support legacy BOOTP, `DHCPINFORM`, and `DHCPRELEASE` messages.
- [x] **DNS Proxy**
  - [x] Intercept guest DNS queries (UDP port 53) and redirect them to host DNS servers.
  - [x] Translate DNS replies back and forward them to the guest (UDP NAPT reply translation).
  - [x] Implement DNS query failover and caching.
  - [x] Implement automatic host DNS server discovery (e.g., by parsing `/etc/resolv.conf`).
- [x] **Host Port Forwarding (`hostfwd`)**
  - [x] Implement configuration for forwarding rules (host-driver task).
  - [x] Implement host-side listeners for incoming connections (host-driver task).
  - [x] Implement logic to forward host connections to the guest in core library.
- [x] **Guest Port Forwarding (`guestfwd`)**
  - [x] Implement configuration for guest forwarding rules.
  - [x] Intercept guest-initiated connections to a specific address.
  - [x] Forward the connection to a specified service on the host (e.g., `127.0.0.1`).
- [x] **TFTP Server**
  - [x] Implement a TFTP server to handle Read Requests (RRQ).
  - [x] Include security checks to restrict file access to a TFTP root directory.

### Phase 6: Comprehensive IPv6 Support

- [x] **Phase 6.1: Prerequisites & Core Refactoring**
  - [x] Add `Ipv6Header` parsing to `packets-zc`.
  - [x] Refactor `TcpManager` and `UdpManager` to be generic over `IpAddr` (supporting both `Ipv4Addr` and `Ipv6Addr`).
  - [x] Update connection tracking data structures to use `SocketAddr` instead of `SocketAddrV4`.
  - [x] Implement a `Slirp::handle_ipv6_packet` method for L3 dispatch.
- [x] **Phase 6.2: Neighbor Discovery Protocol (NDP)**
  - [x] Implement `NdpTable` for basic Neighbor Solicitation/Advertisement.
  - [x] Implement Router Solicitation (RS) handling.
  - [x] Implement periodic and solicited Router Advertisement (RA) sending.
  - [x] In RA messages, include the Source Link-Layer Address and Prefix Information options to enable guest SLAAC.
- [x] **Phase 6.3: ICMPv6**
  - [x] Add ICMPv6 packet definitions and builders to `packets-zc`.
  - [x] Implement ICMPv6 Echo (ping) Request/Reply.
  - [x] Implement ICMPv6 "Destination Unreachable" error generation.
  - [x] Implement ICMPv6 "Packet Too Big" error generation, which is critical for Path MTU Discovery.
- [x] **Phase 6.4: L4 Forwarding**
  - [x] Enable TCP and UDP forwarding for IPv6 connections.
  - [x] Write integration tests for end-to-end TCP and UDP connectivity over IPv6.
- [x] **Phase 6.5: Application-Level Protocols**
  - [x] **DNS Proxy:**
    - [x] Extend the DNS proxy to handle `AAAA` record requests (verified unified UDP handler and DNS redirection handles IPv6 DNS transparently).
    - [x] Enable the proxy to forward queries to the host's IPv6 DNS servers.
    - [x] Announce the virtual DNS server via the RDNSS option in NDP Router Advertisements.
  - [x] **DHCPv6:**
    - [x] Implement a stateless DHCPv6 server to provide network information (like DNS servers and domain search lists) to the guest.

### Phase 7: Management and Operations
- [x] **State Inspection API**
  - [x] Implement a `Slirp::connection_info()` method to return a list of active TCP and UDP flows.
- [x] **State Save/Restore**
  - [x] Implement `Slirp::save_state()` to serialize the entire network state (connections, leases, etc.).
  - [x] Implement `Slirp::restore_state()` to deserialize and resume from a saved state.
  - [x] Ensure the `Host` trait has methods to save/restore its I/O state (e.g., re-opening sockets).
- [x] **Debug Logging**
  - [x] Implement a configurable logging system similar to the C `libslirp`'s `SLIRP_DEBUG` flags. This should allow enabling detailed logs for specific modules (e.g., `tcp`, `udp`, `icmp`) to aid in debugging.

### Phase 8: Advanced Enterprise & Diagnostic Features
- [x] **DNS-over-TCP Proxying**
  - [x] Intercept guest TCP connections to the gateway on port 53.
  - [x] Transparently redirect them to the host's configured TCP DNS servers.
  - [x] Write integration tests for DNS-over-TCP.
- [x] **Upstream SOCKS5 Proxy Support**
  - [x] Add `socks5_proxy` configuration option to `Config`.
  - [x] In `TokioHost` driver, route all outgoing TCP connections through the SOCKS5 proxy if configured.
  - [x] Write integration tests using a mock SOCKS5 server.
- [x] **Real ICMP Echo (Ping) Proxying**
  - [x] Extend `SlirpResponse` and `SlirpRequest` to support host-side ICMP sockets.
  - [x] Implement ICMP session tracking in `Slirp` core.
  - [x] Forward guest pings to the host network and return actual replies.
  - [x] Write integration tests for real ICMP proxying.

---

## Feature Compatibility with C-libslirp

This section tracks the feature compatibility between `libslirp-rs` and the original C `libslirp` implementation, highlighting the major progress made.

### L2 Address Resolution
Maps L3 (IP) addresses to L2 (MAC) addresses on the virtual network segment, allowing the guest to resolve the virtual router's MAC.

| Feature | C Implementation | Rust Implementation | Status |
| :--- | :--- | :--- | :--- |
| **ARP Table** | Manages guest MAC-to-IP mappings. | Manages guest MAC-to-IP mappings. | ✅ Complete |
| **ARP Request Handling** | Responds to guest ARP requests for the virtual host. | Responds to guest ARP requests. | ✅ Complete |
| **NDP (IPv6)** | Handles Neighbor Solicitation/Advertisement. | Handles Neighbor Solicitation/Advertisement. | ✅ Complete |

### L3 (IP & ICMP) Forwarding
Core network layer responsible for routing IP packets and handling control messages.

| Feature | C Implementation | Rust Implementation | Status |
| :--- | :--- | :--- | :--- |
| **IPv4 Packet Handling** | Full parsing and forwarding. | Full parsing and forwarding. | ✅ Complete |
| **ICMP Echo (Ping) Reply** | Responds to pings directed at the virtual host. | Responds to pings locally or proxies them. | ✅ Complete |
| **IP Fragmentation** | Can fragment large outgoing packets. | Fully integrated and tested (fragments outgoing packets exceeding MTU). | ✅ Complete |
| **IP Reassembly** | Can reassemble incoming IP fragments. | Fully integrated and tested (reassembles incoming fragments). | ✅ Complete |
| **ICMP Destination Unreachable** | Generates error messages for closed ports/hosts. | Integrated for closed gateway ports (Port Unreachable) and Host Unreachable. | ✅ Complete |
| **ICMP Time Exceeded** | Generates error messages for packets with TTL=0. | Fully integrated and tested (sent when routed packet TTL <= 1). | ✅ Complete |

### IPv6 Support
Full IPv6 networking, NDP, SLAAC, and stateless DHCPv6.

| Feature | C Implementation | Rust Implementation | Status |
| :--- | :--- | :--- | :--- |
| **IPv6 Packet Forwarding** | Full parsing and forwarding of IPv6 traffic. | Fully implemented L3 IPv6 dispatch and routing. | ✅ Complete |
| **ICMPv6 Router Advertisement** | Announces itself as a router on the IPv6 network. | Solicited and periodic RAs with Prefix Info are fully implemented (enables SLAAC). | ✅ Complete |
| **RA RDNSS Option (RFC 8106)** | Announces the DNS server via Router Advertisements. | RDNSS option is fully announced in RAs. | ✅ Complete |
| **DHCPv6** | Stateless DHCPv6 server for network info. | Stateless DHCPv6 server is fully implemented and tested. | ✅ Complete |

### L4 Transport Layer (TCP & UDP NAT)
Network Address Translation (NAT) and connection tracking to connect the guest to the host's network.

| Feature | C Implementation | Rust Implementation | Status |
| :--- | :--- | :--- | :--- |
| **TCP Connection Tracking** | Full state machine for guest connections. | Full state machine implemented. | ✅ Complete |
| **TCP Data Forwarding** | Translates and forwards TCP streams. | Translates and forwards TCP streams. | ✅ Complete |
| **UDP Packet Forwarding** | Translates and forwards UDP datagrams. | Translates and forwards UDP datagrams. | ✅ Complete |
| **Connection Timeouts** | Manages and cleans up idle connections. | Manages SYN, data retransmission (with dynamic RTO), and TIME_WAIT timeouts. | ✅ Complete |

### Advanced TCP & Proxy Features

| Feature | C Implementation | Rust Implementation | Status |
| :--- | :--- | :--- | :--- |
| **Maximum Segment Size (MSS)** | Parses and uses the MSS option. | Parses and uses the MSS option. | ✅ Complete |
| **Congestion Control** | Implements Slow Start, Congestion Avoidance, and Fast Retransmit. | Basic Slow Start and Congestion Avoidance implemented. | ⚠️ Partially Implemented |
| **Window Scale (RFC 1323)** | Supports window scaling for high-bandwidth. | Not Implemented | ❌ Missing |
| **Selective ACKs (SACK)** | Supports SACK for efficient retransmissions. | Not Implemented | ❌ Missing |
| **Upstream SOCKS5 Proxy** | Not supported natively. | Intercepts TCP and performs async SOCKS5 handshake. | ✅ Complete (Rust Extra!) |
| **DNS-over-TCP Proxying** | Not supported natively. | Transparently redirects TCP DNS queries to host DNS. | ✅ Complete (Rust Extra!) |
| **Real ICMP Echo Proxying** | Relies on faked local responder. | Proxies guest pings to the physical network via real host ICMP sockets. | ✅ Complete (Rust Extra!) |

### Application-Level Services
Services built on top of the core networking stack to provide extra functionality.

| Feature | C Implementation | Rust Implementation | Status |
| :--- | :--- | :--- | :--- |
| **DHCPv4** | Fully featured DHCP server (`bootp.c`). | Stateful DHCPv4 server with MAC lease binding and domain options. | ✅ Complete |
| **DNS Forwarding** | Intercepts UDP port 53 and forwards to host. | Intercepts UDP/TCP port 53 and proxies them to host DNS servers. | ✅ Complete |
| **Port Forwarding** | Forwards host ports to the guest. | Implemented via `AcceptIncoming` API for host-initiated TCP. | 🟡 Partial |
| **TFTP Server** | Built-in TFTP server for network booting. | Fully featured async TFTP server with block-by-block retransmission. | ✅ Complete |