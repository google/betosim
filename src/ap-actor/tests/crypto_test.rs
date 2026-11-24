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
use netsim_packets::{
    CcmpHeader, DataFrameHeader, FrameControl, Ieee80211, MacAddress, SequenceControl,
};
use zerocopy::IntoBytes;

// ============================================================================
// Feature: Hostapd Cryptographic Primitives and CCMP Interoperability
// ============================================================================

const RFC3394_KEK: [u8; 16] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
];
const RFC3394_PLAIN: [u8; 16] = [
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
];
const RFC3394_CIPHER: [u8; 24] = [
    0x1f, 0xa6, 0x8b, 0x0a, 0x81, 0x12, 0xb4, 0x47, 0xae, 0xf3, 0x4b, 0xd8, 0xfb, 0x5a, 0x7b, 0x82,
    0x9d, 0x3e, 0x86, 0x23, 0x71, 0xd2, 0xcf, 0xe5,
];

// Scenario: Generate random bytes via BoringSSL FFI
#[test]
fn test_rand_bytes() {
    // Given a requested 16-byte length
    const LEN: usize = 16;
    // When BoringSSL generates random bytes
    let bytes = RandBytes(LEN);
    // Then a buffer of the requested length is returned
    assert_eq!(bytes.len(), LEN);
}

// Scenario: Compute HMAC-SHA1 against RFC 2202 test vector
#[test]
fn test_hmac_sha1() {
    // Given RFC 2202 test vector 1 key, payload, and expected MAC
    let key = [0x0b; 20];
    let data = b"Hi There";
    let expected = [
        0xb6, 0x17, 0x31, 0x86, 0x55, 0x05, 0x72, 0x64, 0xe2, 0x8b, 0xc0, 0xb6, 0xfb, 0x37, 0x8c,
        0x8e, 0xf1, 0x46, 0xbe, 0x00,
    ];
    // When HMAC-SHA1 is computed via BoringSSL
    let result = Hmac(DigestType::SHA1, &key, data);
    // Then the digest matches the RFC 2202 MAC
    assert_eq!(result, expected);
}

// Scenario: Compute SHA-256 digest against FIPS 180-2 test vector
#[test]
fn test_sha256() {
    // Given the FIPS 180-2 "abc" input and expected digest
    let data = b"abc";
    let expected = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];
    // When SHA-256 is computed via BoringSSL
    let result = Sha(DigestType::SHA256, data);
    // Then the 32-byte digest matches the FIPS 180-2 vector
    assert_eq!(result, expected);
}

// Scenario: Wrap a 128-bit key using AES Key Wrap (RFC 3394)
#[test]
fn test_aes_wrap() {
    // Given RFC 3394 Test Vector 1 KEK and plaintext key
    // When AesWrap wraps the plaintext key
    let cipher = AesWrap(&RFC3394_KEK, &RFC3394_PLAIN);
    // Then the wrapped ciphertext matches RFC 3394
    assert_eq!(cipher, RFC3394_CIPHER, "AesWrap failed");
}

// Scenario: Unwrap a 128-bit key using AES Key Wrap (RFC 3394)
#[test]
fn test_aes_unwrap() {
    // Given RFC 3394 Test Vector 1 KEK and wrapped ciphertext
    // When AesUnwrap unwraps the ciphertext
    let unwrapped = AesUnwrap(&RFC3394_KEK, &RFC3394_CIPHER);
    // Then the recovered key matches the original plaintext
    assert_eq!(unwrapped, RFC3394_PLAIN, "AesUnwrap failed");
}

// Scenario: Encrypt an 802.11 CCMP frame via BoringSSL FFI and decrypt via SharedKeyStore
#[test]
fn test_crypto_interop_boringssl_encrypt_rust_decrypt() {
    // Given a protected FromDS 802.11 Data frame, 802.11i CCMP nonce/AAD, and STA session
    const FC_PROTECTED_FROM_DS_DATA: u16 = 0x4208;
    const CCMP_EXT_IV_BIT: u8 = 0x20;
    const CCMP_MIC_LEN: usize = 8;

    let key = [0xAB; 16];
    let plain = b"InteropPayload";
    let dst = MacAddress::new([0x11; 6]);
    let bssid = MacAddress::new([0x22; 6]);
    let src = MacAddress::new([0x33; 6]);
    let [pn5, pn4, pn3, pn2, pn1, pn0] = [0xFF, 0xEE, 0xDD, 0xCC, 0xBB, 0xAA];

    let mac_hdr = DataFrameHeader::new(
        FrameControl::new(FC_PROTECTED_FROM_DS_DATA),
        0,
        dst,
        bssid,
        src,
        SequenceControl::new(0),
    );
    let ccmp_hdr = CcmpHeader { pn0, pn1, rsvd: 0, key_id: CCMP_EXT_IV_BIT, pn2, pn3, pn4, pn5 };

    // Independent 13-byte IEEE 802.11i CCMP nonce (Priority=0 || A2 || PN5..PN0)
    // and 22-byte 3-addr non-QoS CCMP AAD (Masked FC || A1 || A2 || A3 || Masked SC)
    let mut nonce = [0u8; 13];
    nonce[1..7].copy_from_slice(&bssid.bytes);
    nonce[7..13].copy_from_slice(&[pn5, pn4, pn3, pn2, pn1, pn0]);

    let mut aad = [0u8; 22];
    aad[0..2].copy_from_slice(&FC_PROTECTED_FROM_DS_DATA.to_le_bytes());
    aad[2..8].copy_from_slice(&dst.bytes);
    aad[8..14].copy_from_slice(&bssid.bytes);
    aad[14..20].copy_from_slice(&src.bytes);

    let store = SharedKeyStore::new();
    store.set_station_bssid(src, bssid);
    store.add_session(src, key.to_vec());

    // When BoringSSL seals the payload and SharedKeyStore decrypts the assembled MPDU
    let mut ciphertext = Vec::new();
    assert!(
        AesCcmEncrypt(&key, &nonce, &aad, plain, &mut ciphertext, CCMP_MIC_LEN),
        "BoringSSL Encrypt failed"
    );

    let final_packet = [mac_hdr.as_bytes(), ccmp_hdr.as_bytes(), ciphertext.as_slice()].concat();
    let encrypted_ieee = Ieee80211::decode(&final_packet).expect("Final Decode");

    let decrypted =
        store.try_decrypt(&encrypted_ieee).expect("SharedKeyStore::try_decrypt returned None");

    // Then the decrypted frame payload matches the original plaintext
    assert_eq!(&decrypted[encrypted_ieee.hdr_length()..], plain);
}
