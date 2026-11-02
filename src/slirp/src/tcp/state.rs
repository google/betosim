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
    collections::VecDeque,
    net::SocketAddr,
    time::{Duration, Instant},
};

use netsim_packets::MacAddr;
use serde::{Deserialize, Serialize};

use crate::tcp::congestion::CongestionControl;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum State {
    SynSent,
    Established,
    FinWait1,
    FinWait2,
    Closing,
    TimeWait,
    CloseWait,
    LastAck,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SendSequenceSpace {
    #[cfg(test)]
    pub(crate) una: u32,
    #[cfg(not(test))]
    pub(crate) una: u32,
    #[cfg(test)]
    pub(crate) nxt: u32,
    #[cfg(not(test))]
    pub(crate) nxt: u32,
    #[cfg(test)]
    pub(crate) iss: u32,
    #[cfg(not(test))]
    pub(crate) iss: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RecvSequenceSpace {
    #[cfg(test)]
    pub(crate) nxt: u32,
    #[cfg(not(test))]
    pub(crate) nxt: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TcpConnection {
    pub(crate) state: State,
    pub(crate) guest_mac: MacAddr,
    pub(crate) gateway_mac: MacAddr,
    pub(crate) guest_addr: SocketAddr,
    pub(crate) host_addr: SocketAddr,
    pub(crate) send: SendSequenceSpace,
    pub(crate) recv: RecvSequenceSpace,
    pub(crate) unacked: VecDeque<Vec<u8>>,
    pub(crate) retransmission_timeout: Duration,
    pub(crate) retransmissions: u32,
    pub(crate) mss: Option<u16>,
    pub(crate) srtt: Duration,
    pub(crate) rttvar: Duration,
    #[serde(skip)]
    pub(crate) sent_packets: VecDeque<(u32, Instant)>,
    pub(crate) window_size: u16,
    pub(crate) send_window: u32,
    pub(crate) congestion_control: CongestionControl,
    pub(crate) dup_acks: u32,
    pub(crate) recv_window_scale: Option<u8>,
    pub(crate) sack_blocks: Vec<(u32, u32)>,
    pub(crate) recv_buffer: Vec<u8>,
}
