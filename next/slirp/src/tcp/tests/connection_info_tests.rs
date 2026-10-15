// Copyright 2023-2025 The Android Open Source Project
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
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use netsim_packets::MacAddr;

use crate::{
    ConnectionInfo, TcpConnectionInfo, UdpConnectionInfo, tcp,
    tcp::{
        manager::TcpManager,
        state::{RecvSequenceSpace, SendSequenceSpace, TcpConnection},
    },
    udp::UdpManager,
};

#[test]
fn test_connection_info() {
    let mut tcp_manager = TcpManager::new(Ipv4Addr::new(10, 0, 2, 2));
    let mut udp_manager = UdpManager::new();
    let guest_addr: SocketAddr = "10.0.2.15:1234".parse().unwrap();
    let host_addr: SocketAddr = "1.1.1.1:80".parse().unwrap();

    tcp_manager.connections.insert(
        1,
        TcpConnection {
            guest_mac: MacAddr { bytes: [0; 6] },
            gateway_mac: MacAddr { bytes: [0; 6] },
            state: tcp::state::State::Established,
            guest_addr,
            host_addr,
            send: SendSequenceSpace { una: 0, nxt: 0, iss: 0 },
            recv: RecvSequenceSpace { nxt: 0 },
            unacked: Default::default(),
            retransmission_timeout: Duration::from_secs(1),
            retransmissions: 0,
            mss: None,
            srtt: Default::default(),
            rttvar: Default::default(),
            sent_packets: Default::default(),
            window_size: 0,
            send_window: 0,
            congestion_control: crate::tcp::congestion::CongestionControl::new(536),
            dup_acks: 0,
            recv_window_scale: None,
            sack_blocks: Vec::new(),
            recv_buffer: Vec::new(),
        },
    );
    udp_manager.flows.insert("10.0.2.15:5678".parse().unwrap(), (2, "2.2.2.2:53".parse().unwrap()));

    let tcp_info = tcp_manager.get_connections().map(ConnectionInfo::Tcp);
    let udp_info = udp_manager.get_connections().map(ConnectionInfo::Udp);
    let info: Vec<ConnectionInfo> = tcp_info.chain(udp_info).collect();

    assert_eq!(info.len(), 2);
    assert!(info.contains(&ConnectionInfo::Tcp(TcpConnectionInfo {
        local_addr: guest_addr,
        peer_addr: host_addr,
        state: tcp::state::State::Established,
    })));
    assert!(info.contains(&ConnectionInfo::Udp(UdpConnectionInfo {
        local_addr: "10.0.2.15:5678".parse().unwrap(),
        peer_addr: "2.2.2.2:53".parse().unwrap(),
    })));
}
