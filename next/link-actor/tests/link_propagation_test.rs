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

use std::collections::HashMap;

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

    let mut clients = HashMap::new();
    clients.insert(ChipKind::BLUETOOTH, Box::new(shared_mock) as Box<dyn ChipClient>);

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

    let mut clients = HashMap::new();
    clients.insert(ChipKind::BLUETOOTH, Box::new(shared_mock) as Box<dyn ChipClient>);

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

    let mut clients = HashMap::new();
    clients.insert(ChipKind::BLUETOOTH, Box::new(shared_mock) as Box<dyn ChipClient>);

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

    let mut clients = HashMap::new();
    clients.insert(ChipKind::BLUETOOTH, Box::new(shared_mock) as Box<dyn ChipClient>);

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
