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

use bytes::Bytes;
use netsim_packets::{HwsimCmd, HwsimMsg, HwsimMsgHdr};

use crate::world;

#[tokio::test]
async fn test_pmsr_interception() {
    let mut world = world::World::new().await;

    // Given two chips are active
    world.given_a_chip(1).await;
    world.given_a_chip(2).await;

    // Grab the first chip which will act as the initiator
    let chip1 = &mut world.chips[0];

    // When the first chip sends an HWSIM_CMD_START_PMSR message
    let mut msg_bytes = vec![0u8; 16];
    msg_bytes[0..4].copy_from_slice(&20u32.to_le_bytes()); // len 16 (NlMsgHdr) + 4 (HwsimMsgHdr) = 20
    let hwsim_hdr = HwsimMsgHdr { hwsim_cmd: HwsimCmd::StartPmsr, hwsim_version: 0, reserved: 0 };
    msg_bytes.extend_from_slice(&hwsim_hdr.as_bytes());

    chip1.sink_tx.send(Bytes::from(msg_bytes)).await.unwrap();

    // Then the Wifi Actor intercepts it and immediately returns a REPORT_PMSR
    // message natively
    let mut resp_msg = None;
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(1));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            res = chip1.stream_rx.recv() => {
                let resp_bytes = res.expect("Stream closed");
                let msg = HwsimMsg::decode_full(&resp_bytes).unwrap();
                if msg.hwsim_hdr.hwsim_cmd == HwsimCmd::ReportPmsr {
                    resp_msg = Some(msg);
                    break;
                }
            }
            _ = &mut timeout => {
                panic!("Timeout waiting for PMSR response");
            }
        }
    }
    let resp_msg = resp_msg.unwrap();
    assert_eq!(resp_msg.hwsim_hdr.hwsim_cmd, HwsimCmd::ReportPmsr);
}
