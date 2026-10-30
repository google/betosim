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

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use netsim_model::{ChipClient, ChipId, ChipKind, MockChipClient};

use crate::world::World;

// Feature: Link Propagation
//
//   As a system
//   I want link updates to be propagated to the chips
//   So that they are aware of their connections

// Helper to configure common mock expectations
fn configure_base_mock(mock: &mut MockChipClient) {
    mock.expect_read().returning(|_| Ok(netsim_model::Chip::default()));
}

// Scenario: Link updates are propagated to chip clients for creation and
// deletion
//
//   Given a mock chip client expecting updates
//   When a link is created and then deleted
//   Then the chip client receives updates reflecting these changes
#[tokio::test]
async fn test_link_propagation() {
    // Given
    let mut shared_mock = MockChipClient::new();
    let chip_id = ChipId(1);
    let peer_id = ChipId(2);

    configure_base_mock(&mut shared_mock);

    shared_mock
        .expect_update()
        .withf(move |id, patch| {
            // Creation: Update contains link from 1 to 2, and ONLY that link
            *id == ChipId(1)
                && patch.links.is_some()
                && patch.links.as_ref().unwrap().len() == 1
                && patch.links.as_ref().unwrap().contains(&(ChipId(2), -50))
        })
        .times(1)
        .returning(|_, _| Ok(netsim_model::Chip::default()));

    shared_mock
        .expect_update()
        .withf(move |id, patch| {
            // Deletion: Update contains NO links
            *id == ChipId(1) && patch.links.is_some() && patch.links.as_ref().unwrap().is_empty()
        })
        .times(1)
        .returning(|_, _| Ok(netsim_model::Chip::default()));

    let clients =
        HashMap::from([(ChipKind::BLUETOOTH, Box::new(shared_mock) as Box<dyn ChipClient>)]);

    let world = World::with_clients(clients).await;

    world.when_notify_chip_added(chip_id, ChipKind::BLUETOOTH).await;
    world.when_notify_chip_added(peer_id, ChipKind::BLUETOOTH).await;

    // When Create
    let link_id = world.when_create_link(chip_id, peer_id, -50).await.unwrap();

    // When Delete
    world.client.delete(link_id).await.unwrap();

    // Then
    // Verification happens on drop of `mock_client` inside the actor task
}

// Scenario: Link update (RSSI) is propagated to chip clients
//
//   Given a link exists
//   When the link RSSI is updated
//   Then the chip client receives an update with the new RSSI
#[tokio::test]
async fn test_update_link_propagates_patch() {
    // Given
    let mut shared_mock = MockChipClient::new();
    let chip_id = ChipId(1);
    let peer_id = ChipId(2);

    configure_base_mock(&mut shared_mock);

    // 1. Expectation for Creation
    shared_mock
        .expect_update()
        .withf(move |id, patch| {
            *id == ChipId(1)
                && patch.links.is_some()
                && patch.links.as_ref().unwrap().len() == 1
                && patch.links.as_ref().unwrap().contains(&(ChipId(2), -50))
        })
        .times(1)
        .returning(|_, _| Ok(netsim_model::Chip::default()));

    // 2. Expectation for Update
    shared_mock
        .expect_update()
        .withf(move |id, patch| {
            // Verify update contains link with new RSSI -70, and nothing else
            *id == ChipId(1)
                && patch.links.is_some()
                && patch.links.as_ref().unwrap().len() == 1
                && patch.links.as_ref().unwrap().contains(&(ChipId(2), -70))
        })
        .times(1)
        .returning(|_, _| Ok(netsim_model::Chip::default()));

    let clients =
        HashMap::from([(ChipKind::BLUETOOTH, Box::new(shared_mock) as Box<dyn ChipClient>)]);

    let world = World::with_clients(clients).await;

    world.when_notify_chip_added(chip_id, ChipKind::BLUETOOTH).await;
    world.when_notify_chip_added(peer_id, ChipKind::BLUETOOTH).await;
    let link_id = world.when_create_link(chip_id, peer_id, -50).await.unwrap();

    // When
    world.when_update_link(link_id, -70).await.unwrap();

    // Then
    // Verification happens on drop
}

// Scenario: Chip removal triggers link deletion patch to peer
//
//   Given a link exists between two chips
//   When one chip is removed
//   Then the peer chip receives an update removing the link
#[tokio::test]
async fn test_chip_removal_propagates_patch() {
    // Given
    let mut shared_mock = MockChipClient::new();
    let chip_removed = ChipId(1);
    let chip_peer = ChipId(2);

    configure_base_mock(&mut shared_mock);

    // Expectation 1: Chip 1 gets link update (Creation)
    shared_mock
        .expect_update()
        .withf(move |id, patch| {
            *id == ChipId(1)
                && patch.links.is_some()
                && patch.links.as_ref().unwrap().len() == 1
                && patch.links.as_ref().unwrap().contains(&(ChipId(2), -50))
        })
        .times(1)
        .returning(|_, _| Ok(netsim_model::Chip::default()));

    // Expectation 2: Peer (Chip 2) gets link update (Creation)
    shared_mock
        .expect_update()
        .withf(move |id, patch| {
            *id == ChipId(2)
                && patch.links.is_some()
                && patch.links.as_ref().unwrap().len() == 1
                && patch.links.as_ref().unwrap().contains(&(ChipId(1), -50))
        })
        .times(1)
        .returning(|_, _| Ok(netsim_model::Chip::default()));

    // Expectation 3: Peer (Chip 2) gets link removal update (When Chip 1 is
    // removed)
    shared_mock
        .expect_update()
        .withf(move |id, patch| {
            if *id == ChipId(2) && patch.links.is_some() {
                patch.links.as_ref().unwrap().is_empty()
            } else {
                false
            }
        })
        .times(1)
        .returning(|_, _| Ok(netsim_model::Chip::default()));

    let clients =
        HashMap::from([(ChipKind::BLUETOOTH, Box::new(shared_mock) as Box<dyn ChipClient>)]);

    let world = World::with_clients(clients).await;

    world.when_notify_chip_added(chip_removed, ChipKind::BLUETOOTH).await;
    world.when_notify_chip_added(chip_peer, ChipKind::BLUETOOTH).await;

    // Create link (triggers updates to both)
    world.when_create_link(chip_removed, chip_peer, -50).await.unwrap();

    // When
    world.when_notify_chip_removed(chip_removed).await;

    // Then
    // Verification happens on drop
}

// Scenario: Reset notifies downstream chip clients of cleared links
//
//   Given an active link exists
//   When a reset action occurs
//   Then downstream chip clients receive an update with an empty link list
#[tokio::test]
async fn test_reset_propagates_link_removal_to_chips() {
    // Given
    let mut shared_mock = MockChipClient::new();
    let chip_sender = ChipId(1);
    let chip_peer = ChipId(2);

    configure_base_mock(&mut shared_mock);

    // Expectation 1: Creation updates sender (Chip 1)
    shared_mock
        .expect_update()
        .withf(move |id, patch| {
            *id == ChipId(1)
                && patch.links.is_some()
                && patch.links.as_ref().unwrap().len() == 1
                && patch.links.as_ref().unwrap().contains(&(ChipId(2), -50))
        })
        .times(1)
        .returning(|_, _| Ok(netsim_model::Chip::default()));

    // Expectation 2: Creation updates peer (Chip 2)
    shared_mock
        .expect_update()
        .withf(move |id, patch| {
            *id == ChipId(2)
                && patch.links.is_some()
                && patch.links.as_ref().unwrap().len() == 1
                && patch.links.as_ref().unwrap().contains(&(ChipId(1), -55))
        })
        .times(1)
        .returning(|_, _| Ok(netsim_model::Chip::default()));

    // Expectation 3: Reset clears links on sender (Chip 1)
    shared_mock
        .expect_update()
        .withf(move |id, patch| {
            *id == ChipId(1) && patch.links.is_some() && patch.links.as_ref().unwrap().is_empty()
        })
        .times(1)
        .returning(|_, _| Ok(netsim_model::Chip::default()));

    // Expectation 4: Reset clears links on peer (Chip 2)
    shared_mock
        .expect_update()
        .withf(move |id, patch| {
            *id == ChipId(2) && patch.links.is_some() && patch.links.as_ref().unwrap().is_empty()
        })
        .times(1)
        .returning(|_, _| Ok(netsim_model::Chip::default()));

    let clients =
        HashMap::from([(ChipKind::BLUETOOTH, Box::new(shared_mock) as Box<dyn ChipClient>)]);

    let world = World::with_clients(clients).await;

    world.when_notify_chip_added(chip_sender, ChipKind::BLUETOOTH).await;
    world.when_notify_chip_added(chip_peer, ChipKind::BLUETOOTH).await;

    world.when_create_link(chip_sender, chip_peer, -50).await.unwrap();
    world.when_create_link(chip_peer, chip_sender, -55).await.unwrap();

    // When
    link_api::LinkClient::reset(&world.client).await.unwrap();

    // Then
    // Verification happens on drop
}

type ChipUpdateRecord = (ChipId, Option<Vec<(ChipId, i8)>>);

async fn recording_world() -> (World, Arc<Mutex<Vec<ChipUpdateRecord>>>) {
    let mut shared_mock = MockChipClient::new();
    configure_base_mock(&mut shared_mock);
    let recorded_updates = Arc::new(Mutex::new(Vec::new()));
    let updates_clone = Arc::clone(&recorded_updates);
    shared_mock.expect_update().returning(move |id, patch| {
        updates_clone.lock().unwrap().push((id, patch.links));
        Ok(netsim_model::Chip::default())
    });
    let clients =
        HashMap::from([(ChipKind::BLUETOOTH, Box::new(shared_mock) as Box<dyn ChipClient>)]);
    let world = World::with_clients(clients).await;
    (world, recorded_updates)
}

// Scenario: Asymmetric bidirectional links propagate updates only to modified sender
//
//   Given two chips with bidirectional links having different RSSIs
//   When one link's RSSI is updated
//   Then only the sender of the updated link receives a new patch
#[tokio::test]
async fn test_asymmetric_bidirectional_links() {
    // Given
    let (world, recorded_updates) = recording_world().await;
    let chip_a = ChipId(1);
    let chip_b = ChipId(2);

    world.when_notify_chip_added(chip_a, ChipKind::BLUETOOTH).await;
    world.when_notify_chip_added(chip_b, ChipKind::BLUETOOTH).await;

    let link_a_b = world.when_create_link(chip_a, chip_b, -50).await.unwrap();
    world.when_create_link(chip_b, chip_a, -85).await.unwrap();

    // When
    world.when_update_link(link_a_b, -35).await.unwrap();

    // Then
    let recorded = recorded_updates.lock().unwrap();
    assert_eq!(
        *recorded,
        vec![
            (chip_a, Some(vec![(chip_b, -50)])),
            (chip_b, Some(vec![(chip_a, -85)])),
            (chip_a, Some(vec![(chip_b, -35)])),
        ]
    );
}

// Scenario: Removing central chip in star topology cascades link pruning to peripherals
//
//   Given a central chip connected bidirectionally to three peripheral chips
//   When the central chip is removed
//   Then each peripheral receives an empty links patch, dead chip receives 0 updates,
//   and all 6 links are pruned from the world
#[tokio::test]
async fn test_star_topology_chip_removal_cascade() {
    // Given
    let (world, recorded_updates) = recording_world().await;
    let center = ChipId(1);
    let peripheral_links = [(ChipId(2), -50), (ChipId(3), -55), (ChipId(4), -60)];
    let peripherals = peripheral_links.map(|(chip, _)| chip);

    world.when_notify_chip_added(center, ChipKind::BLUETOOTH).await;
    for peripheral in peripherals {
        world.when_notify_chip_added(peripheral, ChipKind::BLUETOOTH).await;
    }

    for (peripheral, rssi) in peripheral_links {
        world.when_create_link(center, peripheral, rssi).await.unwrap();
        world.when_create_link(peripheral, center, rssi).await.unwrap();
    }

    assert_eq!(world.client.list().await.unwrap().len(), 6);
    {
        let setup_updates = recorded_updates.lock().unwrap();
        for (peripheral, rssi) in peripheral_links {
            let (_, patch) = setup_updates
                .iter()
                .find(|(id, _)| *id == peripheral)
                .expect("peripheral update must exist");
            assert_eq!(patch, &Some(vec![(center, rssi)]));
        }
    }

    // When
    let update_count_before_removal = recorded_updates.lock().unwrap().len();
    world.when_notify_chip_removed(center).await;

    // Then
    let remaining_links = world.client.list().await.unwrap();
    assert!(remaining_links.is_empty(), "All 6 links must be pruned from the world");

    let recorded = recorded_updates.lock().unwrap();
    let removal_updates = &recorded[update_count_before_removal..];
    assert_eq!(removal_updates.len(), 3);
    assert!(
        !removal_updates.iter().any(|(id, _)| *id == center),
        "Dead center chip must receive zero updates on removal"
    );
    for peripheral in peripherals {
        assert!(
            removal_updates.contains(&(peripheral, Some(vec![]))),
            "Peripheral {peripheral:?} must receive an update with empty links"
        );
    }
}
