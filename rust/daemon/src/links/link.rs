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

//! Link library

use crate::devices::chip::ChipIdentifier;
use log::info;
use std::collections::HashMap;
use std::sync::RwLock;

/// Wildcard chip ID for global RSSI is defined as 0.
pub const ANY_CHIP: ChipIdentifier = ChipIdentifier(0);

/// Internal representation of Physical Layer Kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhyKind {
    None,
    BluetoothClassic,
    BluetoothLowEnergy,
    Wifi,
    Uwb,
    WifiRtt,
}

/// Internal representation of a Link.
#[derive(Debug, Clone)]
pub struct Link {
    pub sender_id: ChipIdentifier,
    pub receiver_id: ChipIdentifier,
    pub link_kind: PhyKind,
    pub rssi: i8,
}

/// Manages link properties.
pub struct LinkManager {
    rssi: RwLock<HashMap<(ChipIdentifier, ChipIdentifier, PhyKind), i8>>,
}

impl Default for LinkManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LinkManager {
    /// Creates a new LinkManager.
    pub fn new() -> Self {
        LinkManager { rssi: RwLock::new(HashMap::new()) }
    }

    /// List all current links.
    pub fn list(&self) -> Vec<Link> {
        self.rssi
            .read()
            .unwrap()
            .iter()
            .map(|((sender_id, receiver_id, link_kind), rssi)| Link {
                sender_id: *sender_id,
                receiver_id: *receiver_id,
                link_kind: *link_kind,
                rssi: *rssi,
            })
            .collect()
    }

    /// Sets or updates an RSSI between two chips.
    pub fn set_rssi(
        &self,
        sender: ChipIdentifier,
        receiver: ChipIdentifier,
        link_kind: PhyKind,
        rssi: i8,
    ) {
        self.rssi.write().unwrap().insert((sender, receiver, link_kind), rssi);
        info!("Set RSSI between sender {sender} and receiver {receiver} on {link_kind:?}: {rssi}");
    }

    /// Gets the RSSI setting on the specified link if one exists.
    /// This checks for RSSI setting in the following order of precedence:
    /// 1. Exact match: (sender, receiver, phy_kind)
    /// 2. Sender specific, receiver wildcard: (sender, ANY_CHIP, phy_kind)
    /// 3. Receiver specific, sender wildcard: (ANY_CHIP, receiver, phy_kind)
    /// 4. Global wildcard: (ANY_CHIP, ANY_CHIP, phy_kind)
    pub fn get_rssi(
        &self,
        sender: ChipIdentifier,
        receiver: ChipIdentifier,
        link_kind: PhyKind,
    ) -> Option<i8> {
        let map = self.rssi.read().unwrap();

        // Define checks in order of precedence
        let keys_to_check = [
            (sender, receiver, link_kind),   // 1. Exact match
            (sender, ANY_CHIP, link_kind),   // 2. Sender specific, receiver wildcard
            (ANY_CHIP, receiver, link_kind), // 3. Receiver specific, sender wildcard
            (ANY_CHIP, ANY_CHIP, link_kind), // 4. Global wildcard
        ];

        for key in keys_to_check {
            if let Some(rssi_val) = map.get(&key) {
                return Some(*rssi_val);
            }
        }
        None
    }

    /// Deletes an RSSI between two chips for a specific PhyKind.
    /// Returns true if an RSSI was removed, false otherwise.
    pub fn delete_rssi(
        &self,
        sender: ChipIdentifier,
        receiver: ChipIdentifier,
        link_kind: PhyKind,
    ) -> bool {
        let removed = self.rssi.write().unwrap().remove(&(sender, receiver, link_kind)).is_some();
        if removed {
            info!("Deleted RSSI between sender {sender} and receiver {receiver} on {link_kind:?}");
        }
        removed
    }

    /// Delete all link properties associated with a specific chip.
    pub fn delete_link_for_chip(&self, chip_id: ChipIdentifier) {
        let mut rssis = self.rssi.write().unwrap();
        let initial_count = rssis.len();
        rssis.retain(|(sender, receiver, _), _| *sender != chip_id && *receiver != chip_id);
        let removed_count = initial_count - rssis.len();
        if removed_count > 0 {
            info!("Removed {removed_count} RSSI(s) associated with chip {chip_id}");
        }
    }

    /// Reset all link properties.
    pub fn reset(&self) {
        let mut rssis = self.rssi.write().unwrap();
        let initial_count = rssis.len();
        if initial_count > 0 {
            rssis.clear();
            info!("Cleared all {initial_count} RSSI(s).");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_get_rssi() {
        let link_manager = LinkManager::new();

        let sender = ChipIdentifier(1);
        let receiver = ChipIdentifier(2);
        let phy_kind_ble = PhyKind::BluetoothLowEnergy;
        let phy_kind_wifi = PhyKind::Wifi;
        let rssi_ble = -50;
        let rssi_wifi = -60;

        // Initially, no rssi set
        assert_eq!(link_manager.get_rssi(sender, receiver, phy_kind_ble), None);
        assert_eq!(link_manager.get_rssi(sender, receiver, phy_kind_wifi), None);

        // Set BLE RSSI
        link_manager.set_rssi(sender, receiver, phy_kind_ble, rssi_ble);
        assert_eq!(link_manager.get_rssi(sender, receiver, phy_kind_ble), Some(rssi_ble));
        // WiFi RSSI should still be None for this pair with a different PhyKind
        assert_eq!(link_manager.get_rssi(sender, receiver, phy_kind_wifi), None);

        // Set WiFi RSSI for the same pair
        link_manager.set_rssi(sender, receiver, phy_kind_wifi, rssi_wifi);
        assert_eq!(link_manager.get_rssi(sender, receiver, phy_kind_ble), Some(rssi_ble)); // BLE should persist
        assert_eq!(link_manager.get_rssi(sender, receiver, phy_kind_wifi), Some(rssi_wifi));

        // Set an existing value again
        let new_rssi_ble = -55;
        link_manager.set_rssi(sender, receiver, phy_kind_ble, new_rssi_ble);
        assert_eq!(link_manager.get_rssi(sender, receiver, phy_kind_ble), Some(new_rssi_ble));

        // Check non-existent pair
        let other_sender = ChipIdentifier(3);
        assert_eq!(link_manager.get_rssi(other_sender, receiver, phy_kind_ble), None);
    }

    #[test]
    fn test_get_rssi_with_wildcards() {
        let link_manager = LinkManager::new();

        let s1 = ChipIdentifier(1);
        let r1 = ChipIdentifier(2);
        let r2 = ChipIdentifier(3);
        let phy_ble = PhyKind::BluetoothLowEnergy;

        // Precedence: (S,R) > (S,*) > (*,R) > (*,*)
        link_manager.set_rssi(ANY_CHIP, ANY_CHIP, phy_ble, -40); // (*,*)
        assert_eq!(link_manager.get_rssi(s1, r1, phy_ble), Some(-40));
        link_manager.set_rssi(ANY_CHIP, r1, phy_ble, -30); // (*,R1)
        assert_eq!(link_manager.get_rssi(s1, r1, phy_ble), Some(-30));
        link_manager.set_rssi(s1, ANY_CHIP, phy_ble, -20); // (S1,*)
        assert_eq!(link_manager.get_rssi(s1, r1, phy_ble), Some(-20)); // (S1,*) takes precedence over (*,R1) for (S1,R1)
        assert_eq!(link_manager.get_rssi(s1, r2, phy_ble), Some(-20)); // (S1,R2) matches (S1,*)
        link_manager.set_rssi(s1, r1, phy_ble, -10); // (S1,R1) - most specific
        assert_eq!(link_manager.get_rssi(s1, r1, phy_ble), Some(-10));
    }

    #[test]
    fn test_list_links() {
        let link_manager = LinkManager::new();

        assert!(link_manager.list().is_empty(), "Initially, links should be empty");

        let s1 = ChipIdentifier(1);
        let r1 = ChipIdentifier(2);
        let phy1 = PhyKind::BluetoothLowEnergy;
        let rssi1 = -50;
        link_manager.set_rssi(s1, r1, phy1, rssi1);

        let s2 = ChipIdentifier(3);
        let r2 = ChipIdentifier(4);
        let phy2 = PhyKind::Wifi;
        let rssi2 = -60;
        link_manager.set_rssi(s2, r2, phy2, rssi2);

        // Same sender/receiver as the first, but different PhyKind
        let phy3 = PhyKind::Uwb;
        let rssi3 = -70;
        link_manager.set_rssi(s1, r1, phy3, rssi3);

        let links = link_manager.list();
        assert_eq!(links.len(), 3, "Should have 3 links");

        // Order isn't guaranteed, so check for presence of each link
        assert!(
            links.iter().any(|link| link.sender_id == s1
                && link.receiver_id == r1
                && link.link_kind == phy1
                && link.rssi == rssi1),
            "Link 1 (s1,r1,phy1) not found or incorrect"
        );
        assert!(
            links.iter().any(|link| link.sender_id == s2
                && link.receiver_id == r2
                && link.link_kind == phy2
                && link.rssi == rssi2),
            "Link 2 (s2,r2,phy2) not found or incorrect"
        );
        assert!(
            links.iter().any(|link| link.sender_id == s1
                && link.receiver_id == r1
                && link.link_kind == phy3
                && link.rssi == rssi3),
            "Link 3 (s1,r1,phy3) not found or incorrect"
        );
    }

    #[test]
    fn test_delete_rssi() {
        let link_manager = LinkManager::new();

        let sender = ChipIdentifier(1);
        let receiver = ChipIdentifier(2);
        let phy_ble = PhyKind::BluetoothLowEnergy;
        let phy_wifi = PhyKind::Wifi;
        let rssi_ble = -50;
        let rssi_wifi = -60;

        link_manager.set_rssi(sender, receiver, phy_ble, rssi_ble);
        link_manager.set_rssi(sender, receiver, phy_wifi, rssi_wifi);
        link_manager.set_rssi(ChipIdentifier(3), receiver, phy_ble, -70); // Another link

        link_manager.set_rssi(sender, ANY_CHIP, phy_ble, -80); // Wildcard link for sender

        assert_eq!(link_manager.list().len(), 4, "Should have 4 links initially");
        assert_eq!(
            link_manager.get_rssi(sender, ChipIdentifier(99), phy_ble),
            Some(-80),
            "Wildcard (S,*) should apply"
        );

        assert_eq!(link_manager.get_rssi(sender, receiver, phy_ble), Some(rssi_ble));
        assert_eq!(link_manager.get_rssi(sender, receiver, phy_wifi), Some(rssi_wifi));

        // Try to delete a non-existent sender/receiver pair for a specific PhyKind
        assert!(!link_manager.delete_rssi(ChipIdentifier(10), ChipIdentifier(11), phy_ble));
        assert_eq!(
            link_manager.list().len(),
            4,
            "Deleting non-existent pair should not change link count"
        );

        // Try to delete for a PhyKind that doesn't exist for this pair
        assert!(!link_manager.delete_rssi(sender, receiver, PhyKind::Uwb));
        assert_eq!(
            link_manager.list().len(),
            4,
            "Deleting non-existent PhyKind for existing pair should not change link count"
        );

        // Delete the BLE RSSI for (sender, receiver). WiFi RSSI should remain.
        assert!(
            link_manager.delete_rssi(sender, receiver, phy_ble),
            "Deletion of existing BLE RSSI should return true"
        );
        assert_eq!(
            link_manager.get_rssi(sender, receiver, phy_ble),
            Some(-80),
            "BLE RSSI should now fall back to (S,*)"
        );
        assert_eq!(
            link_manager.get_rssi(sender, receiver, phy_wifi),
            Some(rssi_wifi),
            "WiFi RSSI should still exist"
        );
        assert_eq!(
            link_manager.list().len(),
            3,
            "Remaining links: (S,R,WiFi), (3,R,BLE), (S,*,BLE)"
        );

        // Try deleting the BLE RSSI again, should return false as it's already removed
        assert!(
            !link_manager.delete_rssi(sender, receiver, phy_ble),
            "Deleting already removed BLE RSSI should return false"
        );

        // Delete the WiFi RSSI
        assert!(
            link_manager.delete_rssi(sender, receiver, phy_wifi),
            "Deletion of existing WiFi RSSI should return true"
        );
        assert_eq!(
            link_manager.get_rssi(sender, receiver, phy_wifi),
            None,
            "WiFi RSSI should be gone"
        );
        assert_eq!(link_manager.list().len(), 2, "Remaining links: (3,R,BLE), (S,*,BLE)");

        // Delete the wildcard RSSI (S, *, BLE)
        assert!(
            link_manager.delete_rssi(sender, ANY_CHIP, phy_ble),
            "Deleting (S,*,BLE) should return true"
        );
        assert_eq!(
            link_manager.get_rssi(sender, ChipIdentifier(99), phy_ble),
            None,
            "(S,*) RSSI should be gone"
        );
        // If a global (*,*) existed, it would be picked up here. Since it doesn't:
        assert_eq!(
            link_manager.get_rssi(sender, receiver, phy_ble),
            None,
            "No BLE RSSI should remain for (S,R)"
        );
        assert_eq!(link_manager.list().len(), 1, "Only the (3,R,BLE) link should remain");
    }

    #[test]
    fn test_delete_link_for_chip() {
        let link_manager = LinkManager::new();

        let chip1 = ChipIdentifier(1);
        let chip2 = ChipIdentifier(2);
        let chip3 = ChipIdentifier(3);
        let phy_ble = PhyKind::BluetoothLowEnergy;
        let phy_wifi = PhyKind::Wifi;

        // Set up some RSSIs
        link_manager.set_rssi(chip1, chip2, phy_ble, -50); // Involves chip1 (sender)
        link_manager.set_rssi(chip2, chip1, phy_wifi, -55); // Involves chip1 (receiver)
        link_manager.set_rssi(chip1, chip3, phy_ble, -60); // Involves chip1 (sender)
        link_manager.set_rssi(chip3, chip1, phy_wifi, -65); // Involves chip1 (receiver)
        link_manager.set_rssi(chip2, chip3, phy_ble, -70); // Does NOT involve chip1 (C2->C3)
        link_manager.set_rssi(ANY_CHIP, chip1, phy_ble, -75); // (*->C1) Involves chip1
        link_manager.set_rssi(chip1, ANY_CHIP, phy_wifi, -80); // (C1->*) Involves chip1
        link_manager.set_rssi(ANY_CHIP, ANY_CHIP, phy_ble, -85); // (*->*) Does NOT involve chip1 specifically

        assert_eq!(link_manager.list().len(), 8, "Initial number of links should be 8");

        link_manager.delete_link_for_chip(chip1);

        let remaining_links = link_manager.list();
        assert_eq!(remaining_links.len(), 2, "Expected (C2->C3) and (*,*) to remain");

        assert!(
            remaining_links.iter().any(|link| link.sender_id == chip2
                && link.receiver_id == chip3
                && link.link_kind == phy_ble
                && link.rssi == -70),
            "Link (C2->C3, BLE, -70) not found in remaining links: {remaining_links:?}"
        );

        assert!(
            remaining_links.iter().any(|link| link.sender_id == ANY_CHIP
                && link.receiver_id == ANY_CHIP
                && link.link_kind == phy_ble
                && link.rssi == -85),
            "Global wildcard link (*,*, BLE, -85) not found in remaining links: {remaining_links:?}"
        );
        assert_eq!(
            link_manager.get_rssi(ChipIdentifier(98), ChipIdentifier(99), phy_ble),
            Some(-85),
            "Global wildcard should be active"
        );

        assert_eq!(link_manager.get_rssi(chip1, chip2, phy_ble), Some(-85)); // Fallback to global wildcard entry
        assert_eq!(link_manager.get_rssi(chip2, chip1, phy_wifi), None);
        assert_eq!(link_manager.get_rssi(chip1, chip3, phy_ble), Some(-85)); // Fallback to global wildcard entry
        assert_eq!(link_manager.get_rssi(chip3, chip1, phy_wifi), None);
        assert_eq!(
            link_manager.get_rssi(ANY_CHIP, chip1, phy_ble),
            Some(-85),
            "(*->C1) should be gone"
        );
        assert_eq!(
            link_manager.get_rssi(chip1, ANY_CHIP, phy_wifi),
            None,
            "(C1->*) should be gone"
        );

        assert_eq!(link_manager.get_rssi(chip2, chip3, phy_ble), Some(-70));

        link_manager.delete_link_for_chip(ChipIdentifier(99));
        assert_eq!(
            link_manager.list().len(),
            2,
            "Removing RSSI's for a chip not involved should not change link count (should be 2: C2->C3 and *,*)"
        );

        link_manager.reset();
        link_manager.set_rssi(chip1, chip2, phy_ble, -50);
        link_manager.set_rssi(chip1, ANY_CHIP, phy_ble, -55);
        link_manager.set_rssi(ANY_CHIP, chip2, phy_ble, -60);
        link_manager.set_rssi(ANY_CHIP, ANY_CHIP, phy_ble, -65);
        assert_eq!(link_manager.list().len(), 4);

        link_manager.delete_link_for_chip(ANY_CHIP);
        assert_eq!(link_manager.get_rssi(chip1, chip2, phy_ble), Some(-50));
    }

    #[test]
    fn test_reset() {
        let link_manager = LinkManager::new();

        let sender = ChipIdentifier(1);
        let receiver = ChipIdentifier(2);
        let phy_kind_ble = PhyKind::BluetoothLowEnergy;
        let phy_kind_wifi = PhyKind::Wifi;

        link_manager.set_rssi(sender, receiver, phy_kind_ble, -50);
        link_manager.set_rssi(ChipIdentifier(3), ChipIdentifier(4), phy_kind_wifi, -60);
        assert!(!link_manager.list().is_empty(), "Links should exist before clearing");

        link_manager.reset();
        assert!(link_manager.list().is_empty(), "All links should be cleared");
        assert_eq!(
            link_manager.get_rssi(sender, receiver, phy_kind_ble),
            None,
            "Specific RSSI should be gone after clear"
        );
        assert_eq!(
            link_manager.get_rssi(ChipIdentifier(3), ChipIdentifier(4), phy_kind_wifi),
            None,
            "Another specific RSSI should be gone"
        );

        link_manager.reset();
        assert!(
            link_manager.list().is_empty(),
            "Links should remain empty after clearing an already empty set"
        );
    }
}
