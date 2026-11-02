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

#include "crypto_ffi.h"

#include <openssl/aead.h>
#include <openssl/aes.h>
#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/evp.h>
#include <openssl/hmac.h>
#include <openssl/obj_mac.h>
#include <openssl/rand.h>
#include <openssl/sha.h>

#include <cstring>
#include <iostream>
#include <random>
#include <vector>

namespace netsim {
namespace hostap {

using rust::Vec;

static const EVP_MD *GetDigest(DigestType type) {
  switch (type) {
    case DigestType::MD5:
      return EVP_md5();
    case DigestType::SHA1:
      return EVP_sha1();
    case DigestType::SHA256:
      return EVP_sha256();
    case DigestType::SHA384:
      return EVP_sha384();
    case DigestType::SHA512:
      return EVP_sha512();
    default:
      return nullptr;
  }
}

// HMAC
Vec<uint8_t> Hmac(uint8_t digest_type_u8, rust::Slice<const uint8_t> key,
                  rust::Slice<const uint8_t> data) {
  DigestType digest_type = static_cast<DigestType>(digest_type_u8);
  Vec<uint8_t> result;
  const EVP_MD *md = GetDigest(digest_type);
  if (!md) {
    return result;
  }

  unsigned int len = EVP_MD_size(md);
  result.reserve(len);
  std::vector<uint8_t> buf(len);

  if (HMAC(md, key.data(), key.size(), data.data(), data.size(), buf.data(),
           &len)) {
    for (uint8_t b : buf) result.push_back(b);
  }
  return result;
}

// SHA Hash
Vec<uint8_t> Sha(uint8_t digest_type_u8, rust::Slice<const uint8_t> data) {
  DigestType digest_type = static_cast<DigestType>(digest_type_u8);
  Vec<uint8_t> result;
  const EVP_MD *md = GetDigest(digest_type);
  if (!md) return result;

  EVP_MD_CTX *ctx = EVP_MD_CTX_new();
  if (!ctx) return result;

  unsigned int len = EVP_MD_size(md);
  std::vector<uint8_t> buf(len);

  if (EVP_DigestInit_ex(ctx, md, nullptr) &&
      EVP_DigestUpdate(ctx, data.data(), data.size()) &&
      EVP_DigestFinal_ex(ctx, buf.data(), &len)) {
    for (uint8_t b : buf) result.push_back(b);
  }
  EVP_MD_CTX_free(ctx);
  return result;
}

// AES-Wrap (RFC 3394)
Vec<uint8_t> AesWrap(rust::Slice<const uint8_t> kek,
                     rust::Slice<const uint8_t> plain) {
  Vec<uint8_t> result;
  if (plain.size() % 8 != 0 || plain.size() < 16) return result;

  AES_KEY aes_key;
  if (AES_set_encrypt_key(kek.data(), kek.size() * 8, &aes_key) < 0) {
    return result;
  }

  size_t out_len = plain.size() + 8;
  std::vector<uint8_t> out_buf(out_len);

  int ret = AES_wrap_key(&aes_key, nullptr, out_buf.data(), plain.data(),
                         plain.size());
  if (ret > 0) {
    for (uint8_t b : out_buf) result.push_back(b);
  }
  return result;
}

Vec<uint8_t> AesUnwrap(rust::Slice<const uint8_t> kek,
                       rust::Slice<const uint8_t> cipher) {
  Vec<uint8_t> result;
  if (cipher.size() % 8 != 0 || cipher.size() < 24) return result;

  AES_KEY aes_key;
  if (AES_set_decrypt_key(kek.data(), kek.size() * 8, &aes_key) < 0) {
    return result;
  }

  size_t out_len = cipher.size() - 8;
  std::vector<uint8_t> out_buf(out_len);

  int ret = AES_unwrap_key(&aes_key, nullptr, out_buf.data(), cipher.data(),
                           cipher.size());
  if (ret > 0) {
    for (uint8_t b : out_buf) result.push_back(b);
  }
  return result;
}

// AES-CCM using EVP_AEAD (BoringSSL style)
// We use EVP_aead_aes_128_ccm_bluetooth_8 because standard generic CCM
// might be unavailable/hidden in this build configuration.
// Bluetooth_8 uses M=8 (MIC length) and L=2 (length field bytes),
// which matches WPA2-CCMP requirements (8-byte MIC, 13-byte Nonce).

bool AesCcmEncrypt(rust::Slice<const uint8_t> key,
                   rust::Slice<const uint8_t> nonce,
                   rust::Slice<const uint8_t> aad,
                   rust::Slice<const uint8_t> plain, Vec<uint8_t> &out_data,
                   size_t tag_len) {
  const EVP_AEAD *aead = nullptr;
  if (key.size() == 16) {
    aead = EVP_aead_aes_128_ccm_bluetooth_8();
  } else {
    std::cerr << "AesCcmEncrypt: Unsupported key size " << key.size()
              << std::endl;
    return false;
  }

  EVP_AEAD_CTX ctx;
  if (!EVP_AEAD_CTX_init(&ctx, aead, key.data(), key.size(), tag_len,
                         nullptr)) {
    return false;
  }

  size_t max_out_len = plain.size() + EVP_AEAD_max_overhead(aead);
  std::vector<uint8_t> out_buf(max_out_len);
  size_t out_len = 0;

  if (!EVP_AEAD_CTX_seal(&ctx, out_buf.data(), &out_len, max_out_len,
                         nonce.data(), nonce.size(), plain.data(), plain.size(),
                         aad.data(), aad.size())) {
    EVP_AEAD_CTX_cleanup(&ctx);
    return false;
  }
  EVP_AEAD_CTX_cleanup(&ctx);

  out_data.reserve(out_len);
  for (size_t i = 0; i < out_len; i++) out_data.push_back(out_buf[i]);

  return true;
}

bool AesCcmDecrypt(rust::Slice<const uint8_t> key,
                   rust::Slice<const uint8_t> nonce,
                   rust::Slice<const uint8_t> aad,
                   rust::Slice<const uint8_t> cipher_with_tag,
                   Vec<uint8_t> &out_plain, size_t tag_len) {
  const EVP_AEAD *aead = nullptr;
  if (key.size() == 16) {
    aead = EVP_aead_aes_128_ccm_bluetooth_8();
  } else {
    std::cerr << "AesCcmDecrypt: Unsupported key size " << key.size()
              << std::endl;
    return false;
  }

  EVP_AEAD_CTX ctx;
  if (!EVP_AEAD_CTX_init(&ctx, aead, key.data(), key.size(), tag_len,
                         nullptr)) {
    return false;
  }

  size_t max_out_len = cipher_with_tag.size();
  std::vector<uint8_t> out_buf(max_out_len);
  size_t out_len = 0;

  if (!EVP_AEAD_CTX_open(&ctx, out_buf.data(), &out_len, max_out_len,
                         nonce.data(), nonce.size(), cipher_with_tag.data(),
                         cipher_with_tag.size(), aad.data(), aad.size())) {
    EVP_AEAD_CTX_cleanup(&ctx);
    return false;
  }
  EVP_AEAD_CTX_cleanup(&ctx);

  out_plain.reserve(out_len);
  for (size_t i = 0; i < out_len; i++) out_plain.push_back(out_buf[i]);

  return true;
}

Vec<uint8_t> RandBytes(size_t len) {
  Vec<uint8_t> out;
  out.reserve(len);
  // TODO: Switch back to RAND_bytes when linking is fixed.
  std::random_device rd;
  std::mt19937 gen(rd());
  std::uniform_int_distribution<> dis(0, 255);
  for (size_t i = 0; i < len; ++i) {
    out.push_back(static_cast<uint8_t>(dis(gen)));
  }
  return out;
}

// KDF-z / IEEE 802.11-2012 11.6.1.7.2
// KDF-z / IEEE 802.11-2012 11.6.1.7.2
static bool Sha256PrfBits(const Vec<uint8_t> &key, const char *label,
                          const std::vector<uint8_t> &data,
                          std::vector<uint8_t> &out, size_t bits) {
  uint16_t counter = 1;
  size_t pos = 0;
  size_t buf_len = (bits + 7) / 8;
  out.resize(buf_len);
  size_t label_len = strlen(label);

  while (pos < buf_len) {
    size_t plen = buf_len - pos;

    // LE encoding
    uint8_t count_bytes[2] = {(uint8_t)(counter & 0xff),
                              (uint8_t)(counter >> 8)};
    uint8_t len_bytes[2] = {(uint8_t)(bits & 0xff), (uint8_t)(bits >> 8)};

    unsigned int md_len;
    uint8_t hash[SHA256_DIGEST_LENGTH];

    HMAC_CTX *ctx = HMAC_CTX_new();
    if (!ctx) return false;

    // HMAC-SHA256(K, i || Label || Context || Length)
    if (!HMAC_Init_ex(ctx, key.data(), key.size(), EVP_sha256(), nullptr) ||
        !HMAC_Update(ctx, count_bytes, 2) ||
        !HMAC_Update(ctx, (const uint8_t *)label, label_len) ||
        !HMAC_Update(ctx, data.data(), data.size()) ||
        !HMAC_Update(ctx, len_bytes, 2) || !HMAC_Final(ctx, hash, &md_len)) {
      HMAC_CTX_free(ctx);
      return false;
    }
    HMAC_CTX_free(ctx);

    size_t copy_len =
        (plen >= SHA256_DIGEST_LENGTH) ? SHA256_DIGEST_LENGTH : plen;
    memcpy(out.data() + pos, hash, copy_len);
    pos += copy_len;
    counter++;
  }

  if (bits % 8) {
    out[buf_len - 1] &= (0xff << (8 - bits % 8));
  }
  return true;
}

// PBKDF2-HMAC-SHA1 for WPA2 PMK Derivation
rust::Vec<uint8_t> Pbkdf2HmacSha1(rust::Slice<const uint8_t> password,
                                  rust::Slice<const uint8_t> salt,
                                  uint32_t iterations, size_t key_len) {
  rust::Vec<uint8_t> result;
  std::vector<uint8_t> out_buf(key_len);

  if (PKCS5_PBKDF2_HMAC(reinterpret_cast<const char *>(password.data()),
                        password.size(), salt.data(), salt.size(), iterations,
                        EVP_sha1(), key_len, out_buf.data())) {
    for (uint8_t b : out_buf) result.push_back(b);
  }
  return result;
}

rust::Vec<uint8_t> EcP256CalculatePwe(rust::Slice<const uint8_t> password,
                                      rust::Slice<const uint8_t> address1,
                                      rust::Slice<const uint8_t> address2) {
  Vec<uint8_t> result;
  if (address1.size() != 6 || address2.size() != 6) return result;

  // Derived Key = Max(Addr1, Addr2) || Min(Addr1, Addr2)
  Vec<uint8_t> key;
  key.reserve(12);
  if (memcmp(address1.data(), address2.data(), 6) > 0) {
    for (auto b : address1) key.push_back(b);
    for (auto b : address2) key.push_back(b);
  } else {
    for (auto b : address2) key.push_back(b);
    for (auto b : address1) key.push_back(b);
  }

  EC_GROUP *group = EC_GROUP_new_by_curve_name(NID_X9_62_prime256v1);
  if (!group) return result;

  BIGNUM *prime = BN_new();
  BIGNUM *x = BN_new();
  EC_POINT *pwe_point = EC_POINT_new(group);
  BN_CTX *bn_ctx = BN_CTX_new();

  if (!prime || !x || !pwe_point || !bn_ctx ||
      !EC_GROUP_get_curve_GFp(group, prime, nullptr, nullptr, bn_ctx)) {
    goto cleanup;
  }

  // Hunting and Pecking loop
  // k = 40 (max iterations per hostapd default)
  // We try reasonably hard.
  for (uint8_t counter = 1; counter <= 40; counter++) {
    // data = password || counter
    std::vector<uint8_t> kdf_data;
    kdf_data.assign(password.begin(), password.end());
    kdf_data.push_back(counter);

    // pwd-value = KDF-z(pwd-seed, "SAE Hunting and Pecking", p)
    // Note: hostapd logic is slightly confusing on names.
    // It calls "pwd-seed" the result of HMAC on (addrs, password|counter).
    // Then calls "pwd-value" the result of KDF-z(pwd-seed, label, prime).
    // Wait, NO.
    // hostapd:
    //  pwd_seed = HMAC(addrs, password || counter)
    //  pwd_value = KDF-z(pwd_seed, "SAE Hunting and Pecking", prime)

    // Let's replicate this 2-step process.

    // 1. Calculate main seed: HMAC(key=addrs, data=password||counter)
    Vec<uint8_t> hmac_seed;
    hmac_seed.reserve(SHA256_DIGEST_LENGTH);
    unsigned int md_len;
    uint8_t hash[SHA256_DIGEST_LENGTH];
    if (!HMAC(EVP_sha256(), key.data(), key.size(), kdf_data.data(),
              kdf_data.size(), hash, &md_len))
      continue;
    for (int i = 0; i < SHA256_DIGEST_LENGTH; i++) hmac_seed.push_back(hash[i]);

    // 2. KDF-z using hmac_seed as key, label="SAE Hunting and Pecking",
    // data=prime
    std::vector<uint8_t> prime_bin(BN_num_bytes(prime));
    BN_bn2bin(prime, prime_bin.data());

    std::vector<uint8_t> pwd_value;
    // P-256 prime is 256 bits.
    if (!Sha256PrfBits(hmac_seed, "SAE Hunting and Pecking", prime_bin,
                       pwd_value, 256))
      continue;

    // Check pwd_value < prime
    BIGNUM *pwd_bn = BN_bin2bn(pwd_value.data(), pwd_value.size(), nullptr);
    if (!pwd_bn) continue;

    if (BN_cmp(pwd_bn, prime) >= 0) {
      BN_free(pwd_bn);
      continue;  // Must be < prime
    }

    // Check if x = pwd_value corresponds to a point
    // pwd-seed last bit determines y-bit.
    // pwd-seed here refers to hmac_seed.
    int y_bit = hmac_seed[hmac_seed.size() - 1] & 0x01;

    if (EC_POINT_set_compressed_coordinates_GFp(group, pwe_point, pwd_bn, y_bit,
                                                bn_ctx) == 1) {
      // Found it!
      // Serialize to uncompressed
      size_t len = EC_POINT_point2oct(
          group, pwe_point, POINT_CONVERSION_UNCOMPRESSED, nullptr, 0, bn_ctx);
      std::vector<uint8_t> temp_res(len);
      EC_POINT_point2oct(group, pwe_point, POINT_CONVERSION_UNCOMPRESSED,
                         temp_res.data(), len, bn_ctx);
      for (auto b : temp_res) result.push_back(b);
      BN_free(pwd_bn);
      break;
    }
    BN_free(pwd_bn);
  }

cleanup:
  BN_CTX_free(bn_ctx);
  EC_POINT_free(pwe_point);
  BN_free(x);
  BN_free(prime);
  EC_GROUP_free(group);
  return result;
}

rust::Vec<uint8_t> EcP256PointMul(rust::Slice<const uint8_t> point_src,
                                  rust::Slice<const uint8_t> scalar) {
  Vec<uint8_t> result;
  EC_GROUP *group = EC_GROUP_new_by_curve_name(NID_X9_62_prime256v1);
  EC_POINT *p = EC_POINT_new(group);
  EC_POINT *res = EC_POINT_new(group);
  BIGNUM *k = BN_bin2bn(scalar.data(), scalar.size(), nullptr);
  BN_CTX *ctx = BN_CTX_new();

  if (group && p && res && k && ctx &&
      EC_POINT_oct2point(group, p, point_src.data(), point_src.size(), ctx) &&
      EC_POINT_mul(group, res, nullptr, p, k, ctx)) {
    size_t len = EC_POINT_point2oct(group, res, POINT_CONVERSION_UNCOMPRESSED,
                                    nullptr, 0, ctx);
    if (len > 0) {
      std::vector<uint8_t> temp_res(len);
      EC_POINT_point2oct(group, res, POINT_CONVERSION_UNCOMPRESSED,
                         temp_res.data(), len, ctx);
      for (auto b : temp_res) result.push_back(b);
    }
  }

  BN_CTX_free(ctx);
  BN_free(k);
  EC_POINT_free(res);
  EC_POINT_free(p);
  EC_GROUP_free(group);
  return result;
}

rust::Vec<uint8_t> EcP256PointAdd(rust::Slice<const uint8_t> point_a,
                                  rust::Slice<const uint8_t> point_b) {
  Vec<uint8_t> result;
  EC_GROUP *group = EC_GROUP_new_by_curve_name(NID_X9_62_prime256v1);
  EC_POINT *a = EC_POINT_new(group);
  EC_POINT *b = EC_POINT_new(group);
  EC_POINT *res = EC_POINT_new(group);
  BN_CTX *ctx = BN_CTX_new();

  if (group && a && b && res && ctx &&
      EC_POINT_oct2point(group, a, point_a.data(), point_a.size(), ctx) &&
      EC_POINT_oct2point(group, b, point_b.data(), point_b.size(), ctx) &&
      EC_POINT_add(group, res, a, b, ctx)) {
    size_t len = EC_POINT_point2oct(group, res, POINT_CONVERSION_UNCOMPRESSED,
                                    nullptr, 0, ctx);
    if (len > 0) {
      std::vector<uint8_t> temp_res(len);
      EC_POINT_point2oct(group, res, POINT_CONVERSION_UNCOMPRESSED,
                         temp_res.data(), len, ctx);
      for (auto b : temp_res) result.push_back(b);
    }
  }
  BN_CTX_free(ctx);
  EC_POINT_free(res);
  EC_POINT_free(b);
  EC_POINT_free(a);
  EC_GROUP_free(group);
  return result;
}

rust::Vec<uint8_t> BnModAdd(rust::Slice<const uint8_t> a,
                            rust::Slice<const uint8_t> b,
                            rust::Slice<const uint8_t> m) {
  Vec<uint8_t> result;
  BIGNUM *bn_a = BN_bin2bn(a.data(), a.size(), nullptr);
  BIGNUM *bn_b = BN_bin2bn(b.data(), b.size(), nullptr);
  BIGNUM *bn_m = BN_bin2bn(m.data(), m.size(), nullptr);
  BIGNUM *res = BN_new();
  BN_CTX *ctx = BN_CTX_new();

  if (bn_a && bn_b && bn_m && res && ctx &&
      BN_mod_add(res, bn_a, bn_b, bn_m, ctx)) {
    int len = BN_num_bytes(res);
    if (len >= 0) {
      std::vector<uint8_t> temp_res(len);
      BN_bn2bin(res, temp_res.data());
      for (auto b : temp_res) result.push_back(b);
    }
  }

  BN_CTX_free(ctx);
  BN_free(res);
  BN_free(bn_m);
  BN_free(bn_b);
  BN_free(bn_a);
  return result;
}

rust::Vec<uint8_t> BnModSub(rust::Slice<const uint8_t> a,
                            rust::Slice<const uint8_t> b,
                            rust::Slice<const uint8_t> m) {
  Vec<uint8_t> result;
  BIGNUM *bn_a = BN_bin2bn(a.data(), a.size(), nullptr);
  BIGNUM *bn_b = BN_bin2bn(b.data(), b.size(), nullptr);
  BIGNUM *bn_m = BN_bin2bn(m.data(), m.size(), nullptr);
  BIGNUM *res = BN_new();
  BN_CTX *ctx = BN_CTX_new();

  if (bn_a && bn_b && bn_m && res && ctx &&
      BN_mod_sub(res, bn_a, bn_b, bn_m, ctx)) {
    int len = BN_num_bytes(res);
    if (len >= 0) {
      std::vector<uint8_t> temp_res(len);
      BN_bn2bin(res, temp_res.data());
      for (auto b : temp_res) result.push_back(b);
    }
  }

  BN_CTX_free(ctx);
  BN_free(res);
  BN_free(bn_m);
  BN_free(bn_b);
  BN_free(bn_a);
  return result;
}

}  // namespace hostap
}  // namespace netsim
