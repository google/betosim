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

use netsim_model::{ChipKind, ChipVariant};

use crate::world::TestWorld;

#[tokio::test]
async fn test_websocket_happy_path() {
    let mut world = TestWorld::new().await;

    let client_idx = world
        .given_a_websocket_client_connected(Some("test-peer"), Some("00:11:22:33:44:55"))
        .await;

    world.then_backend_receives_add_chip_request(client_idx, "test-peer", "00:11:22:33:44:55");

    let test_packet = vec![0x01, 0x02, 0x03, 0x04];
    world.when_client_sends_packet(client_idx, &test_packet).await;
    world.then_backend_receives_packet(client_idx, &test_packet).await;

    let response_packet = vec![0x05, 0x06, 0x07, 0x08];
    world.when_backend_sends_packet(client_idx, &response_packet).await;
    world.then_client_receives_packet(client_idx, &response_packet).await;

    world.when_client_closes_connection(client_idx).await;
    world.then_backend_deletes_chip(client_idx).await;
}

#[tokio::test]
async fn test_per_radio_routes_all_kinds() {
    let mut world = TestWorld::new().await;

    // Wi-Fi route
    let wifi_idx =
        world.given_a_websocket_client_connected_at_path("/v1/websocket/wifi?name=wifi-dev").await;
    let wifi_req = world.add_request(wifi_idx);
    assert_eq!(wifi_req.device_config.name, "wifi-dev");
    assert_eq!(wifi_req.chip.kind, ChipKind::WIFI);
    assert!(matches!(wifi_req.chip.variant, Some(ChipVariant::Wifi(_))));

    // UWB route with address
    let uwb_idx = world
        .given_a_websocket_client_connected_at_path(
            "/v1/websocket/uwb?name=uwb-dev&address=11:22:33:44:55:66",
        )
        .await;
    let uwb_req = world.add_request(uwb_idx);
    assert_eq!(uwb_req.chip.kind, ChipKind::UWB);
    match &uwb_req.chip.variant {
        Some(ChipVariant::Uwb(uwb)) => assert_eq!(uwb.address, "11:22:33:44:55:66"),
        other => panic!("Expected Uwb variant, got {other:?}"),
    }

    // NFC route
    let nfc_idx =
        world.given_a_websocket_client_connected_at_path("/v1/websocket/nfc?name=nfc-dev").await;
    let nfc_req = world.add_request(nfc_idx);
    assert_eq!(nfc_req.chip.kind, ChipKind::NFC);
    assert!(matches!(nfc_req.chip.variant, Some(ChipVariant::Nfc(_))));

    // Cellular route with emulator quirks
    let cell_idx = world
        .given_a_websocket_client_connected_at_path(
            "/v1/websocket/cellular?name=cell-dev&device_kind=EMULATOR&sdk_version=36",
        )
        .await;
    let cell_req = world.add_request(cell_idx);
    assert_eq!(cell_req.chip.kind, ChipKind::CELLULAR);
    match &cell_req.chip.variant {
        Some(ChipVariant::Cell(cell)) => {
            assert!(cell.quirks.goldfish_ril_37_or_earlier);
            assert!(!cell.quirks.is_cuttlefish);
            assert!(cell.quirks.auto_ctzv);
            assert!(cell.quirks.allow_ipv4_for_ipv6);
            assert!(cell.quirks.dynamic_slirp_lease);
        }
        other => panic!("Expected Cell variant, got {other:?}"),
    }

    // Cellular Data route
    let cell_data_idx =
        world.given_a_websocket_client_connected_at_path("/v1/websocket/cellular_data").await;
    let cell_data_req = world.add_request(cell_data_idx);
    assert_eq!(cell_data_req.chip.kind, ChipKind::CELLULAR_DATA);
    assert!(matches!(cell_data_req.chip.variant, Some(ChipVariant::CellularData(_))));

    // Ethernet route
    let eth_idx = world.given_a_websocket_client_connected_at_path("/v1/websocket/ethernet").await;
    let eth_req = world.add_request(eth_idx);
    assert_eq!(eth_req.chip.kind, ChipKind::ETHERNET);
    assert!(matches!(eth_req.chip.variant, Some(ChipVariant::Ethernet(_))));

    // Modem alias route for Cellular
    let modem_idx = world
        .given_a_websocket_client_connected_at_path("/v1/websocket/modem?name=modem-dev")
        .await;
    let modem_req = world.add_request(modem_idx);
    assert_eq!(modem_req.chip.kind, ChipKind::CELLULAR);

    // Verify bidirectional streaming on non-Bluetooth radio (Wi-Fi & UWB)
    for (idx, payload) in [(wifi_idx, &[0x80, 0x00, 0x01][..]), (uwb_idx, &[0x20, 0x01, 0x00][..])]
    {
        world.when_client_sends_packet(idx, payload).await;
        world.then_backend_receives_packet(idx, payload).await;

        world.when_backend_sends_packet(idx, payload).await;
        world.then_client_receives_packet(idx, payload).await;
    }

    for idx in [wifi_idx, uwb_idx, nfc_idx, cell_idx, cell_data_idx, eth_idx, modem_idx] {
        world.when_client_closes_connection(idx).await;
        world.then_backend_deletes_chip(idx).await;
    }
}

#[tokio::test]
async fn test_device_and_chip_metadata_query_parameters() {
    let mut world = TestWorld::new().await;

    let idx = world
        .given_a_websocket_client_connected_at_path(
            "/v1/websocket/bt?device_guid=emulator-5554&name=Pixel_9&device_kind=CUTTLEFISH\
             &version=35.1.0&sdk_version=35&build_id=AP4A&variant=userdebug&arch=x86_64\
             &avd_path=%2Ftmp%2Favd&chip_id=02%3A00%3A00%3A00%3A00%3A01\
             &manufacturer=Android&product_name=VirtBt",
        )
        .await;

    let req = world.add_request(idx);
    assert_eq!(req.device_guid, "emulator-5554");
    assert_eq!(req.device_config.name, "Pixel_9");
    let info = req.device_config.device_info.as_ref().expect("Expected device_info");
    assert_eq!(info.name, "Pixel_9");
    assert_eq!(info.kind, "CUTTLEFISH");
    assert_eq!(info.version, "35.1.0");
    assert_eq!(info.sdk_version, "35");
    assert_eq!(info.build_id, "AP4A");
    assert_eq!(info.variant, "userdebug");
    assert_eq!(info.arch, "x86_64");
    assert_eq!(info.avd_path, "/tmp/avd");

    assert_eq!(req.chip.name, "02:00:00:00:00:01");
    assert_eq!(req.chip.manufacturer, "Android");
    assert_eq!(req.chip.product_name, "VirtBt");
    match &req.chip.variant {
        Some(ChipVariant::Bluetooth(bt)) => assert_eq!(bt.address, "02:00:00:00:00:01"),
        other => panic!("Expected Bluetooth variant, got {other:?}"),
    }

    world.when_client_closes_connection(idx).await;
    world.then_backend_deletes_chip(idx).await;
}
