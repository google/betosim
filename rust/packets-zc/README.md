# `packets_zc`

`packets_zc` is a crate for zero-copy parsing and handling of network packets.
It provides structures and utilities for working with various network protocols,
leveraging the `zerocopy` library for high-performance, allocation-free parsing
where possible.

## Features

- Zero-copy parsing of packet headers.
- Structures for common network protocols.
- Utility functions for interpreting header fields.
- Optional JSON serialization/deserialization for some protocol headers, often
  mimicking `tshark` field names.

## Supported Protocols and Structures

### Ethernet II Frames

The `ethernet` module defines structures for Ethernet II frames:
- `MacAddr`: Represents a 6-byte MAC address.
- `EthernetFrame`: Represents the 14-byte Ethernet II header (Destination MAC, Source MAC, EtherType).
- EtherType constants are provided in `ethernet::ether_type`.

The `ethernet_util` module provides utility functions like `ethertype_to_string` and MAC address type checks (broadcast, multicast, unicast).
The `ethernet_json` module supports JSON serialization/deserialization of Ethernet headers.

### IEEE 802.11 (Wi-Fi) MAC Headers

The `ieee80211` module provides structures for common IEEE 802.11 MAC headers:
- `FrameControl`: Represents the 2-byte Frame Control field.
- `SequenceControl`: Represents the 2-byte Sequence Control field.
- `MacHeader3Addr`: A generic 24-byte MAC header with 3 address fields, common for many frame types.
- Specific header structures like `DataFrameHeader` and `BeaconFrameHeader`.
- Constants for frame types and subtypes (e.g., `ieee80211::frame_type::DATA`, `ieee80211::management_subtype::BEACON`).

The `ieee80211_util` module offers utilities for interpreting Frame Control fields and determining address roles (RA, TA, DA, SA, BSSID) based on ToDS/FromDS flags.
The `ieee80211_json` module provides `serde`-compatible structures for JSON representation of 802.11 headers.

### IEEE 802.2 LLC and SNAP Headers

The `llc` module provides support for parsing IEEE 802.2 Logical Link Control (LLC) and Subnetwork Access Protocol (SNAP) headers. These headers are often found in Ethernet frames when encapsulating non-IP protocols or when the EtherType field is used as a length field.

-   **`LlcHeader`**: Represents the 3-byte LLC header (DSAP, SSAP, Control).
-   **`SnapHeader`**: Represents the 5-byte SNAP header (OUI, Protocol ID), which typically follows an LLC header when DSAP and SSAP are `0xAA`.
-   **`LlcSnapHeader`**: A combined structure for parsing both LLC and SNAP headers together (8 bytes).
-   Constants for common SAP values (e.g., `llc::sap::SNAP`) and control field values (e.g., `llc::control_field::UI`) are provided.

**Example: Parsing an LLC+SNAP Header**
```rust
use packets_zc::llc::{LlcSnapHeader, sap, control_field};
use packets_zc::ethernet::ether_type; // For example PID

// LLC (SNAP SAPs, UI control) + SNAP (Example OUI, IPv4 PID)
let bytes: [u8; 8] = [
    0xAA,       // DSAP (SNAP)
    0xAA,       // SSAP (SNAP)
    0x03,       // Control (UI)
    0x00, 0x00, 0x0C, // OUI (e.g., Cisco)
    0x08, 0x00, // Protocol ID (IPv4)
];

if let Some((header_ref, _rest)) = LlcSnapHeader::parse(&bytes[..]) {
    assert_eq!(header_ref.llc.dsap, sap::SNAP);
    assert_eq!(header_ref.llc.ssap, sap::SNAP);
    assert_eq!(header_ref.llc.control, control_field::UI);
    assert_eq!(header_ref.snap.oui, [0x00, 0x00, 0x0C]);
    assert_eq!(header_ref.snap.protocol_id.get(), ether_type::IPV4);
}
```

The `llc_util` module offers helper functions, such as converting SAP and control field values to strings.
The `llc_json` module provides `serde`-compatible structures and functions for JSON serialization and deserialization of LLC/SNAP headers, mirroring `tshark` field names.

### `nl80211` Netlink Messages

The `nl80211` module provides structures for parsing Netlink messages specific to the `mac80211_hwsim` kernel module, which is used for simulating Wi-Fi hardware.
- `GenlMsgHdr`: Represents the generic Netlink message header.
- `NlAttrHdr`: Represents the Netlink attribute header.
- Attribute ID constants are defined in `nl80211::attr_id`.

The `nl80211_util` module includes functions for iterating over Netlink attributes, parsing common attribute types (u32, string, MAC address), building Netlink messages, and extracting 802.11 frames from `HWSIM_ATTR_FRAME_DATA` attributes.
The `nl80211_json` module supports JSON representation of Netlink attribute headers and payloads.

### IP (IPv4 and IPv6)

The `ip` module provides structures for both IPv4 and IPv6 headers.
- `Ipv4Header`: Represents the 20-byte IPv4 header.
- `Ipv6Header`: Represents the 40-byte IPv6 header.
- `Ipv6HopByHopHeader`: Represents the IPv6 Hop-by-Hop Options extension header.
- Protocol constants are available (e.g., `ip::IP_P_ICMP`, `ip::IP_P_ICMPV6`).

### Transport Layer Protocols (TCP, UDP, ICMP)

The crate includes modules for common transport layer protocols:
- **`tcp`**: Defines `TcpHeader` for TCP segments. It includes methods for parsing the header and accessing flags (SYN, ACK, FIN, etc.).
- **`udp`**: Defines `UdpHeader` for UDP datagrams.
- **`icmp`**: Defines `IcmpHeader` for ICMP messages and `IcmpEchoHeader` for echo requests/replies.
- **`icmpv6`**: Defines `Icmpv6Header` for ICMPv6 messages.

### Packet Capture (`pcap` and `pcapng`)

The `pcap` and `pcapng` modules provide minimal parsers for the respective packet capture file formats.
- `PcapReader`: A reader that can handle both `pcap` and `pcapng` files.
- `PcapHeader` and `PcapRecordHeader`: Structures for the `pcap` file and record headers.
- `SectionHeaderBlock` and `EnhancedPacketBlock`: Structures for `pcapng` blocks.

### High-Level Packet Parsing

The `packet` module provides a simplified, high-level interface for parsing a byte slice into a structured `Packet` object.
- `parse()`: The main entry point, which takes a raw byte slice and returns an `Option<Packet>`.
- `Packet`: A struct containing the parsed `EthernetPacket` and optional `IpPacket` and `TransportPacket` layers.
- `IpPacket`: An enum that can be `V4` or `V6`.
- `TransportPacket`: An enum for transport layer protocols like `Icmp` and `Icmpv6`.

## Usage

Add `packets_zc` to your `Cargo.toml` (adjust path or version as needed):
```toml
[dependencies]
packets_zc = { path = "../packets-zc" } # Example path
zerocopy = "0.8"
```

Then, use the `parse` function provided by the relevant module:
```rust
use packets_zc::ethernet::{parse, ether_type};

fn process_ethernet_packet(data: &[u8]) {
    if let Some((ethernet_frame, payload)) = parse(data) {
        // Access ethernet_frame fields and payload
        match ethernet_frame.ethertype.get() {
            ether_type::IPV4 => { /* ... */ }
            _ => { /* ... */ }
        }
    }
}
```

For high-level parsing of multiple layers at once, use the top-level `parse` function:
```rust
use packets_zc::packet::{self, IpPacket, TransportPacket};

fn inspect_packet(data: &[u8]) {
    if let Some(p) = packet::parse(data) {
        println!("Parsed Ethernet frame with type: {:?}", p.ethernet.ethertype());
        if let Some(ip) = p.ip {
            match ip {
                IpPacket::V4(hdr, _) => println!("  - IPv4 packet, protocol: {}", hdr.protocol),
                IpPacket::V6(hdr, _) => println!("  - IPv6 packet, next header: {}", hdr.next_header),
            }
        }
        if let Some(transport) = p.transport {
            match transport {
                TransportPacket::Icmp(hdr, _) => println!("    - ICMP type: {}", hdr.icmp_type),
                TransportPacket::Icmpv6(hdr, _) => println!("    - ICMPv6 type: {}", hdr.icmpv6_type),
            }
        }
    }
}
```

The `parse` function returns an `Option<ParseResult<T>>`, where `ParseResult<T>` is a type alias for `(Ref<&[u8], T>, &[u8])`. This tuple contains a `zerocopy` reference to the parsed header and a slice representing the remaining payload.

