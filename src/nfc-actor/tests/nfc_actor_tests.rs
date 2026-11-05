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

use actor_framework::{ActorLifecycle, ActorService};
use bytes::Bytes;
use netsim_model::{ChipId, DeviceId};
use nfc_actor::NfcActor;
use tokio::sync::mpsc;

use crate::world::*;

// --- Tests ---

// Feature: Casimir Disconnect Cleanup
// Scenario: Casimir disconnects and triggers clean EOF
//
//   Given a new NFC world with a registered chip
//   When Casimir disconnects the device
//   Then the chip is cleanly removed and tasks are aborted
#[tokio::test]
async fn test_casimir_eof_cleanup() {
    // Given
    let mut world = NfcWorld::new().await;
    let chip_id = 1;
    world.given_a_chip(chip_id).await;

    // When
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        world.when_casimir_disconnects(chip_id).await;
    })
    .await;
    assert!(timeout.is_ok(), "Test timed out! Possible hang detected.");

    // Then
    world.then_chip_is_cleaned_up(chip_id).await;
}

// Feature: NFC Enablement Gating
// Scenario: Chips can be dynamically enabled/disabled via handle_update
//
//   Given a new NFC world with a registered chip
//   When handle_update sets enabled = false
//   Then the chip state transitions to disabled
//   When handle_update sets enabled = true
//   Then the chip state transitions back to enabled
#[tokio::test]
async fn test_nfc_enablement_toggle() {
    // Given
    let mut world = NfcWorld::new().await;
    let chip_id = 1;
    world.given_a_chip(chip_id).await;

    // When (Android OS calls NfcAdapter.disable())
    let update_disable = netsim_model::ChipUpdate { enabled: Some(false), ..Default::default() };
    let update_res1 =
        world.actor.handle_update(ChipId(chip_id), update_disable, &mut world.ctx).await;
    assert!(update_res1.is_ok());

    // Then (Verify chip is now disabled)
    let state_disabled =
        world.actor.handle_get(ChipId(chip_id), &mut world.ctx).await.unwrap().unwrap();
    assert!(!state_disabled.enabled, "Chip should be disabled after update!");

    // When (Android OS calls NfcAdapter.enable())
    let update_enable = netsim_model::ChipUpdate { enabled: Some(true), ..Default::default() };
    let update_res2 =
        world.actor.handle_update(ChipId(chip_id), update_enable, &mut world.ctx).await;
    assert!(update_res2.is_ok());

    // Then (Verify chip is enabled again)
    let state_enabled_again =
        world.actor.handle_get(ChipId(chip_id), &mut world.ctx).await.unwrap().unwrap();
    assert!(state_enabled_again.enabled, "Chip should be enabled after second update!");
}

// variant update path
//
// Given a new NFC world with a registered chip
// When handle_update sets state via variant = Some(false)
// Then the chip state transitions to disabled
// When handle_update sets state via variant = Some(true)
// Then the chip state transitions back to enabled
#[tokio::test]
async fn test_nfc_update_via_variant() {
    // Given
    let mut world = NfcWorld::new().await;
    let chip_id = 1;
    world.given_a_chip(chip_id).await;

    // When (Update via variant field to disable along with metadata)
    let update_disable = netsim_model::ChipUpdate {
        name: Some("custom-nfc-1".to_string()),
        manufacturer: Some("TestMfg".to_string()),
        product_name: Some("TestProd".to_string()),
        variant: Some(netsim_model::ChipVariantUpdate::Nfc(netsim_model::NfcUpdate {
            radio: netsim_model::RadioUpdate { state: Some(false) },
        })),
        ..Default::default()
    };
    let update_res1 =
        world.actor.handle_update(ChipId(chip_id), update_disable, &mut world.ctx).await;
    assert!(update_res1.is_ok());

    // Then (Verify chip is now disabled and metadata is applied)
    let state_disabled =
        world.actor.handle_get(ChipId(chip_id), &mut world.ctx).await.unwrap().unwrap();
    assert!(!state_disabled.enabled, "Chip should be disabled after variant update!");
    assert_eq!(state_disabled.name, "custom-nfc-1");
    assert_eq!(state_disabled.manufacturer, "TestMfg");
    assert_eq!(state_disabled.product_name, "TestProd");

    // When (Update via variant field to enable)
    let update_enable = netsim_model::ChipUpdate {
        variant: Some(netsim_model::ChipVariantUpdate::Nfc(netsim_model::NfcUpdate {
            radio: netsim_model::RadioUpdate { state: Some(true) },
        })),
        ..Default::default()
    };
    let update_res2 =
        world.actor.handle_update(ChipId(chip_id), update_enable, &mut world.ctx).await;
    assert!(update_res2.is_ok());

    // Then (Verify chip is enabled again and metadata is preserved)
    let state_enabled_again =
        world.actor.handle_get(ChipId(chip_id), &mut world.ctx).await.unwrap().unwrap();
    assert!(state_enabled_again.enabled, "Chip should be enabled after second variant update!");
    assert_eq!(state_enabled_again.name, "custom-nfc-1");
    assert_eq!(state_enabled_again.manufacturer, "TestMfg");
    assert_eq!(state_enabled_again.product_name, "TestProd");

    // And handle_list reflects the updated chip metadata and NFC radio state
    let chips = world.actor.handle_list(&mut world.ctx).await.unwrap();
    assert_eq!(chips.len(), 1);
    assert_eq!(chips[0].name, "custom-nfc-1");
    assert_eq!(chips[0].manufacturer, "TestMfg");
    assert_eq!(chips[0].product_name, "TestProd");
    assert_eq!(
        chips[0].variant,
        Some(netsim_model::ChipVariant::Nfc(netsim_model::Nfc {
            radio: netsim_model::Radio { state: Some(true), ..Default::default() },
        }))
    );

    // When both enabled and variant are provided, variant takes precedence
    let update_precedence = netsim_model::ChipUpdate {
        enabled: Some(true),
        variant: Some(netsim_model::ChipVariantUpdate::Nfc(netsim_model::NfcUpdate {
            radio: netsim_model::RadioUpdate { state: Some(false) },
        })),
        ..Default::default()
    };
    let state_precedence = world
        .actor
        .handle_update(ChipId(chip_id), update_precedence, &mut world.ctx)
        .await
        .unwrap();
    assert!(!state_precedence.enabled);
    assert_eq!(
        state_precedence.variant,
        Some(netsim_model::ChipVariant::Nfc(netsim_model::Nfc {
            radio: netsim_model::Radio { state: Some(false), ..Default::default() },
        }))
    );
}

// Feature: NFC Proximity Threshold and Mapping
// Scenario: Verify spatial distance calculation and threshold gating
//
//   Given two positions in 3D space
//   When distance is calculated
//   Then distance <= 0.04m is within NFC proximity threshold
#[test]
fn test_nfc_spatial_proximity_threshold() {
    let p1 = netsim_model::Position { x: 0.0, y: 0.0, z: 0.0 };
    let p2 = netsim_model::Position { x: 0.0, y: 0.0, z: 0.035 }; // 3.5cm
    let p3 = netsim_model::Position { x: 0.0, y: 0.0, z: 0.045 }; // 4.5cm
    assert!(p1.distance(&p2) <= 0.04, "3.5cm should be within 4cm NFC threshold!");
    assert!(p1.distance(&p3) > 0.04, "4.5cm should be outside 4cm NFC threshold!");
}

// Feature: NFC Spatial Proximity Gating
// Scenario: NFC packets between devices are gated by the 4cm (0.04m) proximity threshold
//
//   Given two NFC chips on devices beyond 4cm (0.10m apart)
//   When Device 1 enters discovery mode and emits RF poll packets
//   Then Device 2 does not receive the packets (rf_taps_rx == 0)
//   When Device 2 moves within 4cm (0.02m apart) and Device 1 rediscovers
//   Then Device 2 receives the RF poll packets (rf_taps_rx >= 1)
#[tokio::test]
async fn test_nfc_spatial_proximity_gating() {
    use casimir::packets::nci::{
        CoreInitCommand, CoreResetCommand, DeactivationType, DiscoverConfiguration, FeatureEnable,
        ResetType, RfDeactivateCommand, RfDiscoverCommand, RfTechnologyAndMode,
    };
    use pdl_runtime::Packet;

    let mut world = NfcWorld::new().await;
    let chip1 = 1;
    let chip2 = 2;
    world.given_a_chip(chip1).await;
    world.given_a_chip(chip2).await;

    // Set initial device positions: 10cm apart (> 4cm threshold)
    world.set_device_position(DeviceId(chip1), 0.0, 0.0, 0.0);
    world.set_device_position(DeviceId(chip2), 0.10, 0.0, 0.0);

    async fn send_nci<P: Packet>(
        actor: &mut NfcActor,
        ctx: &mut TestContext,
        chip_id: ChipId,
        pkt: P,
    ) {
        let mut bytes = pkt.encode_to_vec().unwrap();
        if bytes.len() > 3 {
            bytes[2] = (bytes.len() - 3) as u8;
        }
        actor.on_stream(chip_id, Bytes::from(bytes), ctx).await;
    }

    // Initialize chip1 and start discovery
    send_nci(
        &mut world.actor,
        &mut world.ctx,
        ChipId(chip1),
        CoreResetCommand { reset_type: ResetType::KeepConfig },
    )
    .await;
    send_nci(
        &mut world.actor,
        &mut world.ctx,
        ChipId(chip1),
        CoreInitCommand { feature_enable: FeatureEnable {} },
    )
    .await;
    send_nci(
        &mut world.actor,
        &mut world.ctx,
        ChipId(chip1),
        RfDiscoverCommand {
            configurations: vec![DiscoverConfiguration {
                technology_and_mode: RfTechnologyAndMode::NfcAPassivePollMode,
                discovery_frequency: 1,
            }],
        },
    )
    .await;

    // Wait for Casimir to process RfDiscoverCommand and emit rf::PollCommand from chip1
    let first_poll = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while world.actor.nfc_stats.to_proto().rf_taps_tx() < 1 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(first_poll.is_ok(), "Chip 1 must transmit RF poll packets within timeout");

    // Verify chip1 transmitted RF taps but chip2 was out of range (10cm > 4cm)
    assert!(
        world.actor.nfc_stats.to_proto().rf_taps_tx() >= 1,
        "Chip 1 must transmit RF poll packets"
    );
    assert_eq!(
        world.actor.nfc_stats.to_proto().rf_taps_rx(),
        0,
        "Chip 2 must not receive RF packets when beyond 4cm threshold (0.10m > 0.04m)"
    );

    // Move Device 2 within 4cm (0.02m apart)
    world.set_device_position(DeviceId(chip2), 0.02, 0.0, 0.0);

    // Restart discovery on chip1 so it emits a new RF poll command
    send_nci(
        &mut world.actor,
        &mut world.ctx,
        ChipId(chip1),
        RfDeactivateCommand { deactivation_type: DeactivationType::IdleMode },
    )
    .await;
    send_nci(
        &mut world.actor,
        &mut world.ctx,
        ChipId(chip1),
        RfDiscoverCommand {
            configurations: vec![DiscoverConfiguration {
                technology_and_mode: RfTechnologyAndMode::NfcAPassivePollMode,
                discovery_frequency: 1,
            }],
        },
    )
    .await;

    // Wait for Casimir to emit the next RF poll command and deliver to chip2
    let second_poll = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while world.actor.nfc_stats.to_proto().rf_taps_rx() < 1 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(
        second_poll.is_ok(),
        "Chip 2 must receive RF packets when within 4cm threshold (0.02m <= 0.04m)"
    );
}

#[tokio::test]
async fn test_nfc_initial_enablement_universal() {
    let mut world = NfcWorld::new().await;
    let chip_id = 1;
    world.given_a_chip(chip_id).await;

    let chip = world.actor.handle_get(ChipId(chip_id), &mut world.ctx).await.unwrap().unwrap();
    assert!(
        chip.enabled,
        "Chip must be enabled upon creation across all platforms during transition to Netsim"
    );
}

#[tokio::test]
async fn test_nfc_stats_counters() {
    let mut world = NfcWorld::new().await;
    let chip_id = ChipId(1);
    world.given_a_chip(chip_id.0).await;

    // Enable the chip so that it can receive packets from Casimir
    let update_enable = netsim_model::ChipUpdate { enabled: Some(true), ..Default::default() };
    let update_res = world.actor.handle_update(chip_id, update_enable, &mut world.ctx).await;
    assert!(update_res.is_ok());

    use casimir::packets::nci::{
        ControlPacket, ControlPacketChild, CoreInitCommand, CoreResetCommand, DeactivationType,
        DiscoverConfiguration, FeatureEnable, ResetType, RfDeactivateCommand, RfDiscoverCommand,
        RfDiscoverSelectCommand, RfDiscoveryId, RfInterfaceType, RfPacketChild, RfProtocolType,
        RfSetListenModeRoutingCommand, RfTechnologyAndMode,
    };
    use nfc_actor::{NfcAction, NfcApi};
    use pdl_runtime::Packet;

    async fn send_and_verify(
        actor: &mut NfcActor,
        ctx: &mut TestContext,
        chip_id: ChipId,
        mut packet_bytes: Vec<u8>,
        api: NfcApi,
        expected_count: u32,
    ) {
        if packet_bytes.len() > 3 {
            packet_bytes[2] = (packet_bytes.len() - 3) as u8;
        }
        let msg = Bytes::from(packet_bytes);
        actor.on_stream(chip_id, msg, ctx).await;
        assert_eq!(actor.nfc_stats.get(api), expected_count, "Assertion failed for {:?}", api);
    }
    // 1. CoreReset -> does NOT increment AdapterDisable
    let cmd = CoreResetCommand { reset_type: ResetType::KeepConfig };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::AdapterDisable,
        0,
    )
    .await;

    // 2. CoreInit -> AdapterEnable
    let cmd = CoreInitCommand { feature_enable: FeatureEnable {} };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::AdapterEnable,
        1,
    )
    .await;

    // Setup Mock Peer on RF channel
    let (tx, rx) = tokio::sync::oneshot::channel();
    let action = NfcAction::CreateControlChannel { respond_to: tx };
    world.actor.handle_action(None, action, &mut world.ctx).await.unwrap();
    let (rf_io, rf_device_id) = rx.await.unwrap().unwrap();

    let (rf_rx, rf_tx) = tokio::io::split(rf_io);
    let mut rf_reader = ::casimir::RfReader::new(rf_rx);
    let mut rf_writer = ::casimir::RfWriter::new(rf_tx);

    let casimir_device_id = world.actor.active_chips.get(&chip_id).unwrap().casimir_device_id;
    let (mock_peer_cmd_tx, mut mock_peer_cmd_rx) = mpsc::channel::<Vec<u8>>(10);

    let mock_peer_handle = tokio::spawn(async move {
        use ::casimir::packets::rf;
        use pdl_runtime::Packet;

        loop {
            tokio::select! {
                res = rf_reader.read() => {
                    let bytes = match res {
                        Ok(b) => b,
                        Err(_) => break, // EOF
                    };
                    let pkt = match rf::RfPacket::decode_full(&bytes) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    if pkt.packet_type() == rf::RfPacketType::SelectCommand
                        && pkt.protocol() == rf::Protocol::IsoDep
                        && pkt.technology() == rf::Technology::NfcA
                    {
                        if let Ok(cmd) = rf::IsoDepT4ATSelectCommand::decode_full(&bytes) {
                            let resp = rf::IsoDepT4ATSelectResponse {
                                sender: rf_device_id,
                                receiver: cmd.sender(),
                                bitrate: rf::BitRate::BitRate106KbitS,
                                power_level: 10,
                                rats_response: vec![0x78, 0x80, 0x00, 0x00],
                            };
                            let _ = rf_writer.write(&resp.encode_to_vec().unwrap()).await;
                        }
                    } else if let Ok(rf::RfPacketChild::PollCommand(cmd)) = pkt.specialize() {
                        let resp = rf::NfcAPollResponse {
                            sender: rf_device_id,
                            receiver: cmd.sender(),
                            protocol: rf::Protocol::IsoDep,
                            bitrate: rf::BitRate::BitRate106KbitS,
                            power_level: 10,
                            nfcid1: vec![1, 2, 3, 4],
                            int_protocol: 0b01, // Type 4A Tag
                            bit_frame_sdd: 0,
                        };
                        let _ = rf_writer.write(&resp.encode_to_vec().unwrap()).await;
                    }
                }
                Some(data_bytes) = mock_peer_cmd_rx.recv() => {
                    let pkt = rf::Data {
                        sender: rf_device_id,
                        receiver: casimir_device_id,
                        technology: rf::Technology::NfcA,
                        protocol: rf::Protocol::IsoDep,
                        bitrate: rf::BitRate::BitRate106KbitS,
                        power_level: 10,
                        data: data_bytes,
                    };
                    let _ = rf_writer.write(&pkt.encode_to_vec().unwrap()).await;
                }
            }
        }
    });

    // 3. RfDiscover -> AdapterEnableReaderMode
    let cmd = RfDiscoverCommand {
        configurations: vec![DiscoverConfiguration {
            technology_and_mode: RfTechnologyAndMode::NfcAPassivePollMode,
            discovery_frequency: 1,
        }],
    };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::AdapterEnableReaderMode,
        1,
    )
    .await;

    // Wait for mock peer to activate Casimir (poll + select)
    let rx_sink = world.packet_receivers.get_mut(&chip_id).unwrap();
    loop {
        let bytes = rx_sink.recv().await.unwrap();
        let Ok(pkt) = ControlPacket::decode_full(&bytes) else {
            continue;
        };
        let Ok(ControlPacketChild::RfPacket(rf_pkt)) = pkt.specialize() else {
            continue;
        };
        if let Ok(RfPacketChild::RfIntfActivatedNotification(_)) = rf_pkt.specialize() {
            break;
        }
    }

    // 4. RfDiscoverSelect -> TagConnect
    let cmd = RfDiscoverSelectCommand {
        rf_discovery_id: RfDiscoveryId::try_from(1).unwrap(),
        rf_protocol: RfProtocolType::IsoDep,
        rf_interface: RfInterfaceType::Frame,
    };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::TagConnect,
        1,
    )
    .await;

    // 5. DataPacket (Guest -> Casimir in Poll Mode) -> TagTransceive
    let packet_bytes = vec![0, 0, 3, 1, 2, 3]; // NCI Data Packet: MT=0, ConnID=0, RFU=0, Len=3, Payload=[1,2,3]
    world
        .packet_senders
        .get(&chip_id)
        .unwrap()
        .send(Bytes::from(packet_bytes.clone()))
        .await
        .unwrap();
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        packet_bytes,
        NfcApi::TagTransceive,
        1,
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(world.actor.nfc_stats.to_proto().nci_data_rx(), 1);
    assert_eq!(world.actor.nfc_stats.to_proto().tag_emulation_count(), 1);
    assert!(world.actor.nfc_stats.to_proto().rf_taps_tx() >= 2);
    assert!(world.actor.nfc_stats.to_proto().rf_taps_rx() >= 2);

    // 6. DataPacket (Casimir -> Guest while in Listen Mode via mock peer) ->
    //    CardEmulationProcessCommandApdu
    world.actor.active_chips.get(&chip_id).unwrap().set_mode(nfc_actor::NfcMode::Listen);
    let nci_data_pkt = vec![0, 0, 3, 1, 2, 3]; // NCI Data Packet
    mock_peer_cmd_tx.send(nci_data_pkt).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(world.actor.nfc_stats.get(NfcApi::CardEmulationProcessCommandApdu), 1);
    assert_eq!(world.actor.nfc_stats.to_proto().nci_data_tx(), 1);
    assert_eq!(world.actor.nfc_stats.to_proto().card_emulation_count(), 1);

    // 7. RfDeactivate(SleepMode) -> TagClose
    let cmd = RfDeactivateCommand { deactivation_type: DeactivationType::SleepMode };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::TagClose,
        1,
    )
    .await;

    // 8. RfSetListenModeRouting -> AdapterSetListenModeRouting (switches mode to
    //    LISTEN)
    let cmd = RfSetListenModeRoutingCommand { more_to_follow: 0, routing_entries: vec![] };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::AdapterSetListenModeRouting,
        1,
    )
    .await;

    // 9. DataPacket (Guest -> Casimir in Listen Mode) ->
    //    CardEmulationSendResponseApdu
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        vec![0, 0, 2, 0x90, 0x00],
        NfcApi::CardEmulationSendResponseApdu,
        1,
    )
    .await;
    assert_eq!(world.actor.nfc_stats.to_proto().card_emulation_count(), 2);
    let ipc = world.actor.nfc_stats.to_ipc_proto();
    assert_eq!(ipc.nfc_stats.card_emulation_count(), 2);
    assert_eq!(ipc.nfc_stats.tag_emulation_count(), 1);
    assert!(ipc.nfc_api_stats.nci_data.send() >= 1);
    assert_eq!(ipc.nfc_api_stats.nci_data.receive(), 1);

    // 10. RfDeactivate(IdleMode) -> AdapterDisableReaderMode
    let cmd = RfDeactivateCommand { deactivation_type: DeactivationType::IdleMode };
    send_and_verify(
        &mut world.actor,
        &mut world.ctx,
        chip_id,
        cmd.encode_to_vec().unwrap(),
        NfcApi::AdapterDisableReaderMode,
        1,
    )
    .await;

    // 11. Chip disable -> AdapterDisable
    let update_disable = netsim_model::ChipUpdate { enabled: Some(false), ..Default::default() };
    let update_res = world.actor.handle_update(chip_id, update_disable, &mut world.ctx).await;
    assert!(update_res.is_ok());
    assert_eq!(world.actor.nfc_stats.get(NfcApi::AdapterDisable), 1);

    mock_peer_handle.abort();
}

#[tokio::test]
async fn test_multiple_chip_stats_separation() {
    let mut world = NfcWorld::new().await;

    // Given two active chips
    world.given_a_chip(1).await;
    world.given_a_chip(2).await;

    // Send packets through the streams to naturally increment counters
    let chip1_tx = world.packet_senders.get(&ChipId(1)).unwrap();
    let chip2_tx = world.packet_senders.get(&ChipId(2)).unwrap();

    // Send 3 DATA packets (Guest -> Casimir) for Chip 1
    // MT=0x00 (DATA)
    for _ in 0..3 {
        chip1_tx.send(Bytes::from(vec![0x00, 0x00, 0x00])).await.unwrap();
    }

    // Send 5 CMD packets (Guest -> Casimir) for Chip 2
    // MT=0x20 (CMD) -> 0x20 >> 5 == 1 (NCI_MT_CMD)
    for _ in 0..5 {
        chip2_tx.send(Bytes::from(vec![0x20, 0x00, 0x00])).await.unwrap();
    }

    // Yield to allow the background stream tasks to process the items
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Call GetStatistics
    use nfc_actor::{NfcAction, NfcActionResult};

    let res = world.actor.handle_action(None, NfcAction::GetStatistics, &mut world.ctx).await;
    assert!(res.is_ok());

    match res.unwrap() {
        NfcActionResult::Statistics(stats) => {
            assert_eq!(stats.len(), 2);
            let mut chip1_found = false;
            let mut chip2_found = false;
            for s in stats.iter() {
                if s.id == 1 {
                    assert_eq!(s.tx_count, 3);
                    assert_eq!(s.rx_count, 0);
                    chip1_found = true;
                } else if s.id == 2 {
                    assert_eq!(s.tx_count, 5);
                    assert_eq!(s.rx_count, 0);
                    chip2_found = true;
                }
            }
            assert!(chip1_found);
            assert!(chip2_found);
        }
        _ => panic!("Expected Statistics"),
    }
}

#[tokio::test]
async fn test_nci_packet_framing_codec() {
    use bytes::Bytes;
    use futures::StreamExt;
    use nfc_actor::NciCodec;
    use tokio_util::codec::FramedRead;

    // Continuous NCI stream:
    // Packet 1: Header [0x00, 0x00, 0x03] + Payload [0x01, 0x02, 0x03] (Len = 6)
    // Packet 2: Header [0x20, 0x00, 0x01] + Payload [0xAA]             (Len = 4)
    let raw_nci_bytes = vec![0x00, 0x00, 0x03, 0x01, 0x02, 0x03, 0x20, 0x00, 0x01, 0xAA];
    let cursor = std::io::Cursor::new(raw_nci_bytes);
    let mut stream = FramedRead::new(cursor, NciCodec);

    let frame1 = stream.next().await.unwrap().unwrap();
    assert_eq!(
        frame1.len(),
        6,
        "NCI Frame 1 length must be exactly 6 bytes (3 header + 3 payload)! No trailing bytes allowed."
    );
    assert_eq!(frame1, Bytes::from_static(&[0x00, 0x00, 0x03, 0x01, 0x02, 0x03]));

    let frame2 = stream.next().await.unwrap().unwrap();
    assert_eq!(
        frame2.len(),
        4,
        "NCI Frame 2 length must be exactly 4 bytes (3 header + 1 payload)!"
    );
    assert_eq!(frame2, Bytes::from_static(&[0x20, 0x00, 0x01, 0xAA]));

    assert!(stream.next().await.is_none());
}

const CHIP_1: ChipId = ChipId(1);
const CHIP_2: ChipId = ChipId(2);
const DEVICE_1: DeviceId = DeviceId(1);
const DEVICE_2: DeviceId = DeviceId(2);
const ORIGIN_METERS: f32 = 0.0;
const IN_RANGE_X_METERS: f32 = 0.02;
// CoreResetCommand + CoreInitCommand ->
// CoreResetResponse + CoreResetNotification + CoreInitResponse.
const NCI_CORE_INIT_TX: u64 = 2;
const NCI_CORE_INIT_RX: u64 = 3;
// Adds RfSetListenModeRoutingCommand -> RfSetListenModeRoutingResponse.
const NCI_LISTEN_ROUTING_TX: u64 = NCI_CORE_INIT_TX + 1;
const NCI_LISTEN_ROUTING_RX: u64 = NCI_CORE_INIT_RX + 1;
// CoreResetCommand + CoreInitCommand + RfDiscoverCommand + RfDeactivateCommand >= 3 TX/RX.
const MIN_POST_DISCOVERY_NCI_TX: u64 = 3;
const MIN_POST_DISCOVERY_NCI_RX: u64 = 3;
const MIN_P2P_RF_PACKETS: u64 = 1;

/// Scenario: Disabling NFC chips gates NCI and RF traffic in both directions and
/// re-enabling resumes NCI and RF forwarding.
#[tokio::test]
async fn test_nfc_client_disabled_gates_nci_and_rf_and_resumes_on_reenable() {
    // Given two in-proximity NFC chips with Chip 1 NCI core initialized
    let mut world = NfcClientWorld::new().await;
    world.given_chip_at(CHIP_1, DEVICE_1, ORIGIN_METERS, ORIGIN_METERS, ORIGIN_METERS).await;
    world.given_chip_at(CHIP_2, DEVICE_2, IN_RANGE_X_METERS, ORIGIN_METERS, ORIGIN_METERS).await;
    world.given_nci_core_initialized(CHIP_1).await;
    world.then_chip_packet_counts_are(CHIP_1, NCI_CORE_INIT_TX, NCI_CORE_INIT_RX).await;
    world.then_api_stat_count_is(nfc_actor::NfcApi::AdapterEnable, 1);

    // When Chip 1 is disabled and NCI command and data packets are sent
    world.when_chip_enabled_set(CHIP_1, false).await;
    world.when_nci_core_init_sent(CHIP_1).await;
    world.when_nci_apdu_response_data_sent(CHIP_1).await;

    // Then no NCI response is emitted and counters remain unchanged
    world.then_no_nci_packet_is_received(CHIP_1).await;
    world.then_chip_packet_counts_are(CHIP_1, NCI_CORE_INIT_TX, NCI_CORE_INIT_RX).await;
    world.then_api_stat_count_is(nfc_actor::NfcApi::AdapterEnable, 1);

    // When Chip 2 is disabled while Chip 1 is re-enabled and starts RF discovery
    world.when_chip_enabled_set(CHIP_2, false).await;
    world.when_chip_enabled_set(CHIP_1, true).await;
    world.when_nci_discovery_started(CHIP_1).await;

    // Then Chip 1 transmits RF polls while disabled Chip 2 drops ingress RF packets
    world.then_rf_polls_transmitted_while_peer_receives_none().await;

    // When Chip 1 is disabled during active RF discovery across a Casimir poll interval
    world.when_chip_enabled_set(CHIP_1, false).await;

    // Then egress RF poll packets from Chip 1 are gated
    world.then_rf_tx_remains_unchanged_over_poll_window().await;

    // When both Chip 1 and Chip 2 are re-enabled
    world.when_chip_enabled_set(CHIP_1, true).await;
    world.when_chip_enabled_set(CHIP_2, true).await;

    // Then RF poll delivery resumes between the two chips and discovery stops cleanly
    world.then_rf_rx_resumes().await;
    world.when_nci_discovery_stopped(CHIP_1).await;

    world.shutdown().await;
}

/// Scenario: Resetting an NFC chip restores initial metadata and enablement, resets
/// listen mode back to idle, and clears per-chip packet counters.
#[tokio::test]
async fn test_nfc_client_reset_restores_initial_chip_mode_and_clears_stats() {
    // Given an initialized NFC chip with custom metadata placed into listen mode
    let mut world = NfcClientWorld::new().await;
    world.given_chip_with_metadata(CHIP_1, DEVICE_1, "custom-nfc-1", "InitMfg", "InitProd").await;
    world.given_nci_core_initialized(CHIP_1).await;
    world.given_nci_listen_mode_routing_configured(CHIP_1).await;
    world.then_chip_packet_counts_are(CHIP_1, NCI_LISTEN_ROUTING_TX, NCI_LISTEN_ROUTING_RX).await;
    world.then_api_stat_count_is(nfc_actor::NfcApi::AdapterSetListenModeRouting, 1);
    world.then_api_stat_count_is(nfc_actor::NfcApi::CardEmulationSendResponseApdu, 0);

    // And the chip's metadata and radio state are mutated via update
    world
        .when_chip_metadata_and_radio_updated(CHIP_1, "mutated-nfc-1", "MutMfg", "MutProd", false)
        .await;
    world.then_chip_read_matches(CHIP_1, "mutated-nfc-1", "MutMfg", "MutProd", false).await;

    // When the chip is reset via NfcClient::reset
    world.when_chip_is_reset(CHIP_1).await;

    // Then initial metadata and enabled radio state are restored and packet counters are zeroed
    world.then_reset_restored_initial_chip(CHIP_1, "custom-nfc-1", "InitMfg", "InitProd").await;
    world.then_chip_packet_counts_are(CHIP_1, 0, 0).await;

    // When an APDU response data packet is sent followed on the FIFO stream by CoreInitCommand
    world.when_nci_apdu_response_data_sent(CHIP_1).await;
    world.when_nci_core_init_completed(CHIP_1).await;

    // Then traffic resumes on the re-enabled chip in Idle mode without counting listen-mode APDUs
    world.then_chip_tx_count_at_least(CHIP_1, 1).await;
    world.then_api_stat_count_is(nfc_actor::NfcApi::CardEmulationSendResponseApdu, 0);

    world.shutdown().await;
}

/// Scenario: Peer-to-peer NFC interactions track p2p_tx_count and p2p_rx_count, reflect
/// chip name in radio statistics, and resetting one chip restores initial name and counters
/// while preserving peer chip stats.
#[tokio::test]
async fn test_nfc_client_p2p_stats_and_chip_name_preserved_and_reset() {
    // Given two named NFC chips within physical proximity
    let mut world = NfcClientWorld::new().await;
    world
        .given_named_chip_at(
            CHIP_1,
            DEVICE_1,
            "custom-nfc-1",
            ORIGIN_METERS,
            ORIGIN_METERS,
            ORIGIN_METERS,
        )
        .await;
    world
        .given_named_chip_at(
            CHIP_2,
            DEVICE_2,
            "custom-nfc-2",
            IN_RANGE_X_METERS,
            ORIGIN_METERS,
            ORIGIN_METERS,
        )
        .await;

    // And both chips are NCI core initialized
    world.given_nci_core_initialized(CHIP_1).await;
    world.given_nci_core_initialized(CHIP_2).await;

    // When both chips start RF discovery and exchange peer-to-peer traffic
    world.when_nci_discovery_started(CHIP_1).await;
    world.when_nci_discovery_started(CHIP_2).await;

    // Then peer-to-peer packet exchanges are observed on both chips
    world.then_p2p_traffic_exchanged(CHIP_1, CHIP_2).await;

    // When RF discovery is stopped on both chips to halt periodic timers
    world.when_nci_discovery_stopped(CHIP_1).await;
    world.when_nci_discovery_stopped(CHIP_2).await;

    // Then Chip 1 radio statistics reflect custom name and active traffic counts
    world
        .then_chip_radio_stats(
            CHIP_1,
            "custom-nfc-1",
            MIN_POST_DISCOVERY_NCI_TX,
            MIN_POST_DISCOVERY_NCI_RX,
            MIN_P2P_RF_PACKETS,
            MIN_P2P_RF_PACKETS,
        )
        .await;

    // When Chip 1's name is mutated via update
    world.when_chip_name_updated(CHIP_1, "renamed-nfc-1").await;

    // Then radio statistics immediately reflect the updated chip name
    world.then_radio_stats_name_is(CHIP_1, "renamed-nfc-1").await;

    // When Chip 1 is reset via NfcClient::reset
    world.when_chip_is_reset(CHIP_1).await;

    // Then Chip 1 initial name is restored with all packet counters zeroed
    world.then_chip_radio_stats_zeroed(CHIP_1, "custom-nfc-1").await;

    // And Chip 2 radio statistics and counters are preserved
    world
        .then_chip_radio_stats(
            CHIP_2,
            "custom-nfc-2",
            MIN_POST_DISCOVERY_NCI_TX,
            MIN_POST_DISCOVERY_NCI_RX,
            MIN_P2P_RF_PACKETS,
            MIN_P2P_RF_PACKETS,
        )
        .await;

    world.shutdown().await;
}
