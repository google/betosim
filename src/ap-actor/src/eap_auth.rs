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

use netsim_packets::{
    EAP_CODE_REQUEST, EAP_CODE_RESPONSE, EAP_CODE_SUCCESS, EAP_TYPE_IDENTITY, EAPOL_TYPE_PACKET,
    EapHeader, EapolHeader, MacAddr,
};
use zerocopy::{FromBytes, IntoBytes};

use crate::ApError;

#[derive(Debug, Clone, Copy, PartialEq)]
enum EapState {
    Idle,
    IdentityReqSent,
    Authenticated,
}

#[derive(Debug, Clone)]
pub(crate) struct EapAuthenticator {
    state: EapState,
    identifier: u8,
}

#[derive(Debug, Clone)]
pub(crate) enum EapOutput {
    Frame(Vec<u8>),
    Success,
    #[allow(dead_code)]
    Failure,
}

impl EapAuthenticator {
    pub(crate) fn new(bssid: MacAddr, sta_addr: MacAddr) -> Self {
        let _bssid = bssid;
        let _sta_addr = sta_addr;
        Self { state: EapState::Idle, identifier: 0 }
    }

    pub(crate) fn start(&mut self) -> Result<Vec<EapOutput>, ApError> {
        self.state = EapState::IdentityReqSent;
        self.identifier = self.identifier.wrapping_add(1);

        // Construct EAP-Request/Identity
        let header = EapHeader::new(EAP_CODE_REQUEST, self.identifier, 5); // 5 = Header(4) + Type(1)
        let mut body = Vec::new();
        body.extend_from_slice(header.as_bytes());
        body.push(EAP_TYPE_IDENTITY);

        Ok(vec![EapOutput::Frame(self.wrap_eap(body))])
    }

    pub(crate) fn handle_eap(&mut self, packet: &[u8]) -> Result<Vec<EapOutput>, ApError> {
        // Packet contains EAPOL Header + EAP Header + Data
        if packet.len() < 4 {
            return Ok(vec![]);
        }
        // EAP packet starts after EAPOL header (4 bytes)?
        // handle_frame caller usually strips LLC/SNAP(8) + EAPOL(4)?
        // Input is generic EAP packet.

        let (header, body) = match EapHeader::read_from_prefix(packet) {
            Ok(res) => res,
            Err(_) => return Err(ApError::InvalidFrame),
        };

        if header.code == EAP_CODE_RESPONSE && self.state == EapState::IdentityReqSent {
            // Expect Type Identity
            if body.first() == Some(&EAP_TYPE_IDENTITY) {
                // Accept ANY Identity -> Send Success
                self.state = EapState::Authenticated;
                self.identifier = self.identifier.wrapping_add(1);

                // Construct EAP-Success
                // Header only (Code 3, ID, Len 4)
                let success = EapHeader::new(EAP_CODE_SUCCESS, self.identifier, 4);

                return Ok(vec![
                    EapOutput::Frame(self.wrap_eap(success.as_bytes().to_vec())),
                    EapOutput::Success,
                ]);
            }
        }

        Ok(vec![])
    }

    fn wrap_eap(&self, eap_packet: Vec<u8>) -> Vec<u8> {
        // Wraps EAP Packet in EAPOL Frame
        let mut frame = Vec::new();
        let header = EapolHeader::new(1, EAPOL_TYPE_PACKET, eap_packet.len() as u16);
        frame.extend_from_slice(header.as_bytes());
        frame.extend_from_slice(&eap_packet);
        frame
    }
}
