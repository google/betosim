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

mod world;

use world::World;

const HCI_RESET_CMD: [u8; 4] = [0x01, 0x03, 0x0c, 0x00];
const HCI_READ_LOCAL_VERSION_CMD: [u8; 4] = [0x01, 0x01, 0x10, 0x00];
const HCI_ACL_DATA_PACKET: [u8; 9] = [0x02, 0x10, 0x00, 0x04, 0x00, 0xde, 0xad, 0xbe, 0xef];
const HCI_COMMAND_COMPLETE_EVT: [u8; 7] = [0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00];
const HCI_NUM_COMPLETED_PACKETS_EVT: [u8; 8] = [0x04, 0x13, 0x05, 0x01, 0x10, 0x00, 0x01, 0x00];
// H4 Command indicator + opcode LSB only; EOF before the 3-byte command header completes.
const TRUNCATED_HCI_CMD_HEADER: [u8; 2] = [0x01, 0x03];
// H4 ACL indicator + 4-byte header declaring 4 payload bytes, followed by only 2 bytes.
const TRUNCATED_HCI_ACL_PACKET: [u8; 7] = [0x02, 0x10, 0x00, 0x04, 0x00, 0xde, 0xad];

#[tokio::test]
async fn client_connection_registers_bluetooth_hci_chip() {
    // Scenario: Connecting a TCP client to hci_server::run registers a visible Bluetooth HCI chip
    // Given a running HCI socket server backed by a DeviceClient
    let mut world = World::given_running_server().await;

    // When a TCP client connects to the HCI server
    let client_idx = world.when_client_connects().await;

    // Then DeviceClient receives AddChipByGuid with socket-<addr> and ChipKind::BLUETOOTH
    world.then_bluetooth_chip_is_registered(client_idx);
}

#[tokio::test]
async fn uplink_h4_framing_and_recovery_to_packet_stream() {
    // Scenario: Uplink H4 packets are framed onto packet_stream and recover from invalid prefixes
    // Given a running HCI socket server with a connected TCP client
    let mut world = World::given_running_server().await;
    let client_idx = world.when_client_connects().await;

    // When the client sends an H4 HCI Command packet (0x01) and an H4 ACL packet (0x02)
    let hci_cmd = [0x01, 0x02, 0x01, 0x01, 0x03];
    let hci_acl = [0x02, 0x10, 0x00, 0x02, 0x00, 0xaa, 0xbb];
    world.when_client_sends_h4_bytes(client_idx, &hci_cmd).await;
    world.when_client_sends_h4_bytes(client_idx, &hci_acl).await;

    // Then the registered chip's packet_stream yields the exact framed H4 packets
    world.then_packet_stream_receives(client_idx, &hci_cmd).await;
    world.then_packet_stream_receives(client_idx, &hci_acl).await;

    // When the client sends corrupt H4 bytes followed by an HCI Reset command ([0x01, 0x03, 0x0c, 0x00])
    // and then disconnects its TCP socket
    let corrupt_then_reset = [0xff, 0xde, 0xad, 0x01, 0x03, 0x0c, 0x00];
    world.when_client_sends_h4_bytes(client_idx, &corrupt_then_reset).await;
    world.when_client_disconnects(client_idx).await;

    // Then H4Codec recovers at the HCI Reset boundary, yields the HCI Reset frame,
    // and the chip's packet_stream terminates cleanly with None
    let hci_reset = [0x01, 0x03, 0x0c, 0x00];
    world.then_packet_stream_receives(client_idx, &hci_reset).await;
    world.then_packet_stream_ends(client_idx).await;
}

#[tokio::test]
async fn downlink_packet_sink_writes_to_client_socket() {
    // Scenario: Downlink HCI packets sent to packet_sink are written to the TCP client socket
    // Given a running HCI socket server with a connected TCP client
    let mut world = World::given_running_server().await;
    let client_idx = world.when_client_connects().await;

    // When the backend sends HCI Event (0x04) payloads to the registered chip's packet_sink
    let hci_command_complete_evt = [0x04, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00];
    let hci_inquiry_complete_evt = [0x04, 0x01, 0x01, 0x00];
    world.when_backend_sends_to_packet_sink(client_idx, &hci_command_complete_evt).await;
    world.when_backend_sends_to_packet_sink(client_idx, &hci_inquiry_complete_evt).await;

    // Then the TCP client socket receives the exact bytes in order
    world.then_client_socket_receives(client_idx, &hci_command_complete_evt).await;
    world.then_client_socket_receives(client_idx, &hci_inquiry_complete_evt).await;
}

#[tokio::test]
async fn add_chip_error_is_handled_gracefully_and_server_continues() {
    // Scenario: An error from device_client.add_chip does not crash the server and subsequent clients succeed
    // Given a running HCI socket server configured to fail the next add_chip call
    let mut world = World::given_running_server().await;
    world.given_next_add_chip_will_fail();

    // When the first client connects and add_chip fails
    world.when_client_connects_and_registration_fails().await;

    // And a second client subsequently connects
    let second_client_idx = world.when_client_connects().await;

    // Then the second client's Bluetooth HCI chip is registered and exchanges packets normally
    world.then_bluetooth_chip_is_registered(second_client_idx);
    let hci_reset = [0x01, 0x03, 0x0c, 0x00];
    world.when_client_sends_h4_bytes(second_client_idx, &hci_reset).await;
    world.then_packet_stream_receives(second_client_idx, &hci_reset).await;
}

#[tokio::test]
async fn multiple_connected_clients_stream_and_disconnect_independently() {
    // Scenario: Simultaneously connected HCI TCP clients maintain isolated streams and lifecycles
    // Given a running HCI socket server backed by a DeviceClient
    let mut world = World::given_running_server().await;

    // When two TCP clients are connected to the HCI server at the same time
    let client_a = world.when_client_connects().await;
    let client_b = world.when_client_connects().await;

    // Then both clients register Bluetooth HCI chips
    world.then_bluetooth_chip_is_registered(client_a);
    world.then_bluetooth_chip_is_registered(client_b);

    // When client A sends an HCI Command and client B sends an HCI ACL packet
    world.when_client_sends_h4_bytes(client_a, &HCI_RESET_CMD).await;
    world.when_client_sends_h4_bytes(client_b, &HCI_ACL_DATA_PACKET).await;

    // Then each client's stream receives its own uplink packet
    world.then_packet_stream_receives(client_a, &HCI_RESET_CMD).await;
    world.then_packet_stream_receives(client_b, &HCI_ACL_DATA_PACKET).await;

    // When the backend sends downlink HCI events addressed to client A and client B respectively
    world.when_backend_sends_to_packet_sink(client_a, &HCI_COMMAND_COMPLETE_EVT).await;
    world.when_backend_sends_to_packet_sink(client_b, &HCI_NUM_COMPLETED_PACKETS_EVT).await;

    // Then each client's socket receives its corresponding downlink event
    world.then_client_socket_receives(client_a, &HCI_COMMAND_COMPLETE_EVT).await;
    world.then_client_socket_receives(client_b, &HCI_NUM_COMPLETED_PACKETS_EVT).await;

    // When client A disconnects its TCP socket
    world.when_client_disconnects(client_a).await;

    // Then client A's packet_stream ends while client B's reader and writer tasks remain active
    world.then_packet_stream_ends(client_a).await;
    world.when_client_sends_h4_bytes(client_b, &HCI_READ_LOCAL_VERSION_CMD).await;
    world.then_packet_stream_receives(client_b, &HCI_READ_LOCAL_VERSION_CMD).await;
    world.when_backend_sends_to_packet_sink(client_b, &HCI_COMMAND_COMPLETE_EVT).await;
    world.then_client_socket_receives(client_b, &HCI_COMMAND_COMPLETE_EVT).await;
}

#[tokio::test]
async fn truncated_h4_frame_at_disconnect_terminates_packet_stream() {
    // Scenario: Trailing short-header or mid-payload truncated H4 frames at EOF close packet_stream
    // Given a running HCI socket server with two connected TCP clients
    let mut world = World::given_running_server().await;
    let short_header_client = world.when_client_connects().await;
    let mid_payload_client = world.when_client_connects().await;

    // When the first client sends a valid HCI Reset followed by a truncated 2-byte command header
    // and disconnects its TCP socket
    world.when_client_sends_h4_bytes(short_header_client, &HCI_RESET_CMD).await;
    world.when_client_sends_h4_bytes(short_header_client, &TRUNCATED_HCI_CMD_HEADER).await;
    world.when_client_disconnects(short_header_client).await;

    // Then the first chip's packet_stream yields the complete HCI Reset and terminates with None
    world.then_packet_stream_receives(short_header_client, &HCI_RESET_CMD).await;
    world.then_packet_stream_ends(short_header_client).await;

    // When the second client sends a valid HCI Reset followed by an ACL packet truncated
    // mid-payload and disconnects its TCP socket
    world.when_client_sends_h4_bytes(mid_payload_client, &HCI_RESET_CMD).await;
    world.when_client_sends_h4_bytes(mid_payload_client, &TRUNCATED_HCI_ACL_PACKET).await;
    world.when_client_disconnects(mid_payload_client).await;

    // Then the second chip's packet_stream yields the complete HCI Reset and terminates with None
    world.then_packet_stream_receives(mid_payload_client, &HCI_RESET_CMD).await;
    world.then_packet_stream_ends(mid_payload_client).await;
}
