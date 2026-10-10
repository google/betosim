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

use std::{sync::Arc, time::Duration};

use actor_framework::ResourceActor;
use ap_actor::{ApActor, ApClient, shared::SharedKeyStore};
use device_actor::DeviceClient;
use netsim_model::{ChipClient, ChipCreate, ChipId};
use slirp_actor::SlirpActor;
use tokio::{net::UdpSocket, sync::mpsc};
use wifi_actor::WifiActor;

use crate::hwsim_helper::{unwrap_hwsim_to_ethernet, wrap_ethernet_in_hwsim};

// BSSID for the AP
const HOSTAPD_BSSID: [u8; 6] = [0x00, 0x13, 0x10, 0x85, 0xfe, 0x01];
// MAC for the connection
const CLIENT_MAC: [u8; 6] = [0x02, 0x01, 0x02, 0x03, 0x04, 0x05];
const CLIENT_HWSIM_ADDR: [u8; 6] = [0x42, 0x42, 0x42, 0x42, 0x42, 0x42];
const TEST_FREQ: u32 = 2412;

#[tokio::test]
async fn test_udp_guest_to_host() {
    // 1. Setup Architecture
    // 1.1 Slirp
    let slirp_actor_impl = SlirpActor::new(Default::default());
    let (slirp_runner, slirp_client) = slirp_actor::new();

    // 1.2 Ap
    let shared_keys = Arc::new(SharedKeyStore::new());
    let ap_actor_impl = ApActor::new(shared_keys.clone());
    let (ap_runner, ap_client_base) = ResourceActor::new(32);
    let ap_client = ApClient::new(ap_client_base);

    // 1.3 Wifi
    // Create Medium
    // Create WifiActor Logic
    // Medium is now internal

    // Create Dummy DeviceClient
    let (dummy_tx, _dummy_rx) = mpsc::channel(1);
    let resource_client = actor_framework::ResourceClient::new(dummy_tx);
    let device_client = DeviceClient::new(Box::new(resource_client));

    let wifi_actor_impl = WifiActor::new(
        Some(ap_client.clone()),
        Some(slirp_client.clone()),
        device_client,
        None, // wifi_tap
        shared_keys.clone(),
        Arc::new(wifi_actor::SystemClock),
    );

    // Create Runner
    let (wifi_runner, wifi_client) = wifi_actor::new();

    // Spawn Actors
    tokio::spawn(slirp_runner.run(slirp_actor_impl));
    tokio::spawn(ap_runner.run(ap_actor_impl));
    tokio::spawn(wifi_runner.run(wifi_actor_impl));

    // 2. Create a Chip
    let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel(128); // From Actor to Chip
    let (sink_tx, sink_rx) = tokio::sync::mpsc::channel(128); // From Chip to Actor

    use tokio_stream::wrappers::ReceiverStream;
    let packet_stream = Box::new(ReceiverStream::new(sink_rx)); // Chip -> Actor

    // Capture packets sent to Chip
    let packet_sink =
        Box::pin(futures::sink::unfold(stream_tx, |tx, item: bytes::Bytes| async move {
            let _ = tx.send(item).await;
            Ok::<_, std::io::Error>(tx)
        }));

    let chip = netsim_model::Chip {
        name: "wifi-chip".to_string(),
        manufacturer: "google".to_string(),
        product_name: "test".to_string(),
        kind: netsim_model::ChipKind::WIFI,
        device_id: device_api::DeviceId(1),
        variant: Some(netsim_model::ChipVariant::Wifi(netsim_model::Wifi {
            radio: Default::default(),
        })),
        ..Default::default()
    };

    let params =
        ChipCreate { packet_stream: Some(packet_stream), packet_sink: Some(packet_sink), chip };

    // Create AP
    println!("Creating AP...");
    let ap_config = ap_actor::ApConfig {
        ssid: "TestAP".to_string(),
        bssid: netsim_packets::MacAddr::from(HOSTAPD_BSSID),
        channel: 6,
        hw_mode: netsim_model::WifiMode::G,
        wpa_passphrase: None,
    };
    use ap_actor::ApResponse;
    let id = ap_client.create_ap(None, ap_config).await.expect("Failed to create AP");
    println!("AP Created with ID: {}", id);
    println!(
        "BSSID in KeyStore: {:?}",
        shared_keys.has_bssid(&netsim_packets::ieee80211::MacAddress::new(HOSTAPD_BSSID))
    );

    wifi_client.create(ChipId(1), params).await.expect("Failed to create chip");
    println!("Chip created");

    // 3. Setup Host UDP Listener
    let host_socket = UdpSocket::bind("127.0.0.1:0").await.expect("Failed to bind host socket");
    let host_addr = host_socket.local_addr().unwrap();
    println!("Host listening on {}", host_addr);

    // 4. Send UDP packet from "Guest" (simulated via wrapping)
    use netsim_packets::utils::test_utils::PacketBuilder;
    let payload = b"Hello Host from Slirp!";

    // For Slirp, the Host is accessible via 10.0.2.2 (alias for Host Loopback)
    // Sending to 127.0.0.1 from Guest would just mean Guest Localhost.
    let host_ip = [10, 0, 2, 2]; // Slirp special alias

    let eth_frame = PacketBuilder::new(HOSTAPD_BSSID, CLIENT_MAC, 0x0800)
        .ipv4(
            [10, 0, 2, 15],
            host_ip,
            17, // UDP
        )
        .udp(12345, host_addr.port())
        .payload(payload);

    // Wrap in Hwsim
    let hwsim_msg = wrap_ethernet_in_hwsim(
        &eth_frame,
        &HOSTAPD_BSSID,
        &HOSTAPD_BSSID,     // dest address (AP)
        &CLIENT_HWSIM_ADDR, // src address (Client)
        TEST_FREQ,
    )
    .expect("Failed to wrap");

    // 5. Inject into Actor (Simulate Guest sending to Chip Sink)
    sink_tx.send(bytes::Bytes::from(hwsim_msg)).await.expect("Failed to send to sink_tx");

    println!("Packet injected");
    let mut buf = [0u8; 1024];
    // Remove set_read_timeout as tokio UdpSocket doesn't support it directly, use
    // timeout wrapper
    println!("Waiting for packet on Host socket...");
    let (amt, src) = tokio::time::timeout(Duration::from_secs(2), host_socket.recv_from(&mut buf))
        .await
        .expect("Host did not receive packet (timeout?)")
        .expect("recv_from failed");

    assert_eq!(&buf[..amt], payload);
    println!("Host received packet from {}", src);

    // 6. Host Replies
    let reply_payload = b"Hello Guest from Host!";
    host_socket.send_to(reply_payload, src).await.expect("Failed to reply");

    // 7. Verify Guest Received (via stream_rx from Actor)
    let mut received_reply = false;
    let timeout = tokio::time::sleep(Duration::from_secs(2));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
             Some(msg_bytes) = stream_rx.recv() => {
                 match unwrap_hwsim_to_ethernet(&msg_bytes) {
                     Ok(eth_frame) => {
                         let eth_type = u16::from_be_bytes([eth_frame[12], eth_frame[13]]);
                         if eth_type == 0x0806 {
                             // ARP Packet
                             // Extract Sender SHA (Slirp MAC) and SPA (Slirp IP) from Request
                             // ARP Header starts at 14
                             // SHA @ 14 + 8 = 22
                             // SPA @ 14 + 14 = 28
                             let mut slirp_mac = [0u8; 6];
                             slirp_mac.copy_from_slice(&eth_frame[22..28]);
                             let mut slirp_ip = [0u8; 4];
                             slirp_ip.copy_from_slice(&eth_frame[28..32]);

                             let client_ip = [10, 0, 2, 15]; // Guest IP

                             // Build ARP Reply with PacketBuilder
                             let curr_eth_frame = PacketBuilder::new(slirp_mac, CLIENT_MAC, 0x0806)
                                 .arp(2, CLIENT_MAC, client_ip, slirp_mac, slirp_ip)
                                 .payload(&[]);

                             // Wrap in HWSIM
                             let hwsim_msg = wrap_ethernet_in_hwsim(
                                 &curr_eth_frame,
                                 &HOSTAPD_BSSID,
                                 &HOSTAPD_BSSID,
                                 &CLIENT_HWSIM_ADDR,
                                 TEST_FREQ,
                             ).expect("Failed to wrap ARP reply");

                             // Inject Reply (Guest -> Host)
                             sink_tx.send(bytes::Bytes::from(hwsim_msg)).await.expect("Failed to inject ARP reply");

                         } else if eth_frame.windows(reply_payload.len()).any(|w| w == reply_payload) {
                             received_reply = true;
                             break;
                         } else {
                             println!("Received non-ARP packet with mismatching payload. Len: {}", eth_frame.len());
                         }
                     },
                     Err(e) => {
                         println!("Failed to unwrap hwsim on Guest: {}", e);
                     }
                 }
             }
             _ = &mut timeout => {
                 break; // Timeout
             }
        }
    }

    assert!(received_reply, "Guest did not receive reply from Host via Slirp");
    println!("Test passed.");
}
