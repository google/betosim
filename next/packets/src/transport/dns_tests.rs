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

use zerocopy::FromBytes;

use crate::transport::{DnsFlags, DnsHeader, DnsPacketBuilder, ResourceClass, ResourceType};

#[test]
fn test_dns_builder_query_encoding_and_normalization() {
    // Non-zero buffer verifies that DnsPacketBuilder::new zero-initializes the 12-byte header.
    let mut buffer = [0xFFu8; 128];
    let mut builder = DnsPacketBuilder::new(&mut buffer).expect("buffer valid");
    builder.transaction_id(0xABCD);
    builder.flags(DnsFlags::RECURSION_DESIRED);

    // 1. Standard multi-label domain (20 bytes: 16-byte QNAME + 4-byte QTYPE/QCLASS)
    assert!(
        builder
            .add_question("www.google.com", ResourceType::A as u16, ResourceClass::Internet as u16)
            .is_some()
    );
    // 2. Trailing-dot FQDN normalization (17 bytes: 13-byte QNAME + 4-byte QTYPE/QCLASS)
    assert!(
        builder
            .add_question("android.com.", ResourceType::Aaaa as u16, ResourceClass::Internet as u16)
            .is_some()
    );
    // 3. Root domain "." (5 bytes: 1-byte null root label + 4-byte QTYPE/QCLASS)
    assert!(
        builder.add_question(".", ResourceType::A as u16, ResourceClass::Internet as u16).is_some()
    );

    let total_len = builder.build();
    assert_eq!(total_len, 12 + 20 + 17 + 5);

    let header = DnsHeader::read_from_bytes(&buffer[..12]).unwrap();
    assert_eq!(header.transaction_id.get(), 0xABCD);
    assert_eq!(header.flags.get(), DnsFlags::RECURSION_DESIRED);
    assert_eq!(header.num_questions.get(), 3);
    assert_eq!(header.num_answers.get(), 0);
    assert_eq!(header.num_authority.get(), 0);
    assert_eq!(header.num_additional.get(), 0);

    // Verify wire-format QNAME + QTYPE + QCLASS for all 3 questions
    assert_eq!(&buffer[12..32], b"\x03www\x06google\x03com\x00\x00\x01\x00\x01");
    assert_eq!(&buffer[32..49], b"\x07android\x03com\x00\x00\x1c\x00\x01");
    assert_eq!(&buffer[49..54], b"\x00\x00\x01\x00\x01");
}

#[test]
fn test_dns_builder_rejects_invalid_labels_and_rolls_back() {
    let mut buffer = [0u8; 256];
    let mut builder = DnsPacketBuilder::new(&mut buffer).expect("buffer valid");

    // Seed with one valid question ("ok.com": 8-byte QNAME + 4-byte QTYPE/QCLASS = 12 bytes)
    assert!(builder.add_question("ok.com", 1, 1).is_some());

    // Intermediate empty label ("foo..bar") fails after writing "foo" and must roll back offset
    assert!(builder.add_question("foo..bar", 1, 1).is_none());
    // Leading empty label (".example.com") fails immediately
    assert!(builder.add_question(".example.com", 1, 1).is_none());

    // Label exceeding RFC 1035 63-octet limit (64 bytes) is rejected and rolled back
    let overlong_domain = format!("{}.com", "a".repeat(64));
    assert!(builder.add_question(&overlong_domain, 1, 1).is_none());

    // Label at the exact 63-octet limit succeeds (63 + 1 + 3 + 1 + 1 + 4 = 73 bytes)
    let max_label_domain = format!("{}.com", "a".repeat(63));
    assert!(builder.add_question(&max_label_domain, 1, 1).is_some());

    let total_len = builder.build();
    assert_eq!(total_len, 12 + 12 + 73);
    let header = DnsHeader::read_from_bytes(&buffer[..12]).unwrap();
    assert_eq!(header.num_questions.get(), 2);
}

#[test]
fn test_dns_builder_buffer_bounds_and_rollback() {
    // Buffer smaller than DnsHeader (12 bytes) is rejected
    let mut too_small = [0u8; 11];
    assert!(DnsPacketBuilder::new(&mut too_small).is_none());

    // 12-byte header + 10-byte payload = 22 bytes total:
    // - "example.com" QNAME (13 bytes) overflows inside write_name
    // - "ok.com" QNAME (8 bytes) fits in write_name, but overflows the 4-byte QTYPE/QCLASS write
    let mut buffer = [0u8; 22];
    let mut builder = DnsPacketBuilder::new(&mut buffer).expect("buffer valid");
    assert!(builder.add_question("example.com", 1, 1).is_none());
    assert!(builder.add_question("ok.com", 1, 1).is_none());

    assert_eq!(builder.build(), 12);
    let header = DnsHeader::read_from_bytes(&buffer[..12]).unwrap();
    assert_eq!(header.num_questions.get(), 0);
}
