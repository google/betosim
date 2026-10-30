// Copyright 2026 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![allow(clippy::unwrap_used)]
#![allow(unreachable_pub)]

//! `packets` is a crate for zero-copy parsing and handling of network packets.
//!
//! It provides structures and utilities for working with various network
//! protocols, including Ethernet, IEEE 802.11, and Netlink messages specific to
//! `mac80211_hwsim`. The crate emphasizes performance by leveraging the
//! `zerocopy` library.
//!
//! # Testing Methodology
//!
//! The `netsim-packets` crate employs a rigorous testing strategy to ensure
//! correctness and compatibility with standard tools.
//!
//! 1. **Golden File Validation**: We use `tshark` (Wireshark) to generate
//!    "golden" JSON output for sample PCAP files. These PCAP files are
//!    typically sourced from public repositories (e.g., Wireshark Wiki) or
//!    generated via standard tools, ensuring they represent real-world traffic.
//!    Our internal tests parse the same PCAP files and verify that our JSON
//!    output matches the structure and content of the `tshark` output.
//!
//! 2. **Frame-by-Frame Tests**: Each protocol module (e.g., `ethernet`, `icmp`,
//!    `transport`) contains a `tests.rs` file that validates parsing and
//!    serialization against specific test data located in
//!    `src/<module>/test_data`. These tests use a shared utility
//!    `utils::test_utils::validate_pcap_json` to automate the comparison.
//!
//! 3. **Unit Tests**: Individual functions and structs have unit tests to
//!    verify edge cases, error handling, and specific logic (e.g., checksum
//!    calculation, header field access).
//!
//! 4. **Integration Tests**: `tests/integration_tests.rs` covers scenarios
//!    involving multiple layers or complex packet interactions.
//!
//! ## Testing with Comparison Keys
//!
//! To ensure robust validation without being overly fragile to insignificant
//! differences (like timestamps or specific capture metadata),
//! our frame-by-frame tests often use a set of **Comparison Keys**.
//!
//! Instead of comparing the entire JSON output, we validate that specific,
//! critical fields match the expected values. This allows us to verify the
//! correctness of the parsing logic for the layers we care about (e.g., 802.11,
//! LLC) while ignoring variability in the outer PCAP/PCAPNG container.
//!
//! ### Example: Layered Packet Keys
//!
//! For a packet containing **Radiotap**, **IEEE 802.11**, and **LLC/SNAP**
//! layers, we might use the following keys for validation:
//!
//! ```rust,ignore
//! let comparison_keys = &[
//!     // Radiotap (if relevant to test)
//!     "radiotap.length",
//!     "radiotap.present.tsft",
//!
//!     // IEEE 802.11
//!     "wlan.fc.type",
//!     "wlan.fc.subtype",
//!     "wlan.ra",
//!     "wlan.ta",
//!
//!     // LLC/SNAP
//!     "llc.dsap",
//!     "llc.ssap",
//!     "llc.control",
//!     "llc.oui",
//!     "llc.type",
//! ];
//! ```
//!
//! # Parsing Architecture
//!
//! This crate leverages `zerocopy` for efficient, zero-allocation parsing where
//! possible.
//!
//! * **Low-Level Parsing**: Structs derive `FromBytes` (from `zerocopy`),
//!   allowing safe casting from byte slices. Example: `let (header, payload) =
//!   IcmpHeader::parse(bytes)?;`
//! * **High-Level Parsing**:
//!     * `packet::parse`: The main entry point for **Ethernet** frames. It
//!       parses the Ethernet header and recursively parses IP and Transport
//!       layers.
//!     * `ieee80211::Ieee80211::decode`: The entry point for **IEEE 802.11**
//!       frames.
//!     * `netlink::stream::NetlinkStream::new`: The entry point for **Netlink**
//!       messages (often found in `NETLINK_ROUTE` or `NETLINK_GENERIC`
//!       sockets).
//!
//! ## Examples
//!
//! ### Parsing an Ethernet Packet
//!
//! ```rust
//! use netsim_packets::{EthernetPacket, parse};
//!
//! let bytes = [ /* raw packet bytes */ ];
//! if let Some(packet) = parse(&bytes) {
//!     let (EthernetPacket::Untagged { frame, .. } | EthernetPacket::Vlan { frame, .. }) =
//!         packet.ethernet;
//!     println!("Ethernet Destination: {}", frame.dst_addr);
//!     if let Some(ip) = &packet.ip {
//!         // Handle IP layer
//!     }
//! }
//! ```
//!
//! ### Parsing a Netlink Message
//!
//! ```rust
//! use netsim_packets::NetlinkStream;
//!
//! let bytes = [ /* raw netlink bytes */ ];
//! for res in NetlinkStream::new(&bytes) {
//!     if let Ok(msg) = res {
//!         let ty = msg.0.nlmsg_type;
//!         println!("Netlink Message Type: {ty}");
//!     }
//!     // Handle attributes...
//! }
//! ```
//!
//! # JSON Serialization Architecture
//!
//! The JSON serialization in this crate is designed to produce output
//! compatible with `tshark -T json`. This facilitates direct comparison between
//! `netsim-packets` parsing and standard Wireshark parsing.
//!
//! 1. **Parsing First**: The raw bytes are first parsed into Rust structs using
//!    `zerocopy` (e.g., `Ieee80211`, `LlcSnapHeader`). This ensures that the
//!    internal representation is strongly typed and validated.
//!
//! 2. **Transformation to JSON Structs**: Each module typically has a `json.rs`
//!    submodule (e.g., `llc::json`, `netlink::nl80211_json`) that defines
//!    `serde`-compatible structs. These structs mirror the `tshark` JSON
//!    schema.
//!     * **Field Naming**: Fields are renamed using `#[serde(rename = "...")]`
//!       to match `tshark` keys (e.g., `wlan.fc.type`).
//!     * **Formatting**: Values are often formatted as strings (hex, decimal)
//!       or specific types to match `tshark`'s idiosyncrasies.
//!
//! 3. **Serialization**: The `serde_json` crate is used to serialize these
//!    intermediate structs into the final JSON string. Helper functions like
//!    `packet_to_json` or `to_json` orchestrate the assembly of multiple layers
//!    (e.g., Radiotap + 802.11 + LLC) into a single JSON object.
//!
//! ## Example: LLC/SNAP JSON
//!
//! The `llc::json` module converts an `LlcSnapHeader` into a `JsonLlc` struct,
//! which serializes to:
//!
//! ```json
//! {
//!   "llc": {
//!     "llc.dsap": 170,
//!     "llc.ssap": 170,
//!     "llc.control": 3,
//!     "llc.oui": "0x000000",
//!     "llc.type": 2048
//!   }
//! }
//! ```
#![allow(missing_docs)]
pub(crate) mod ethernet;
pub(crate) mod hci;
pub(crate) mod icmp;
pub(crate) mod ieee80211;
pub(crate) mod ip;
pub(crate) mod llc;
pub(crate) mod netlink;
pub(crate) mod packet;
pub(crate) mod pcap;
pub(crate) mod transport;
pub(crate) mod utils;

// Facade
pub use ethernet::{
    ArpPacket, ArpPacketBuilder, EthernetFrame, EthernetPacket, MacAddr,
    frame::{arp_hardware, arp_op, ether_type},
};
// hci commands and types
pub use hci::commands::{
    HciCommand, HciCommandHeader, LeClearAdvertisingSets, LeSetAdvertisingData,
    LeSetAdvertisingEnable, LeSetAdvertisingParameters, LeSetEventMask,
    LeSetExtendedAdvertisingData, LeSetExtendedAdvertisingEnable,
    LeSetExtendedAdvertisingParameters, LeSetExtendedScanResponseData, LeSetScanEnable,
    LeSetScanParameters, LeSetScanResponseData, Reset, SetEventMask,
};
pub use hci::{
    events::{HciEvent, LeMetaEvent, parse_hci_event},
    types::{
        Address, AdvertisingEventProperties, AdvertisingFilterPolicy, AdvertisingType, Enable,
        EnabledSet, FragmentPreference, GapDataType, LeAdvertisingEventType, LeScanType,
        LeScanningFilterPolicy, Operation, OwnAddressType, PeerAddressType, PrimaryPhyType,
        SecondaryPhyType,
    },
};
// ICMP and NDP headers/builders
pub use icmp::{
    IcmpEcho, IcmpHeader, IcmpType, Icmpv4UnreachableCode as UnreachableCode,
    Icmpv4UnreachableCode, Icmpv6Echo, Icmpv6Header, Icmpv6ParameterProblemCode,
    Icmpv6TimeExceededCode, Icmpv6Type, Icmpv6UnreachableCode, NDP_HOP_LIMIT,
    NeighborAdvertisement, NeighborAdvertisementBuilder, NeighborSolicitation,
    NeighborSolicitationBuilder, PrefixInformationOption, RdnssOption, RouterAdvertisement,
    RouterAdvertisementBuilder, RouterSolicitation, RouterSolicitationBuilder,
    SourceLinkLayerAddressOption, na_flags, ndp_option,
};
// ieee80211 constants
pub use ieee80211::{
    AKM_PSK, CIPHER_CCMP, EXTENDED_CAPABILITIES, EXTENDED_CAPABILITIES_FTM_RESPONDER_BIT,
    EXTENDED_SUPPORTED_RATES, EXTENSION, HE_CAPABILITIES, RSN_VER, SUPPORTED_RATES_DEFAULT,
};
// ieee80211 types
pub use ieee80211::{
    AssociationRequestFixedFields, AssociationResponseFixedFields, AuthenticationFixedFields,
    BeaconFixedFields, BeaconFrameHeader, CcmpHeader, DataFrameHeader, FrameControl, FrameType,
    Ieee80211ToAp, MacHeader3Addr, SequenceControl,
};
pub use ieee80211::{
    Ieee80211, MacAddress,
    action::{
        ActionHeader, FTM_PARAM_ASAP, FTM_PARAM_LMR_FEEDBACK, FineTimingMeasurement, FtmRequest,
        NanAttributeHeader, NanAttributeIterator, NanNdpHeader, NanSdaHeader,
        VendorSpecificPublicActionHeader, category, dpp, nan, p2p, public_action,
    },
    eapol::{
        EAP_CODE_FAILURE, EAP_CODE_REQUEST, EAP_CODE_RESPONSE, EAP_CODE_SUCCESS, EAP_TYPE_IDENTITY,
        EAPOL_KEY_DESC_TYPE_RSN, EAPOL_TYPE_KEY, EAPOL_TYPE_PACKET, EAPOL_TYPE_START,
        EAPOL_VERSION, EapHeader, EapolHeader, EapolKeyFrame,
    },
    frame::{DataSubType, FrameDirection, management_subtype},
    ie::{IeIterator, set_ext_cap, tags, write_ie},
    wmm::write_wmm_param_element,
};
// IP headers and builders
pub use ip::{
    IP_P_HOPOPTS, IP_P_ICMP, IP_P_ICMPV6, IP_P_TCP, IP_P_UDP, Ipv4Builder, Ipv4Header, Ipv6Builder,
    Ipv6Header, Ipv6HopByHopHeader,
};
pub use llc::frame::{LlcSnapHeader, control_field, sap};
pub use netlink::{
    HwsimAttrSet, HwsimAttrSetBuilder, HwsimFrame, HwsimMsgHdr, Nl80211AttrSetBuilder, NlMsgHdr,
    TxRate, TxRateFlag, attr_id_to_string,
    mac80211_hwsim::{HwsimCmd, HwsimMsg},
    nl80211::attr_id,
    nl80211_attr::NlAttrHdr,
    stream::NetlinkStream,
};
// Transport headers and builders
pub use transport::tcp::flags::{
    ACK as TCP_FLAG_ACK, FIN as TCP_FLAG_FIN, PSH as TCP_FLAG_PSH, RST as TCP_FLAG_RST,
    SYN as TCP_FLAG_SYN, URG as TCP_FLAG_URG,
};
// DHCPv6 and DNS headers/builders
pub mod dhcpv6 {
    pub use crate::transport::{
        DHCPV6_CLIENT_PORT, DHCPV6_SERVER_PORT, Dhcpv6Header, Dhcpv6OptionHeader,
        Dhcpv6OptionIterator, MSG_INFORMATION_REQUEST, MSG_REPLY, OPTION_CLIENTID,
        OPTION_DNS_SERVERS, OPTION_DOMAIN_LIST, OPTION_SERVERID,
    };
}
pub mod dns {
    pub use crate::transport::{
        DNS_PORT, DnsFlags, DnsHeader, DnsPacketBuilder, Opcode, Question, ResourceClass,
        ResourceType, ResponseCode,
    };
}
pub use transport::{
    DHCP_SERVER_PORT, DHCPV6_CLIENT_PORT, DHCPV6_SERVER_PORT, DNS_PORT, Dhcpv6Header,
    Dhcpv6OptionHeader, Dhcpv6OptionIterator, DnsFlags, DnsHeader, DnsPacketBuilder,
    MSG_INFORMATION_REQUEST, MSG_REPLY, OPTION_CLIENTID, OPTION_DNS_SERVERS, OPTION_DOMAIN_LIST,
    OPTION_SERVERID, Opcode, Question, ResourceClass, ResourceType, ResponseCode, TFTP_PORT,
    TcpBuilder, UdpBuilder, UdpPacketBuilder,
};
// Checksum helpers
pub use utils::checksum::{
    icmpv6_checksum, ipv4_checksum, tcp_checksum, tcp_checksum_v6, udp_checksum, udp_checksum_v6,
};

pub mod link_layer {
    #[allow(
        warnings,
        clippy::all,
        clippy::unwrap_in_result,
        clippy::map_err_ignore,
        clippy::indexing_slicing
    )]
    mod pdl_generated {
        include!(concat!(env!("OUT_DIR"), "/link_layer_packets.rs"));
    }
    pub use pdl_generated::*;

    /// Allocation-free, manual byte-inspector designed to bypass full PDL
    /// deserialization overhead. Uses hardcoded offsets derived from
    /// `link_layer_packets.pdl`.
    pub fn fast_inspect_p2p_payload(data_slice: &[u8]) -> bool {
        let Some(&first) = data_slice.first() else {
            return false;
        };
        let Ok(packet_type) = PacketType::try_from(first) else {
            return false;
        };
        match packet_type {
            PacketType::Acl => data_slice.len() > 15,
            PacketType::Sco => data_slice.len() > 13,
            PacketType::LeConnectedIsochronousPdu => data_slice.len() > 17,
            PacketType::LeBroadcastIsochronousPdu => data_slice.len() > 17,
            PacketType::LeLegacyAdvertisingPdu => data_slice.len() > 16,
            PacketType::LeExtendedAdvertisingPdu => data_slice.len() > 22,
            PacketType::LePeriodicAdvertisingPdu => data_slice.len() > 35,
            PacketType::LeScanResponse => data_slice.len() > 14,
            _ => false,
        }
    }

    #[cfg(test)]
    mod tests {
        use pdl_runtime::Packet;

        use super::*;

        #[test]
        fn test_fast_inspect_p2p_payload() {
            let src = Address::try_from(1).unwrap();
            let dest = Address::try_from(2).unwrap();

            // Short packets are ignored
            assert!(!fast_inspect_p2p_payload(&[0u8; 13]));

            // ACL empty PDU (no data) -> false
            let acl_empty = Acl {
                source_address: src,
                destination_address: dest,
                packet_boundary_flag: 0,
                broadcast_flag: 0,
                data: vec![],
            };
            let mut acl_empty_bytes = Vec::new();
            acl_empty.encode(&mut acl_empty_bytes).unwrap();
            assert!(!fast_inspect_p2p_payload(&acl_empty_bytes));

            // ACL payload -> true
            let acl_payload = Acl {
                source_address: src,
                destination_address: dest,
                packet_boundary_flag: 0,
                broadcast_flag: 0,
                data: vec![1, 2, 3],
            };
            let mut acl_payload_bytes = Vec::new();
            acl_payload.encode(&mut acl_payload_bytes).unwrap();
            assert!(fast_inspect_p2p_payload(&acl_payload_bytes));

            // SCO empty PDU -> false
            let sco_empty = Sco { source_address: src, destination_address: dest, payload: vec![] };
            let mut sco_empty_bytes = Vec::new();
            sco_empty.encode(&mut sco_empty_bytes).unwrap();
            assert!(!fast_inspect_p2p_payload(&sco_empty_bytes));

            // SCO payload -> true
            let sco_payload =
                Sco { source_address: src, destination_address: dest, payload: vec![1, 2, 3] };
            let mut sco_payload_bytes = Vec::new();
            sco_payload.encode(&mut sco_payload_bytes).unwrap();
            assert!(fast_inspect_p2p_payload(&sco_payload_bytes));

            // LE Scan -> false regardless of size
            let scan_packet = LeScan {
                source_address: src,
                destination_address: dest,
                scanning_address_type: AddressType::Public,
                advertising_address_type: AddressType::Public,
            };
            let mut scan_packet_bytes = Vec::new();
            scan_packet.encode(&mut scan_packet_bytes).unwrap();
            assert!(!fast_inspect_p2p_payload(&scan_packet_bytes));

            // LE Legacy ADV empty -> false
            let adv_empty = LeLegacyAdvertisingPdu {
                source_address: src,
                destination_address: dest,
                advertising_address_type: AddressType::Public,
                target_address_type: AddressType::Public,
                advertising_type: LegacyAdvertisingType::AdvInd,
                advertising_data: vec![],
            };
            let mut adv_empty_bytes = Vec::new();
            adv_empty.encode(&mut adv_empty_bytes).unwrap();
            assert!(!fast_inspect_p2p_payload(&adv_empty_bytes));

            // LE Legacy ADV payload -> true
            let adv_payload = LeLegacyAdvertisingPdu {
                source_address: src,
                destination_address: dest,
                advertising_address_type: AddressType::Public,
                target_address_type: AddressType::Public,
                advertising_type: LegacyAdvertisingType::AdvInd,
                advertising_data: vec![1, 2, 3],
            };
            let mut adv_payload_bytes = Vec::new();
            adv_payload.encode(&mut adv_payload_bytes).unwrap();
            assert!(fast_inspect_p2p_payload(&adv_payload_bytes));

            // LeConnectedIsochronousPdu empty -> false
            let iso_empty = LeConnectedIsochronousPdu {
                source_address: src,
                destination_address: dest,
                cig_id: 0,
                cis_id: 0,
                sequence_number: 0,
                data: vec![],
            };
            let mut iso_empty_bytes = Vec::new();
            iso_empty.encode(&mut iso_empty_bytes).unwrap();
            assert!(!fast_inspect_p2p_payload(&iso_empty_bytes));

            // LeConnectedIsochronousPdu payload -> true
            let iso_payload = LeConnectedIsochronousPdu {
                source_address: src,
                destination_address: dest,
                cig_id: 0,
                cis_id: 0,
                sequence_number: 0,
                data: vec![1, 2, 3],
            };
            let mut iso_payload_bytes = Vec::new();
            iso_payload.encode(&mut iso_payload_bytes).unwrap();
            assert!(fast_inspect_p2p_payload(&iso_payload_bytes));

            // LeScanResponse empty -> false
            let scan_rsp_empty = LeScanResponse {
                source_address: src,
                destination_address: dest,
                advertising_address_type: AddressType::Public,
                scan_response_data: vec![],
            };
            let mut scan_rsp_empty_bytes = Vec::new();
            scan_rsp_empty.encode(&mut scan_rsp_empty_bytes).unwrap();
            assert!(!fast_inspect_p2p_payload(&scan_rsp_empty_bytes));

            // LeScanResponse payload -> true
            let scan_rsp_payload = LeScanResponse {
                source_address: src,
                destination_address: dest,
                advertising_address_type: AddressType::Public,
                scan_response_data: vec![1, 2, 3],
            };
            let mut scan_rsp_payload_bytes = Vec::new();
            scan_rsp_payload.encode(&mut scan_rsp_payload_bytes).unwrap();
            assert!(fast_inspect_p2p_payload(&scan_rsp_payload_bytes));

            // LeExtendedAdvertisingPdu empty -> false
            let ext_adv_empty = LeExtendedAdvertisingPdu {
                source_address: src,
                destination_address: dest,
                advertising_address_type: AddressType::Public,
                target_address_type: AddressType::Public,
                connectable: 0,
                scannable: 0,
                directed: 0,
                sid: 0,
                tx_power: 0,
                primary_phy: PhyType::Le1m,
                secondary_phy: PhyType::Le1m,
                periodic_advertising_interval: 0,
                advertising_data: vec![],
            };
            let mut ext_adv_empty_bytes = Vec::new();
            ext_adv_empty.encode(&mut ext_adv_empty_bytes).unwrap();
            assert!(!fast_inspect_p2p_payload(&ext_adv_empty_bytes));

            // LeExtendedAdvertisingPdu payload -> true
            let ext_adv_payload = LeExtendedAdvertisingPdu {
                source_address: src,
                destination_address: dest,
                advertising_address_type: AddressType::Public,
                target_address_type: AddressType::Public,
                connectable: 0,
                scannable: 0,
                directed: 0,
                sid: 0,
                tx_power: 0,
                primary_phy: PhyType::Le1m,
                secondary_phy: PhyType::Le1m,
                periodic_advertising_interval: 0,
                advertising_data: vec![1, 2, 3],
            };
            let mut ext_adv_payload_bytes = Vec::new();
            ext_adv_payload.encode(&mut ext_adv_payload_bytes).unwrap();
            assert!(fast_inspect_p2p_payload(&ext_adv_payload_bytes));

            // LeBroadcastIsochronousPdu
            let bis_empty = LeBroadcastIsochronousPdu {
                source_address: src,
                destination_address: dest,
                big_id: 0,
                bis_id: 0,
                sequence_number: 0,
                data: vec![],
            };
            let mut bis_empty_bytes = Vec::new();
            bis_empty.encode(&mut bis_empty_bytes).unwrap();
            assert!(!fast_inspect_p2p_payload(&bis_empty_bytes));

            let bis_payload = LeBroadcastIsochronousPdu {
                source_address: src,
                destination_address: dest,
                big_id: 0,
                bis_id: 0,
                sequence_number: 0,
                data: vec![1, 2, 3],
            };
            let mut bis_payload_bytes = Vec::new();
            bis_payload.encode(&mut bis_payload_bytes).unwrap();
            assert!(fast_inspect_p2p_payload(&bis_payload_bytes));
        }
    }
}
pub use packet::frame::{IpPacket, LlcPacket, Packet, TransportPacket, parse};
pub use pcap::{
    PcapHeader, PcapRecordHeader, create_bredr_bb_packet, create_le_ll_packet,
    ng::InterfaceDescriptionBlock, radiotap::create_radiotap_packet,
};
// Additional types needed by tests or external consumers
pub use transport::tcp::{
    TcpHeader, TcpOptionMss, TcpOptionSackPermitted, TcpOptionWindowScale, option_kind,
};
pub use transport::{
    udp::UdpHeader,
    udp_json::{JsonUdpHeader, to_json},
};
pub use utils::{
    test_utils,
    test_utils::{PacketBuilder, build_arp_frame, build_icmpv6_ns_frame},
};
