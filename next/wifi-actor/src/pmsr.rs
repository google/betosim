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

use std::collections::HashMap;

use bytes::Bytes;
use netsim_model::{Chip, ChipId};
use netsim_packets::{HwsimCmd, HwsimMsg, HwsimMsgHdr};
use tracing::info;
use zerocopy::IntoBytes;

const SPEED_OF_LIGHT: f64 = 299_792_458.0; // m/s
const PICOSECONDS_PER_SECOND: u64 = 1_000_000_000_000;

// Netlink Attributes
const NL80211_ATTR_PEER_MEASUREMENTS: u16 = 314;
const NL80211_PMSR_ATTR_PEERS: u16 = 1;
const NL80211_PMSR_PEER_ATTR_ADDR: u16 = 1;
const NL80211_PMSR_PEER_ATTR_RESP: u16 = 4;
const NL80211_PMSR_RESP_ATTR_DATA: u16 = 1;
const NL80211_PMSR_TYPE_FTM: u16 = 1;
const NL80211_PMSR_FTM_RESP_ATTR_RTT_AVG: u16 = 7;
const NL80211_PMSR_FTM_RESP_ATTR_DIST_AVG: u16 = 9;

pub(crate) fn handle_start_pmsr(
    msg: &HwsimMsg,
    initiator_id: u32,
    active_chips: &HashMap<ChipId, Chip>,
) -> Option<Bytes> {
    info!("Received HWSIM_CMD_START_PMSR from {}", initiator_id);

    // Create a minimal REPORT_PMSR with one fake peer to satisfy the framework.

    let initiator_chip = active_chips.get(&ChipId(initiator_id))?;

    // Find all potential targets sorted by ID to maintain deterministic behavior.
    let mut chip_ids: Vec<_> = active_chips.keys().map(|k| k.0).collect();
    chip_ids.sort_unstable();

    // TODO: Parse MAC addresses from StartPmsr attributes and find matching chip
    // instead of picking the first other chip.
    let responder_chip = chip_ids
        .iter()
        .find(|&&id| id != initiator_id)
        .and_then(|&id| active_chips.get(&ChipId(id)));

    let responder_chip = responder_chip?;
    let distance = initiator_chip.pose.position.distance(&responder_chip.pose.position) as f64;
    let tof_seconds = distance / SPEED_OF_LIGHT;
    let rtt_picoseconds = (tof_seconds * 2.0 * PICOSECONDS_PER_SECOND as f64) as u64;
    let dist_mm = (distance * 1000.0) as i64;

    // We send back ReportPmsr
    let mut out_msg = Vec::new();

    // Reconstruct NlMsgHdr
    let mut resp_nl_hdr = msg.nl_hdr;
    resp_nl_hdr.nlmsg_len = 0; // will fix later
    out_msg.extend_from_slice(resp_nl_hdr.as_bytes());

    // Reconstruct HwsimMsgHdr with ReportPmsr
    let hwsim_hdr = HwsimMsgHdr { hwsim_cmd: HwsimCmd::ReportPmsr, hwsim_version: 0, reserved: 0 };
    out_msg.extend_from_slice(&hwsim_hdr.as_bytes());

    // NLA builder helpers
    fn add_attr(buffer: &mut Vec<u8>, attr_type: u16, data: &[u8]) {
        let len = data.len() as u16 + 4;
        buffer.extend_from_slice(&len.to_le_bytes());
        buffer.extend_from_slice(&attr_type.to_le_bytes());
        buffer.extend_from_slice(data);
        // padding
        let pad = (4 - (len % 4)) % 4;
        for _ in 0..pad {
            buffer.push(0);
        }
    }

    fn add_nested(buffer: &mut Vec<u8>, attr_type: u16, closure: impl FnOnce(&mut Vec<u8>)) {
        let mut nested = Vec::new();
        closure(&mut nested);
        let len = (4 + nested.len()) as u16;
        buffer.extend_from_slice(&len.to_le_bytes());
        buffer.extend_from_slice(&(attr_type | 0x8000).to_le_bytes()); // Nested flag
        buffer.extend_from_slice(&nested);
        let pad = (4 - (len % 4)) % 4;
        for _ in 0..pad {
            buffer.push(0);
        }
    }

    let mut payload = Vec::new();
    add_nested(&mut payload, NL80211_ATTR_PEER_MEASUREMENTS, |b1| {
        add_nested(b1, NL80211_PMSR_ATTR_PEERS, |b2| {
            // TODO: If supporting multiple distinct responders simultaneously in the
            // future, loop through the targeted MACs here and increment
            // peer_idx.
            let peer_idx = 1;
            add_nested(b2, peer_idx, |b3| {
                // Peer 1
                let fake_mac = if responder_chip.kind == netsim_model::ChipKind::WIFI {
                    let [id0, id1, id2, id3] = responder_chip.id.to_be_bytes();
                    [0x02, 0x00, id0, id1, id2, id3]
                } else {
                    [0x00, 0x11, 0x22, 0x33, 0x44, 0x55]
                };

                add_attr(b3, NL80211_PMSR_PEER_ATTR_ADDR, &fake_mac);
                add_nested(b3, NL80211_PMSR_PEER_ATTR_RESP, |b4| {
                    add_nested(b4, NL80211_PMSR_RESP_ATTR_DATA, |b5| {
                        add_nested(b5, 1, |b6| {
                            // Data 1
                            add_nested(b6, NL80211_PMSR_TYPE_FTM, |b7| {
                                add_attr(
                                    b7,
                                    NL80211_PMSR_FTM_RESP_ATTR_RTT_AVG,
                                    &rtt_picoseconds.to_le_bytes(),
                                );
                                add_attr(
                                    b7,
                                    NL80211_PMSR_FTM_RESP_ATTR_DIST_AVG,
                                    &dist_mm.to_le_bytes(),
                                );
                            });
                        });
                    });
                });
            });
        });
    });

    out_msg.extend_from_slice(&payload);

    // Fix total NlMsg length
    let total_len = out_msg.len() as u32;
    out_msg.get_mut(0..4)?.copy_from_slice(&total_len.to_le_bytes());

    Some(Bytes::from(out_msg))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use netsim_model::{ChipKind, DeviceId, Pose, Position};

    use super::*;

    #[test]
    fn test_handle_start_pmsr() {
        let mut active_chips = HashMap::new();

        let chip1 = Chip {
            id: 1,
            kind: ChipKind::WIFI,
            name: "chip1".into(),
            manufacturer: "".into(),
            product_name: "".into(),
            pose: Pose {
                position: Position { x: 0.0, y: 0.0, z: 0.0 },
                orientation: Default::default(),
            },
            device_id: DeviceId(1),
            variant: None,
            links: vec![],
            enabled: true,
        };

        let chip2 = Chip {
            id: 2,
            kind: ChipKind::WIFI,
            name: "chip2".into(),
            manufacturer: "".into(),
            product_name: "".into(),
            pose: Pose {
                position: Position { x: 3.0, y: 4.0, z: 0.0 },
                orientation: Default::default(),
            },
            device_id: DeviceId(2),
            variant: None,
            links: vec![],
            enabled: true,
        };

        active_chips.insert(ChipId(1), chip1);
        active_chips.insert(ChipId(2), chip2);

        // Dummy HwsimMsg
        // NlMsgHdr is 16 bytes
        let mut msg_bytes = vec![0u8; 16];
        msg_bytes[0..4].copy_from_slice(&20u32.to_le_bytes()); // len

        // HwsimMsgHdr is 4 bytes
        let hwsim_hdr =
            HwsimMsgHdr { hwsim_cmd: HwsimCmd::StartPmsr, hwsim_version: 0, reserved: 0 };
        msg_bytes.extend_from_slice(&hwsim_hdr.as_bytes());

        // Needs to have at least NlMsgHdr inside
        let msg = HwsimMsg::decode_full(&msg_bytes).unwrap();

        let response = handle_start_pmsr(&msg, 1, &active_chips);
        assert!(response.is_some());
        let response_bytes = response.unwrap();

        // Response should be a valid NlMsg
        let nl_len = u32::from_le_bytes(response_bytes[0..4].try_into().unwrap());
        assert_eq!(nl_len as usize, response_bytes.len());

        // Command should be ReportPmsr
        let resp_msg = HwsimMsg::decode_full(&response_bytes).unwrap();
        assert_eq!(resp_msg.hwsim_hdr.hwsim_cmd, HwsimCmd::ReportPmsr);
    }
}
