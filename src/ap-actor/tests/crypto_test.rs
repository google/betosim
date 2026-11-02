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

use ap_actor::{
    SharedKeyStore,
    ffi::{AesCcmEncrypt, AesUnwrap, AesWrap, DigestType, Hmac, RandBytes, Sha},
};
use netsim_packets::{Ieee80211, MacAddress};

#[test]
fn test_rand_bytes() {
    let bytes = RandBytes(16);
    assert_eq!(bytes.len(), 16);
}

#[test]
fn test_hmac_sha1() {
    let key = vec![0x0b; 20];
    let data = b"Hi There".to_vec();
    // HMAC-SHA1("Hi There", 0x0b...0x0b) = b617318655057264e28bc0b6fb378c8ef146be00
    let expected = vec![
        0xb6, 0x17, 0x31, 0x86, 0x55, 0x05, 0x72, 0x64, 0xe2, 0x8b, 0xc0, 0xb6, 0xfb, 0x37, 0x8c,
        0x8e, 0xf1, 0x46, 0xbe, 0x00,
    ];
    let result = Hmac(DigestType::SHA1, &key, &data);
    assert_eq!(result, expected);
}

#[test]
fn test_sha256() {
    let data = b"abc".to_vec();
    // SHA256("abc") =
    // ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
    let expected = vec![
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];
    let result = Sha(DigestType::SHA256, &data);
    assert_eq!(result, expected);
}

#[test]
fn test_aes_wrap_unwrap() {
    // RFC 3394 Test Vector 1
    // KEK = 000102030405060708090A0B0C0D0E0F
    // Plain = 00112233445566778899AABBCCDDEEFF
    // Cipher = 1FA68B0A8112B447 AEF34BD8FB5A7B82 9D3E862371D2CFE5
    let kek = (0..16).collect::<Vec<u8>>();
    let plain = vec![
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff,
    ];
    let expected_cipher = vec![
        0x1f, 0xa6, 0x8b, 0x0a, 0x81, 0x12, 0xb4, 0x47, 0xae, 0xf3, 0x4b, 0xd8, 0xfb, 0x5a, 0x7b,
        0x82, 0x9d, 0x3e, 0x86, 0x23, 0x71, 0xd2, 0xcf, 0xe5,
    ];

    let cipher = AesWrap(&kek, &plain);
    assert_eq!(cipher, expected_cipher, "AesWrap failed");

    let unwrapped = AesUnwrap(&kek, &cipher);
    assert_eq!(unwrapped, plain, "AesUnwrap failed");
}

#[test]
fn test_crypto_interop_boringssl_encrypt_rust_decrypt() {
    // 1. Setup Keys and Data
    let key = vec![0xAB; 16]; // 128-bit key
    let nonce = vec![0x11; 13]; // 13-byte Nonce
    let aad = vec![0xAA, 0xBB, 0xCC]; // Some AAD
    let plain = b"InteropPayload".to_vec();

    // 2. Encrypt using BoringSSL (via FFI)
    let mut ciphertext = Vec::new();
    let tag_len = 8; // CCMP uses 8-byte MIC

    let success = AesCcmEncrypt(&key, &nonce, &aad, &plain, &mut ciphertext, tag_len);
    assert!(success, "BoringSSL Encryption failed");

    // 3. Construct an Ieee80211 frame that SharedKeyStore expects
    //
    // The SharedKeyStore implementation requires a specific Nonce construction to
    // successfully decrypt: Nonce = Priority(0) || A2 || PN
    //
    // We choose A2 and PN such that they form a valid Nonce.
    //
    // A2 = 0x22...22
    // PN = 0x010203040506 (big-endian bytes [01, 02, 03, 04, 05, 06] but treated as
    // u64/LE depending on impl) We explicitly define PN bytes below.

    let a2 = MacAddress::new([0x22; 6]);
    let pn5 = 0xFF;
    let pn4 = 0xEE;
    let pn3 = 0xDD;
    let pn2 = 0xCC;
    let pn1 = 0xBB;
    let pn0 = 0xAA;

    // CCMP Header: [PN0, PN1, Rsvd, KeyID_ExtIV, PN2, PN3, PN4, PN5]
    let ccmp_header = [pn0, pn1, 0x00, 0x20, pn2, pn3, pn4, pn5];

    // Nonce Construction (Standard CCMP):
    // Byte 0: Flag (Priority) = 0
    // Byte 1-6: A2
    // Byte 7-12: PN (PN5, PN4, PN3, PN2, PN1, PN0)
    let mut real_nonce = vec![0u8; 13];
    real_nonce[0] = 0;
    real_nonce[1..7].copy_from_slice(&a2.bytes);
    real_nonce[7] = pn5;
    real_nonce[8] = pn4;
    real_nonce[9] = pn3;
    real_nonce[10] = pn2;
    real_nonce[11] = pn1;
    real_nonce[12] = pn0;

    // AAD Construction (Standard CCMP):
    // SharedKeyStore.try_decrypt calls ieee80211.get_aad() internally.
    // We use the same method here to ensure BoringSSL uses the exact same AAD.

    // We need a basic frame first.
    let a1 = MacAddress::new([0x11; 6]); // Dest (our station)
    let a3 = MacAddress::new([0x33; 6]); // BSSID/Source
    // Data frame from AP to STA (FromDS=1, ToDS=0)
    let mut frame_bytes = Vec::new();
    // FC: Type=Data(2), Subtype=Data(0), ToDS=0, FromDS=1, Protected=1
    // Type/Subtype = 0x08 (Data)
    // Flags = 0x42 (FromDS | Protected) -> Little endian encoding of FC?
    // FC is u16.
    // Byte 0: Version(2)|Type(2)|Subtype(4). b0000 1000 = 0x08.
    // Byte 1: ToDS(1)|FromDS(1)|MoreFrag...|Protected(1)|...
    // ToDS=0, FromDS=1, Protected=1 => b0100 0010 = 0x42.
    frame_bytes.push(0x08);
    frame_bytes.push(0x42);
    // Duration
    frame_bytes.extend_from_slice(&[0x00, 0x00]);
    // A1 (Dest)
    frame_bytes.extend_from_slice(&a1.bytes);
    // A2 (BSSID)
    frame_bytes.extend_from_slice(&a2.bytes);
    // A3 (Source)
    frame_bytes.extend_from_slice(&a3.bytes);
    // SC
    frame_bytes.extend_from_slice(&[0x00, 0x00]);

    let full_frame_for_aad = frame_bytes.clone();
    let temp_ieee = Ieee80211::decode(&full_frame_for_aad).expect("Header Decode");
    let real_aad = temp_ieee.get_aad();

    // Encrypt with BoringSSL
    let mut real_ciphertext = Vec::new();
    let encrypt_success =
        AesCcmEncrypt(&key, &real_nonce, &real_aad, &plain, &mut real_ciphertext, 8);
    assert!(encrypt_success, "BoringSSL Encrypt failed");

    // Construct Full Encrypted Frame for SharedKeyStore
    // Header || CCMP Header || Ciphertext (includes MIC)
    let mut final_packet = frame_bytes; // Header
    final_packet.extend_from_slice(&ccmp_header);
    final_packet.extend_from_slice(&real_ciphertext);

    let encrypted_ieee = Ieee80211::decode(&final_packet).expect("Final Decode");

    // Setup SharedKeyStore
    let store = SharedKeyStore::new();

    // The SharedKeyStore looks up the session using the 'Source' address.
    // For a FromDS=1 frame (AP -> STA), the Source Address is A3.
    // (A1=Dest, A2=BSSID/Transmitter, A3=Source).

    // Register session for A3 (Source)
    store.add_session(a3, key.clone());

    // DECRYPT with Rust
    let decrypted_bytes_opt = store.try_decrypt(&encrypted_ieee);
    assert!(decrypted_bytes_opt.is_some(), "Rust Decrypt returned None");

    let decrypted_bytes = decrypted_bytes_opt.unwrap();
    // Verify Payload
    // The result of try_decrypt is: Header || Plaintext
    // So we need to slice it.
    let hdr_len = encrypted_ieee.hdr_length();
    let decrypted_payload = &decrypted_bytes[hdr_len..];

    assert_eq!(decrypted_payload, plain, "Decrypted payload mismatch!");
}
