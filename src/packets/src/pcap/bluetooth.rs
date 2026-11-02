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

//! PCAP Writer utilities for Bluetooth Sniffer

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

// --- PCAP Pseudo-Header Flags ---
const BB_FLAG_BR_EDR: u16 = 0x0001;
const BB_FLAG_DECRYPTED: u16 = 0x0008;
const BB_FLAG_HAVE_LAP: u16 = 0x0010;
const BB_FLAG_HAVE_UAP: u16 = 0x0020;
const BB_FLAG_HAVE_HEC: u16 = 0x0080;
const BB_FLAG_HAVE_P_HEADER: u16 = 0x0400;
const BB_FLAG_PAYLOAD_DATA: u16 = 0x0800;

const DEFAULT_BB_FLAGS: u16 = BB_FLAG_BR_EDR
    | BB_FLAG_DECRYPTED
    | BB_FLAG_HAVE_LAP
    | BB_FLAG_HAVE_UAP
    | BB_FLAG_HAVE_HEC
    | BB_FLAG_HAVE_P_HEADER
    | BB_FLAG_PAYLOAD_DATA;

// --- Rootcanal / Packet Types ---
const RC_PACKET_TYPE_ACL: u8 = 0x01;
const RC_PACKET_TYPE_PAGE: u8 = 0x10;
const BB_PTYPE_FHS: u8 = 0b0010;
const BB_PTYPE_DM1: u8 = 0b0011;
const BB_PTYPE_DH5: u8 = 0b1111; // Used for multi-slot packets containing > 17 bytes
const MAX_ACL_PAYLOAD_LEN: usize = 1021; // Max BR/EDR ACL payload (3-DH5) per Bluetooth Core Spec Vol 2, Part B, 6.4.2

// --- Control / L2CAP Fields ---
const LLID_L2CAP_START: u8 = 2;
const FLOW_GO: u8 = 1;

// --- BLE Constants ---
const LE_ADV_ACCESS_ADDRESS: u32 = 0x8E89BED6;
const LE_LEGACY_ADVERTISING_PDU: u8 = 0x0B;
const MAX_LE_PAYLOAD_LEN: usize = 249; // Max BLE advertising payload (255 max 8-bit length - 6 bytes address)

#[repr(C, packed)]
#[derive(Debug, Copy, Clone, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
pub struct BredrBbFrameHeader {
    pub rf_channel: u8,
    pub signal_power: u8,
    pub noise_power: u8,
    pub access_code_offenses: u8,
    pub rate_transport: u8,
    pub corrected_header_bits: u8,
    pub corrected_payload_bits: u16,
    pub lap: u32,
    pub ref_lap: [u8; 3],
    pub ref_uap: u8,
    pub packet_header: u32,
    pub flags: u16,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
pub struct LeLlFrameHeader {
    pub access_address: u32,
    pub pdu_header: [u8; 2],
}

/// Reverses the bits in a byte. Used for Bluetooth HEC/CRC calculations
/// where the spec often defines shift registers operating on LSB first.
fn reverse_byte(b: u8) -> u8 {
    b.reverse_bits()
}

/// Computes the 8-bit Header Error Check (HEC) for Bluetooth Baseband packets.
/// The HEC LFSR is initialized with the Upper Address Part (UAP) of the master.
/// (Reference: Bluetooth Core Spec v5.0, Vol 2, Part B, Section 3.1.1)
fn header_error_check(uap: u8, mut data: u32) -> u8 {
    let mut value = reverse_byte(uap);
    for _ in 0..10 {
        let bit = ((value ^ (data as u8)) & 1) != 0;
        data >>= 1;
        value >>= 1;
        if bit {
            value ^= 0xe5;
        }
    }
    value
}

/// A simple bit-level writer to construct packed bitfields across byte
/// boundaries. Supports bitfields up to 64 bits (such as the 34-bit FHS parity
/// field per Bluetooth Core Spec Vol 2, Part B, 6.5.1).
struct BitWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> BitWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn write(&mut self, val: u64, bits: usize) {
        for i in 0..bits {
            let bit = (val >> i) & 1;
            let byte_idx = self.pos / 8;
            let bit_idx = self.pos % 8;
            if let Some(slot) = self.buf.get_mut(byte_idx) {
                *slot |= (bit as u8) << bit_idx;
            }
            self.pos += 1;
        }
    }
}

/// Constructs the 18-bit Bluetooth packet header and appends the 8-bit HEC.
/// The header contains: LT_ADDR (3 bits), Type (4 bits), Flow (1 bit),
/// ARQN (1 bit), SEQN (1 bit), and HEC (8 bits).
fn build_bt_packet_header(
    uap: u8,
    lt_addr: u8,
    packet_type: u8,
    flow: bool,
    arqn: bool,
    seqn: bool,
) -> u32 {
    let mut header: u32 = (lt_addr as u32 & 0x7)
        | (((packet_type as u32) & 0xF) << 3)
        | ((flow as u32) << 7)
        | ((arqn as u32) << 8)
        | ((seqn as u32) << 9);
    header |= (header_error_check(uap, header) as u32) << 10;
    header
}

pub fn create_bredr_bb_packet(packet: &[u8]) -> Option<Vec<u8>> {
    let (&p_type, rest) = packet.split_first()?;
    let is_page = match p_type {
        RC_PACKET_TYPE_PAGE => true,
        RC_PACKET_TYPE_ACL => false,
        _ => return None,
    };

    let (&address, rest) = rest.split_first_chunk::<6>()?;
    let (&ref_lap, &[uap, a4, a5]) = (address.first_chunk::<3>()?, address.last_chunk::<3>()?);
    let [l0, l1, l2] = ref_lap;
    let lap = u32::from_le_bytes([l0, l1, l2, 0]);
    let nap = u16::from_le_bytes([a4, a5]);

    let payload = if is_page { &[] } else { rest };
    if payload.len() > MAX_ACL_PAYLOAD_LEN {
        return None;
    }
    let packet_type_bits = if is_page {
        BB_PTYPE_FHS
    } else if payload.len() <= 17 {
        BB_PTYPE_DM1 // 1-byte payload header fits up to 17 bytes
    } else {
        BB_PTYPE_DH5 // 2-byte payload header fits up to 1021 bytes
    };

    let header = BredrBbFrameHeader {
        rf_channel: 0,
        signal_power: 0,
        noise_power: 0,
        access_code_offenses: 0,
        rate_transport: 0x30, // 1 Mbps Basic Rate
        corrected_header_bits: 0,
        corrected_payload_bits: 0,
        lap,
        ref_lap,
        ref_uap: uap,
        packet_header: build_bt_packet_header(uap, 0, packet_type_bits, true, true, true),
        flags: DEFAULT_BB_FLAGS,
    };

    let mut bb = Vec::with_capacity(64);
    bb.extend_from_slice(header.as_bytes());

    if is_page {
        let (&class_of_device, _) = rest.split_first_chunk::<3>()?;

        // 18-byte packed FHS (Frequency Hop Synchronization) payload
        // Structure is defined in Bluetooth Core Spec Vol 2, Part B, Section 6.5.1
        // (Table 6.13, 144 bits total = 18 bytes):
        // - Parity bits: 34 bits (bits 0..33, derived from sender BD_ADDR LAP)
        // - LAP: 24 bits (bits 34..57)
        // - Undefined / EIR / Reserved: 2 bits (bits 58..59)
        // - SR: 2 bits (bits 60..61)
        // - SP: 2 bits (bits 62..63)
        // - UAP: 8 bits (bits 64..71, byte 8)
        // - NAP: 16 bits (bits 72..87, bytes 9..10)
        // - Class of Device: 24 bits (bits 88..111, bytes 11..13)
        // - LT_ADDR: 3 bits (bits 112..114)
        // - Clock: 26 bits (bits 115..140)
        // - Page Scan Mode: 3 bits (bits 141..143)
        let mut fhs = [0u8; 18];
        let mut w = BitWriter::new(&mut fhs);

        w.write(0, 34); // Parity bits (34 bits per Bluetooth Core Spec Vol 2, Part B, 6.5.1)
        w.write(lap.into(), 24); // LAP (bits 34..57)
        w.write(0, 6); // Undefined (2) / SR (2) / SP (2) = 6 bits (total 64 bits = 8 bytes)
        w.write(uap.into(), 8); // UAP (byte 8)
        w.write(nap.into(), 16); // NAP (bytes 9..10)
        for byte in class_of_device {
            w.write(byte.into(), 8);
        }
        w.write(1, 3); // LT_ADDR
        w.write(0, 26); // Clock
        w.write(0, 3); // Page Scan Mode

        bb.extend_from_slice(&fhs);
        bb.extend_from_slice(&0u16.to_le_bytes()); // CRC
    } else {
        // ACL payload header: LLID (2 bits), FLOW (1 bit), length (5 or 10 bits)
        // per Bluetooth Core Spec Vol 2, Part B, Section 6.4.2
        let (header_len, length_bits) = if payload.len() <= 17 { (1, 5) } else { (2, 10) };
        let mut p_header = [0u8; 2];
        let mut w = BitWriter::new(&mut p_header);
        w.write(LLID_L2CAP_START.into(), 2);
        w.write(FLOW_GO.into(), 1);
        w.write(payload.len() as u64, length_bits);
        bb.extend_from_slice(p_header.get(..header_len)?);

        bb.extend_from_slice(payload);
        bb.extend_from_slice(&0u16.to_le_bytes()); // CRC
    }

    Some(bb)
}

/// Packages a raw Link Layer packet into a `LINKTYPE_BLUETOOTH_LE_LL` PCAP
/// format. The LE_LL pseudo-header requires the Access Address and a 2-byte PDU
/// header.
pub fn create_le_ll_packet(packet: &[u8]) -> Option<Vec<u8>> {
    let (&pdu_type, rest) = packet.split_first()?;
    if pdu_type != LE_LEGACY_ADVERTISING_PDU {
        return None;
    }

    let (header_bytes, adv_data) = rest.split_first_chunk::<15>()?;
    let &source_address = header_bytes.first_chunk::<6>()?;
    let &[tx_add_raw, rx_add_raw, adv_type_raw] = header_bytes.last_chunk::<3>()?;
    let tx_add = tx_add_raw & 1;
    let rx_add = rx_add_raw & 1;
    let adv_type = adv_type_raw & 0x0F;
    if adv_data.len() > MAX_LE_PAYLOAD_LEN {
        return None;
    }

    // 2-byte LE advertising channel PDU header:
    // PDU Type (4 bits), RFU (1 bit), ChSel (1 bit), TxAdd (1 bit), RxAdd (1 bit), Length (8 bits)
    // per Bluetooth Core Spec Vol 6, Part B, Section 2.3 and Wireshark dissector (packet-btle.c)
    let mut pdu_header = [0u8; 2];
    let mut w = BitWriter::new(&mut pdu_header);
    w.write(adv_type.into(), 4);
    w.write(0, 1); // RFU (bit 4)
    w.write(0, 1); // ChSel (bit 5)
    w.write(tx_add.into(), 1); // TxAdd (bit 6 per Wireshark / Core Spec)
    w.write(rx_add.into(), 1); // RxAdd (bit 7 per Wireshark / Core Spec)
    w.write((6 + adv_data.len()) as u64, 8); // Length

    let header = LeLlFrameHeader { access_address: LE_ADV_ACCESS_ADDRESS, pdu_header };

    Some([header.as_bytes(), &source_address, adv_data, &[0, 0, 0]].concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reverse_byte() {
        assert_eq!(reverse_byte(0x00), 0x00);
        assert_eq!(reverse_byte(0xFF), 0xFF);
        assert_eq!(reverse_byte(0x80), 0x01);
        assert_eq!(reverse_byte(0x01), 0x80);
        assert_eq!(reverse_byte(0b1011_0010), 0b0100_1101);
        for b in 0..=u8::MAX {
            assert_eq!(reverse_byte(b), b.reverse_bits());
        }
    }

    #[test]
    fn test_bit_writer_cross_byte_boundaries() {
        let mut buf = [0u8; 4];
        let mut writer = BitWriter::new(&mut buf);
        writer.write(0b101, 3);
        writer.write(0b110011, 6);
        writer.write(0xAB, 8);
        assert_eq!(writer.pos, 17);
        let packed = u32::from_le_bytes(buf);
        assert_eq!(packed & 0b111, 0b101);
        assert_eq!((packed >> 3) & 0b11_1111, 0b110011);
        assert_eq!((packed >> 9) & 0xFF, 0xAB);
    }

    #[test]
    fn test_build_bt_packet_header_and_hec() {
        let uap = 0x42;
        let header = build_bt_packet_header(uap, 0b101, BB_PTYPE_DM1, true, false, true);
        assert_eq!(header & 0x7, 0b101);
        assert_eq!((header >> 3) & 0xF, BB_PTYPE_DM1 as u32);
        assert_eq!((header >> 7) & 1, 1);
        assert_eq!((header >> 8) & 1, 0);
        assert_eq!((header >> 9) & 1, 1);
        let expected_hec = header_error_check(uap, header & 0x3FF);
        assert_eq!((header >> 10) & 0xFF, expected_hec as u32);
    }

    #[test]
    fn test_create_bredr_bb_packet_rejects_short_or_unknown() {
        assert!(create_bredr_bb_packet(&[]).is_none());
        assert!(create_bredr_bb_packet(&[RC_PACKET_TYPE_ACL; 6]).is_none());
        // Unknown packet type
        assert!(create_bredr_bb_packet(&[0x99, 1, 2, 3, 4, 5, 6, 7]).is_none());
        // Page packet shorter than 10 bytes
        assert!(create_bredr_bb_packet(&[RC_PACKET_TYPE_PAGE, 1, 2, 3, 4, 5, 6, 7, 8]).is_none());
    }

    #[test]
    fn test_create_bredr_bb_packet_page_fhs() {
        // [type=PAGE, addr(6 bytes: LAP=0x332211, UAP=0x44, NAP=0x6655), CoD(3 bytes: 0xAA, 0xBB, 0xCC)]
        let raw = [RC_PACKET_TYPE_PAGE, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0xAA, 0xBB, 0xCC];
        let bb = create_bredr_bb_packet(&raw).expect("valid PAGE packet should succeed");
        // 22-byte BredrBbFrameHeader + 18-byte FHS + 2-byte CRC = 42 bytes
        assert_eq!(bb.len(), 42);

        let (hdr, rest) =
            BredrBbFrameHeader::ref_from_prefix(&bb).expect("valid BredrBbFrameHeader prefix");
        assert_eq!(hdr.rate_transport, 0x30);
        assert_eq!({ hdr.lap }, 0x0033_2211);
        assert_eq!(hdr.ref_lap, [0x11, 0x22, 0x33]);
        assert_eq!(hdr.ref_uap, 0x44);
        assert_eq!({ hdr.flags }, DEFAULT_BB_FLAGS);
        assert_eq!(
            { hdr.packet_header },
            build_bt_packet_header(0x44, 0, BB_PTYPE_FHS, true, true, true)
        );

        // FHS bits 0..33 hold 34-bit Parity (all zero)
        assert_eq!(&rest[0..4], &[0u8; 4]);
        // FHS bits 34..57 hold LAP (0x332211), bits 58..63 are zero (64 bits = 8 bytes total)
        let fhs_u64 = u64::from_le_bytes(rest[0..8].try_into().unwrap());
        assert_eq!(fhs_u64 & 0x0000_0003_FFFF_FFFF, 0); // 34 parity bits
        assert_eq!((fhs_u64 >> 34) & 0xFF_FFFF, 0x0033_2211); // 24 LAP bits
        assert_eq!(fhs_u64 >> 58, 0); // 6 undefined/SR/SP bits

        // Byte 8: UAP
        assert_eq!(rest[8], 0x44);
        // Bytes 9..11: NAP (0x6655 in little-endian)
        assert_eq!(&rest[9..11], &[0x55, 0x66]);
        // Bytes 11..14: Class of Device (0xCCBBAA in little-endian)
        assert_eq!(&rest[11..14], &[0xAA, 0xBB, 0xCC]);
        // Bytes 14..18: LT_ADDR (1), Clock (0), Page Scan Mode (0)
        assert_eq!(&rest[14..18], &[0x01, 0, 0, 0]);
        // Trailing 2 bytes: CRC
        assert_eq!(&rest[18..20], &[0, 0]);
    }

    #[test]
    fn test_create_bredr_bb_packet_acl_dm1_short_payload() {
        let payload = b"hello bt"; // 8 bytes <= 17 -> DM1 with 1-byte payload header
        let mut raw = vec![RC_PACKET_TYPE_ACL, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        raw.extend_from_slice(payload);

        let bb = create_bredr_bb_packet(&raw).expect("valid ACL DM1 packet should succeed");
        // 22-byte header + 1-byte payload header + 8-byte payload + 2-byte CRC = 33 bytes
        assert_eq!(bb.len(), 22 + 1 + payload.len() + 2);

        let (hdr, rest) =
            BredrBbFrameHeader::ref_from_prefix(&bb).expect("valid BredrBbFrameHeader prefix");
        assert_eq!(
            { hdr.packet_header },
            build_bt_packet_header(0x04, 0, BB_PTYPE_DM1, true, true, true)
        );
        let expected_p_hdr = LLID_L2CAP_START | (FLOW_GO << 2) | ((payload.len() as u8) << 3);
        assert_eq!(rest[0], expected_p_hdr);
        assert_eq!(&rest[1..1 + payload.len()], payload);
        assert_eq!(&rest[1 + payload.len()..], &[0, 0]);
    }

    #[test]
    fn test_create_bredr_bb_packet_acl_dh5_long_payload() {
        for len in [20usize, 600usize, MAX_ACL_PAYLOAD_LEN] {
            let payload = vec![0x5Au8; len]; // > 17 -> DH5 with 2-byte (10-bit length) payload header
            let mut raw = vec![RC_PACKET_TYPE_ACL, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60];
            raw.extend_from_slice(&payload);

            let bb = create_bredr_bb_packet(&raw).expect("valid ACL DH5 packet should succeed");
            // 22-byte header + 2-byte payload header + payload + 2-byte CRC
            assert_eq!(bb.len(), 22 + 2 + payload.len() + 2);

            let (hdr, rest) =
                BredrBbFrameHeader::ref_from_prefix(&bb).expect("valid BredrBbFrameHeader prefix");
            assert_eq!(
                { hdr.packet_header },
                build_bt_packet_header(0x40, 0, BB_PTYPE_DH5, true, true, true)
            );
            let expected_p_hdr: u16 = (LLID_L2CAP_START as u16)
                | ((FLOW_GO as u16) << 2)
                | ((payload.len() as u16 & 0x3FF) << 3);
            assert_eq!(&rest[0..2], &expected_p_hdr.to_le_bytes());
            assert_eq!(&rest[2..2 + payload.len()], &payload[..]);
            assert_eq!(&rest[2 + payload.len()..], &[0, 0]);
        }
    }

    #[test]
    fn test_create_bredr_bb_packet_rejects_payload_too_long() {
        let payload = vec![0x5Au8; MAX_ACL_PAYLOAD_LEN + 1];
        let mut raw = vec![RC_PACKET_TYPE_ACL, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60];
        raw.extend_from_slice(&payload);
        assert!(create_bredr_bb_packet(&raw).is_none());
    }

    #[test]
    fn test_create_le_ll_packet() {
        // Rejects < 16 bytes or non-legacy advertising PDU type
        assert!(create_le_ll_packet(&[LE_LEGACY_ADVERTISING_PDU; 15]).is_none());
        assert!(create_le_ll_packet(&[0x00; 16]).is_none());

        let mut raw = vec![0u8; 16];
        raw[0] = LE_LEGACY_ADVERTISING_PDU;
        raw[1..7].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        raw[13] = 1; // tx_add = 1
        raw[14] = 1; // rx_add = 1
        raw[15] = 0x02; // adv_type = ADV_NONCONN_IND
        let adv_data = [0x02, 0x01, 0x06];
        raw.extend_from_slice(&adv_data);

        let ll = create_le_ll_packet(&raw).expect("valid LE legacy advertising PDU should succeed");
        // 6-byte LeLlFrameHeader + 6-byte source address + 3-byte adv_data + 3-byte CRC = 18 bytes
        assert_eq!(ll.len(), 6 + 6 + adv_data.len() + 3);

        let (hdr, rest) =
            LeLlFrameHeader::ref_from_prefix(&ll).expect("valid LeLlFrameHeader prefix");
        assert_eq!({ hdr.access_address }, LE_ADV_ACCESS_ADDRESS);
        assert_eq!(hdr.pdu_header[0], 0x02 | (1 << 6) | (1 << 7));
        assert_eq!(hdr.pdu_header[1], (6 + adv_data.len()) as u8);
        assert_eq!(&rest[0..6], &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        assert_eq!(&rest[6..6 + adv_data.len()], &adv_data);
        assert_eq!(&rest[6 + adv_data.len()..], &[0, 0, 0]);
    }

    #[test]
    fn test_create_le_ll_packet_rejects_payload_too_long() {
        let mut raw = vec![0u8; 16];
        raw[0] = LE_LEGACY_ADVERTISING_PDU;
        raw[1..7].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        raw[13] = 1;
        raw[14] = 1;
        raw[15] = 0x02;
        let adv_data = vec![0x00; MAX_LE_PAYLOAD_LEN + 1];
        raw.extend_from_slice(&adv_data);
        assert!(create_le_ll_packet(&raw).is_none());
    }

    #[test]
    fn test_create_le_ll_packet_max_payload() {
        let mut raw = vec![0u8; 16];
        raw[0] = LE_LEGACY_ADVERTISING_PDU;
        raw[1..7].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        raw[13] = 1;
        raw[14] = 1;
        raw[15] = 0x02;
        let adv_data = vec![0x00; MAX_LE_PAYLOAD_LEN];
        raw.extend_from_slice(&adv_data);
        let ll = create_le_ll_packet(&raw).expect("max payload size should succeed");
        assert_eq!(ll.len(), 6 + 6 + MAX_LE_PAYLOAD_LEN + 3);
        let (hdr, _) = LeLlFrameHeader::ref_from_prefix(&ll).unwrap();
        assert_eq!(hdr.pdu_header[1], 255);
    }
}
