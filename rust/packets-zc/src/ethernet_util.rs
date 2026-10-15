// Copyright 2025 The Android Open Source Project
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

//! Provides utility functions for working with Ethernet-related data structures.

use crate::ethernet::{MacAddr, ether_type};

/// Converts an EtherType value to a human-readable string.
///
/// If the EtherType is unknown, it returns the hex representation of the value.
///
/// # Arguments
/// * `ethertype_val` - The EtherType value (e.g., `ether_type::IPV4`).
///
/// # Examples
/// ```
/// use packets_zc::ethernet::ether_type;
/// use packets_zc::ethernet_util::ethertype_to_string;
///
/// assert_eq!(ethertype_to_string(ether_type::IPV4), "IPv4");
/// assert_eq!(ethertype_to_string(ether_type::ARP), "ARP");
/// assert_eq!(ethertype_to_string(0x1234), "0x1234");
/// ```
pub fn ethertype_to_string(ethertype_val: u16) -> String {
    match ethertype_val {
        ether_type::IPV4 => "IPv4".to_string(),
        ether_type::ARP => "ARP".to_string(),
        ether_type::IPV6 => "IPv6".to_string(),
        ether_type::VLAN => "VLAN".to_string(),
        _ => format!("0x{:04X}", ethertype_val),
    }
}

/// The broadcast MAC address (FF:FF:FF:FF:FF:FF).
pub const BROADCAST_MAC_ADDR: MacAddr = MacAddr { bytes: [0xFF; 6] };

/// Checks if a MAC address is the broadcast address (FF:FF:FF:FF:FF:FF).
///
/// # Arguments
/// * `mac` - A reference to a `MacAddr`.
pub fn is_broadcast_mac(mac: &MacAddr) -> bool {
    mac == &BROADCAST_MAC_ADDR
}

/// Checks if a MAC address is a multicast address.
///
/// A MAC address is multicast if the least significant bit of its first octet is set to 1.
/// This also includes the broadcast address.
///
/// # Arguments
/// * `mac` - A reference to a `MacAddr`.
pub fn is_multicast_mac(mac: &MacAddr) -> bool {
    (mac.bytes[0] & 0x01) != 0
}

/// Checks if a MAC address is a unicast address.
///
/// A MAC address is unicast if it's not multicast (and therefore not broadcast).
///
/// # Arguments
/// * `mac` - A reference to a `MacAddr`.
pub fn is_unicast_mac(mac: &MacAddr) -> bool {
    !is_multicast_mac(mac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethertype_strings() {
        assert_eq!(ethertype_to_string(ether_type::IPV4), "IPv4");
        assert_eq!(ethertype_to_string(ether_type::ARP), "ARP");
        assert_eq!(ethertype_to_string(ether_type::IPV6), "IPv6");
        assert_eq!(ethertype_to_string(ether_type::VLAN), "VLAN");
        assert_eq!(ethertype_to_string(0x9000), "0x9000"); // Example of an IEE 802.3 EtherType
    }

    #[test]
    fn test_mac_address_types() {
        let broadcast = BROADCAST_MAC_ADDR;
        let multicast = MacAddr::new([0x01, 0x00, 0x5E, 0x00, 0x00, 0x01]); // Example multicast
        let unicast = MacAddr::new([0x00, 0x00, 0x5E, 0x00, 0x00, 0x01]); // Example unicast

        assert!(is_broadcast_mac(&broadcast));
        assert!(is_multicast_mac(&broadcast)); // Broadcast is a form of multicast
        assert!(!is_unicast_mac(&broadcast));

        assert!(!is_broadcast_mac(&multicast));
        assert!(is_multicast_mac(&multicast));
        assert!(!is_unicast_mac(&multicast));

        assert!(!is_broadcast_mac(&unicast));
        assert!(!is_multicast_mac(&unicast));
        assert!(is_unicast_mac(&unicast));
    }
}
