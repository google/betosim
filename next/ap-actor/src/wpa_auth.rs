// Copyright 2025-2026 The Android Open Source Project
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
    EAPOL_KEY_DESC_TYPE_RSN, EAPOL_TYPE_KEY, EAPOL_VERSION, EapolHeader, EapolKeyFrame, MacAddr,
};
use tracing::{info, warn};
use zerocopy::{FromBytes, IntoBytes};

use crate::{
    ApError,
    ffi::{AesWrap, DigestType, Hmac},
};

#[derive(Debug, Clone, Copy, PartialEq)]
enum WpaState {
    Idle,
    PtKStart,       // Sent M1
    PtkNegotiating, // Sent M3
    PtkDone,
}

#[derive(Debug, Clone)]
pub(crate) struct WpaAuthenticator {
    bssid: MacAddr,
    sta_addr: MacAddr,
    gtk: [u8; 16],
    pmk: Vec<u8>, // Derived from PSK
    anonce: [u8; 32],
    snonce: [u8; 32],
    ptk: Vec<u8>,
    kck: Vec<u8>,
    kek: Vec<u8>,
    tk: Vec<u8>,
    replay_counter: u64,
    state: WpaState,
    rsn_ie: Vec<u8>,
}

const EAPOL_HEADER_LEN: usize = 4;
// KeyDesc(1) + KeyInfo(2) + KeyLen(2) + Replay(8) + Nonce(32) + IV(16) + RSC(8)
// + ID(8) = 77
const KEY_FRAME_FIXED_LEN: usize = 77;
const MIC_OFFSET: usize = EAPOL_HEADER_LEN + KEY_FRAME_FIXED_LEN; // 81
const MIC_LEN: usize = 16;
const MIC_END: usize = MIC_OFFSET + MIC_LEN; // 97

impl WpaAuthenticator {
    pub(crate) fn new(
        bssid: MacAddr,
        sta_addr: MacAddr,
        ssid: &[u8],
        psk: &[u8],
        rsn_ie: &[u8],
        gtk: [u8; 16],
    ) -> Self {
        // Derive PMK using PBKDF2-HMAC-SHA1(psk, ssid, 4096 iterations, 32 bytes)
        let pmk = crate::ffi::Pbkdf2HmacSha1(psk, ssid, 4096, 32);
        Self {
            bssid,
            sta_addr,
            pmk,
            anonce: [0; 32],
            snonce: [0; 32],
            gtk,
            ptk: Vec::new(),
            kck: Vec::new(),
            kek: Vec::new(),
            tk: Vec::new(),
            replay_counter: 0,
            state: WpaState::Idle,
            rsn_ie: rsn_ie.to_vec(),
        }
    }

    pub(crate) fn initiate_handshake(&mut self) -> Result<Vec<u8>, ApError> {
        self.state = WpaState::PtKStart;
        self.replay_counter += 1;

        let anonce_vec = crate::ffi::RandBytes(32);
        self.anonce.copy_from_slice(&anonce_vec);

        // Construct M1
        // Key Info bits:
        // Version 2 (HMAC-SHA1/AES) = 0x0002
        // Pairwise = 0x0008
        // Key Ack = 0x0080
        // Total = 0x008A
        self.build_eapol_frame(
            0x008A,
            &[0u8; 16], // MIC is 0 in M1
            0,          // Data Len 0
            &[],
        )
    }

    pub(crate) fn remove_session(&mut self, sta_addr: &MacAddr) {
        if self.sta_addr == *sta_addr {
            self.state = WpaState::Idle;
            self.ptk.clear();
            self.tk.clear();
            self.replay_counter = 0;
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum WpaOutput {
    Frame(Vec<u8>),
    InstallKey {
        key_index: u8,
        key: Vec<u8>,
        cipher: String, // "CCMP"
    },
}

impl WpaAuthenticator {
    // ... new ... initiate_handshake ...

    pub(crate) fn handle_eapol(&mut self, frame_data: &[u8]) -> Result<Vec<WpaOutput>, ApError> {
        // ...
        // Use zerocopy to parse header
        let (header, body) = match EapolHeader::read_from_prefix(frame_data) {
            Ok(res) => res,
            Err(_) => return Err(ApError::InvalidFrame),
        };

        if header.packet_type != EAPOL_TYPE_KEY {
            return Ok(vec![]);
        }

        // Parse Key Frame
        let (key_frame, _key_data) = match EapolKeyFrame::read_from_prefix(body) {
            Ok(res) => res,
            Err(_) => return Err(ApError::InvalidFrame),
        };

        // ... (snip parsers) ...

        match self.state {
            WpaState::PtKStart | WpaState::Idle => {
                // ... (M2 logic) ...
                self.snonce = key_frame.key_nonce;
                self.calc_ptk()?;

                if !self.verify_mic(frame_data, &key_frame.mic) {
                    warn!("WPA: MIC verification failed for M2 from {}", self.sta_addr);
                    return Ok(vec![]);
                }

                self.state = WpaState::PtkNegotiating;
                self.replay_counter += 1;

                // Key Info bits:
                // Version 2 (HMAC-SHA1/AES) = 0x0002
                // Pairwise = 0x0008
                // Install = 0x0040
                // Key Ack = 0x0080
                // Key MIC = 0x0100
                // Secure = 0x0200
                // Encrypted Key Data = 0x1000
                // Total = 0x13CA
                let m3_info = 0x13CA;

                // Construct RSN IE (CCMP-PSK) for M3
                // Use the configured RSN IE to ensure consistency
                let rsn_ie = &self.rsn_ie;

                // Use Global Network GTK
                let gtk_kde = crate::rsn::build_gtk_kde(&self.gtk, 1); // KeyID 1

                let mut key_data = Vec::new();
                key_data.extend_from_slice(rsn_ie);
                key_data.extend_from_slice(&gtk_kde);

                let mut encrypted_data = Vec::new();
                if !key_data.is_empty() {
                    let mut data_to_wrap = key_data.clone();
                    // Append padding (zeroes) to 8-byte boundary per AES Key Wrap (RFC 3394)
                    while data_to_wrap.len() % 8 != 0 {
                        data_to_wrap.push(0);
                    }
                    encrypted_data = AesWrap(&self.kek, &data_to_wrap);
                }

                info!("WPA: Sending M3 to {} (with GTK)", self.sta_addr);

                let m3_bytes = self.build_eapol_frame(
                    m3_info,
                    &[0u8; 16],
                    encrypted_data.len() as u16,
                    &encrypted_data,
                )?;

                let mic = self.calc_mic_for_frame(&m3_bytes);
                let mut final_m3 = m3_bytes;
                final_m3
                    .get_mut(MIC_OFFSET..MIC_END)
                    .ok_or(ApError::InvalidFrame)?
                    .copy_from_slice(&mic);

                Ok(vec![WpaOutput::Frame(final_m3)])
            }
            WpaState::PtkNegotiating => {
                if !self.verify_mic(frame_data, &key_frame.mic) {
                    warn!("WPA: MIC verification failed for M4 from {}", self.sta_addr);
                    return Ok(vec![]);
                }

                self.state = WpaState::PtkDone;
                info!("WPA: Handshake Complete. PTK Installed.");

                // Install Key
                let tk = self.tk.clone();
                Ok(vec![WpaOutput::InstallKey {
                    key_index: 0,
                    key: tk,
                    cipher: "CCMP".to_string(),
                }])
            }
            _ => Ok(vec![]),
        }
    }

    fn calc_ptk(&mut self) -> Result<(), ApError> {
        // PTK = PRF-384(PMK, "Pairwise key expansion", Min(AA,SA) || Max(AA,SA) ||
        // Min(ANonce,SNonce) || Max(ANonce,SNonce))
        let label = b"Pairwise key expansion";
        let mut data = Vec::new();

        let (min_addr, max_addr) = if self.bssid.bytes < self.sta_addr.bytes {
            (&self.bssid.bytes, &self.sta_addr.bytes)
        } else {
            (&self.sta_addr.bytes, &self.bssid.bytes)
        };

        let (min_nonce, max_nonce) = if self.anonce < self.snonce {
            (&self.anonce, &self.snonce)
        } else {
            (&self.snonce, &self.anonce)
        };

        data.extend_from_slice(min_addr);
        data.extend_from_slice(max_addr);
        data.extend_from_slice(min_nonce);
        data.extend_from_slice(max_nonce);

        // PRF-384 (48 bytes)
        let ptk_bytes = self.prf(&self.pmk, label, &data, 384);
        let Some(&[kck, kek, tk]) = ptk_bytes.as_chunks::<16>().0.first_chunk::<3>() else {
            return Err(ApError::Internal(
                format!("PTK shorter than 48 bytes: {}", ptk_bytes.len()).into(),
            ));
        };
        self.kck = kck.to_vec();
        self.kek = kek.to_vec();
        self.tk = tk.to_vec();
        self.ptk = ptk_bytes;
        Ok(())
    }

    fn prf(&self, key: &[u8], label: &[u8], data: &[u8], bit_len: usize) -> Vec<u8> {
        let mut result = Vec::new();
        let bytes_len = bit_len / 8;
        let mut i = 0u8;

        // PRF-X uses HMAC-SHA1
        while result.len() < bytes_len {
            let mut input = Vec::new();
            input.extend_from_slice(label);
            input.push(0);
            input.extend_from_slice(data);
            input.push(i);

            let hmac = Hmac(DigestType::SHA1, key, &input);
            result.extend_from_slice(&hmac);
            i += 1;
        }
        result.truncate(bytes_len);
        result
    }

    fn calc_mic_for_frame(&self, frame: &[u8]) -> Vec<u8> {
        // MIC is calculated over the EAPol frame with MIC field set to 0.
        // Hmac-SHA1-128 (first 16 bytes of SHA1) for CCMP.
        let mut mic = Hmac(DigestType::SHA1, &self.kck, frame);
        mic.truncate(16);
        mic
    }

    fn verify_mic(&self, frame: &[u8], received_mic: &[u8]) -> bool {
        // To verify, we must zero out the MIC field in the frame, calculate, and
        // compare.
        let mut frame_copy = frame.to_vec();
        // EAPOL Header (4) + Descr(1) + Info(2) + Len(2) + Replay(8) + Nonce(32) +
        // IV(16) + RSC(8) + ID(8) = 81 Range 81..97 is MIC.
        if frame_copy.len() < MIC_END {
            return false;
        }

        for b in frame_copy.iter_mut().take(MIC_END).skip(MIC_OFFSET) {
            *b = 0;
        }

        let calculated = self.calc_mic_for_frame(&frame_copy);
        calculated == received_mic
    }

    // Helper to build EAPOL frame
    fn build_eapol_frame(
        &self,
        key_info: u16,
        mic: &[u8],
        data_len: u16,
        key_data: &[u8],
    ) -> Result<Vec<u8>, ApError> {
        let mut frame = Vec::new();

        // EAPOL Packet Body Length: KeyDesc(1) + ... + KeyData(N)
        // KeyFrame is 95 bytes fixed part.
        let body_len = 95 + key_data.len();

        let header = EapolHeader {
            version: EAPOL_VERSION,
            packet_type: EAPOL_TYPE_KEY,
            length: (body_len as u16).to_be_bytes(),
        };

        frame.extend_from_slice(header.as_bytes());

        let key_frame = EapolKeyFrame {
            descriptor_type: EAPOL_KEY_DESC_TYPE_RSN,
            key_info: key_info.to_be_bytes(),
            key_len: 16u16.to_be_bytes(), // TK Len (CCMP=16)
            replay_counter: self.replay_counter.to_be_bytes(),
            key_nonce: self.anonce,
            key_iv: [0; 16],
            key_rsc: [0; 8],
            key_id: [0; 8],
            mic: mic.try_into().unwrap_or([0; 16]),
            key_data_len: data_len.to_be_bytes(),
        };
        frame.extend_from_slice(key_frame.as_bytes());
        frame.extend_from_slice(key_data);

        Ok(frame)
    }
}
