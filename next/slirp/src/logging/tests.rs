// Copyright 2023-2025 The Android Open Source Project
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

use super::*;

#[test]
fn test_logging_topics_enabled() {
    // Enable only tcp and udp
    reset_for_testing(Some("tcp,udp"));
    assert!(is_enabled(Topic::Tcp));
    assert!(is_enabled(Topic::Udp));
    assert!(!is_enabled(Topic::Icmp));
    assert!(!is_enabled(Topic::Dhcp));

    // Enable all
    reset_for_testing(Some("all"));
    assert!(is_enabled(Topic::Tcp));
    assert!(is_enabled(Topic::Icmp));
    assert!(is_enabled(Topic::Tftp));

    // Disable all
    reset_for_testing(None);
    assert!(!is_enabled(Topic::Tcp));
    assert!(!is_enabled(Topic::Udp));
}

#[test]
fn test_parse_debug_topics_individual() {
    assert_eq!(Topic::parse_debug_topics("tcp"), Topic::Tcp as u32);
    assert_eq!(Topic::parse_debug_topics("udp"), Topic::Udp as u32);
    assert_eq!(Topic::parse_debug_topics("icmp"), Topic::Icmp as u32);
    assert_eq!(Topic::parse_debug_topics("dhcp"), Topic::Dhcp as u32);
    assert_eq!(Topic::parse_debug_topics("dns"), Topic::Dns as u32);
    assert_eq!(Topic::parse_debug_topics("tftp"), Topic::Tftp as u32);
    assert_eq!(Topic::parse_debug_topics("arp"), Topic::Arp as u32);
}

#[test]
fn test_parse_debug_topics_all() {
    assert_eq!(Topic::parse_debug_topics("all"), u32::MAX);
    assert_eq!(Topic::parse_debug_topics("ALL"), u32::MAX);
}

#[test]
fn test_parse_debug_topics_whitespace_and_case() {
    let flags = Topic::parse_debug_topics("  TCP ,  uDp  , ArP  ");
    assert_eq!(flags, (Topic::Tcp as u32) | (Topic::Udp as u32) | (Topic::Arp as u32));
}

#[test]
fn test_parse_debug_topics_unknown_and_empty() {
    assert_eq!(Topic::parse_debug_topics(""), 0);
    assert_eq!(Topic::parse_debug_topics("unknown,foo,bar"), 0);
    assert_eq!(Topic::parse_debug_topics("unknown,tcp,invalid"), Topic::Tcp as u32);
}

#[test]
fn test_topic_from_name() {
    assert_eq!(Topic::from_name("tcp"), Some(Topic::Tcp));
    assert_eq!(Topic::from_name("  uDp  "), Some(Topic::Udp));
    assert_eq!(Topic::from_name("ICMP"), Some(Topic::Icmp));
    assert_eq!(Topic::from_name("dhcp"), Some(Topic::Dhcp));
    assert_eq!(Topic::from_name("dns"), Some(Topic::Dns));
    assert_eq!(Topic::from_name("tftp"), Some(Topic::Tftp));
    assert_eq!(Topic::from_name("arp"), Some(Topic::Arp));
    assert_eq!(Topic::from_name("all"), None);
    assert_eq!(Topic::from_name("unknown"), None);
    assert_eq!(Topic::from_name(""), None);
}
