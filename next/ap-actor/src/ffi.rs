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
#![allow(non_snake_case)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DigestType {
    MD5 = 0,
    SHA1 = 1,
    SHA256 = 2,
    SHA384 = 3,
    SHA512 = 4,
}

#[cxx::bridge(namespace = "netsim::hostap")]
#[allow(unreachable_pub)]
mod hostap_ffi {
    // SAFETY:
    // 1. Necessity: We use C++ FFI because the `bssl-crypto` Rust crate (and
    //    `bssl-sys` bindings) currently lack complete support for:
    //    - Generic AES-CCM (Required for WPA/EAPOL). `bssl-crypto` only supports
    //      GCM.
    //    - AES Key Wrap (RFC 3394). `AES_wrap_key` is not exposed in safe wrappers.
    //    - Legacy HMAC-MD5/SHA1/SHA384. `bssl-crypto` only supports
    //      HMAC-SHA256/512.
    //    Rewriting these using raw `bssl-sys` FFI is redundant given the existing
    // C++ shim.
    // 2. Soundness:
    //    - The `cxx` crate handles ABI compatibility and type safety for `Vec<u8>`
    //      <-> `std::vector`.
    //    - The C++ implementation (`crypto_ffi.cc`) checks all buffer sizes and
    //      OpenSSL return values, returning empty vectors or `false` on failure
    //      rather than triggering undefined behavior.
    //    - No raw pointers are exposed or manipulated without length checks in the
    //      C++ shim.
    unsafe extern "C++" {
        include!("crypto_ffi.h");

        fn Hmac(digest_type: u8, key: &[u8], data: &[u8]) -> Vec<u8>;
        fn Sha(digest_type: u8, data: &[u8]) -> Vec<u8>;
        fn AesWrap(kek: &[u8], plain: &[u8]) -> Vec<u8>;
        fn AesUnwrap(kek: &[u8], cipher: &[u8]) -> Vec<u8>;
        fn AesCcmEncrypt(
            key: &[u8],
            nonce: &[u8],
            aad: &[u8],
            plain: &[u8],
            out_data: &mut Vec<u8>,
            tag_len: usize,
        ) -> bool;
        fn AesCcmDecrypt(
            key: &[u8],
            nonce: &[u8],
            aad: &[u8],
            cipher_with_tag: &[u8],
            out_plain: &mut Vec<u8>,
            tag_len: usize,
        ) -> bool;
        fn RandBytes(len: usize) -> Vec<u8>;
        // PBKDF2
        fn Pbkdf2HmacSha1(password: &[u8], salt: &[u8], iterations: u32, key_len: usize)
        -> Vec<u8>;
        // SAE / ECC P-256
        fn EcP256CalculatePwe(password: &[u8], address1: &[u8], address2: &[u8]) -> Vec<u8>;
        fn EcP256PointMul(point_src: &[u8], scalar: &[u8]) -> Vec<u8>;
        fn EcP256PointAdd(point_a: &[u8], point_b: &[u8]) -> Vec<u8>;
        fn BnModAdd(a: &[u8], b: &[u8], m: &[u8]) -> Vec<u8>;
        fn BnModSub(a: &[u8], b: &[u8], m: &[u8]) -> Vec<u8>;
    }
}

pub fn Hmac(digest_type: DigestType, key: &[u8], data: &[u8]) -> Vec<u8> {
    self::hostap_ffi::Hmac(digest_type as u8, key, data)
}

pub fn Sha(digest_type: DigestType, data: &[u8]) -> Vec<u8> {
    self::hostap_ffi::Sha(digest_type as u8, data)
}

pub fn AesWrap(kek: &[u8], plain: &[u8]) -> Vec<u8> {
    self::hostap_ffi::AesWrap(kek, plain)
}

pub fn AesUnwrap(kek: &[u8], cipher: &[u8]) -> Vec<u8> {
    self::hostap_ffi::AesUnwrap(kek, cipher)
}

pub fn AesCcmEncrypt(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    plain: &[u8],
    out_data: &mut Vec<u8>,
    tag_len: usize,
) -> bool {
    self::hostap_ffi::AesCcmEncrypt(key, nonce, aad, plain, out_data, tag_len)
}

pub fn AesCcmDecrypt(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    cipher_with_tag: &[u8],
    out_plain: &mut Vec<u8>,
    tag_len: usize,
) -> bool {
    self::hostap_ffi::AesCcmDecrypt(key, nonce, aad, cipher_with_tag, out_plain, tag_len)
}

pub fn RandBytes(len: usize) -> Vec<u8> {
    self::hostap_ffi::RandBytes(len)
}

pub fn Pbkdf2HmacSha1(password: &[u8], salt: &[u8], iterations: u32, key_len: usize) -> Vec<u8> {
    self::hostap_ffi::Pbkdf2HmacSha1(password, salt, iterations, key_len)
}

pub fn EcP256CalculatePwe(password: &[u8], address1: &[u8], address2: &[u8]) -> Vec<u8> {
    self::hostap_ffi::EcP256CalculatePwe(password, address1, address2)
}

pub fn EcP256PointMul(point_src: &[u8], scalar: &[u8]) -> Vec<u8> {
    self::hostap_ffi::EcP256PointMul(point_src, scalar)
}

pub fn EcP256PointAdd(point_a: &[u8], point_b: &[u8]) -> Vec<u8> {
    self::hostap_ffi::EcP256PointAdd(point_a, point_b)
}

pub fn BnModAdd(a: &[u8], b: &[u8], m: &[u8]) -> Vec<u8> {
    self::hostap_ffi::BnModAdd(a, b, m)
}

pub fn BnModSub(a: &[u8], b: &[u8], m: &[u8]) -> Vec<u8> {
    self::hostap_ffi::BnModSub(a, b, m)
}
