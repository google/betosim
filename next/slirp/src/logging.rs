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

//! Configurable debug logging based on SLIRP_DEBUG env var.

use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Topic {
    Tcp = 1 << 0,
    Udp = 1 << 1,
    Icmp = 1 << 2,
    Dhcp = 1 << 3,
    Dns = 1 << 4,
    Tftp = 1 << 5,
    Arp = 1 << 6,
}

static ENABLED_TOPICS: AtomicU32 = AtomicU32::new(0);
static INITIALIZED: std::sync::Once = std::sync::Once::new();

impl Topic {
    /// Parses a single topic name (case-insensitive).
    fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "tcp" => Some(Self::Tcp),
            "udp" => Some(Self::Udp),
            "icmp" => Some(Self::Icmp),
            "dhcp" => Some(Self::Dhcp),
            "dns" => Some(Self::Dns),
            "tftp" => Some(Self::Tftp),
            "arp" => Some(Self::Arp),
            _ => None,
        }
    }

    /// Parses a comma-separated list of debug topics into a bitmask of enabled topics.
    fn parse_debug_topics(val: &str) -> u32 {
        let mut flags = 0;
        for part in val.split(',') {
            let trimmed = part.trim();
            if trimmed.eq_ignore_ascii_case("all") {
                flags = u32::MAX;
            } else if let Some(topic) = Self::from_name(trimmed) {
                flags |= topic as u32;
            }
        }
        flags
    }
}

/// Initializes the logging system by reading the SLIRP_DEBUG environment
/// variable. This is called automatically on the first log check.
pub fn init() {
    INITIALIZED.call_once(|| {
        let flags =
            std::env::var("SLIRP_DEBUG").map(|val| Topic::parse_debug_topics(&val)).unwrap_or(0);
        ENABLED_TOPICS.store(flags, Ordering::Relaxed);
    });
}

/// Checks if a specific debug topic is enabled.
pub fn is_enabled(topic: Topic) -> bool {
    init();
    (ENABLED_TOPICS.load(Ordering::Relaxed) & (topic as u32)) != 0
}

#[macro_export]
macro_rules! slirp_debug {
    ($topic:expr, $($arg:tt)+) => {
        if $crate::logging::is_enabled($topic) {
            log::debug!(target: "libslirp", $($arg)+);
        }
    };
}

#[cfg(test)]
pub fn reset_for_testing(val: Option<&str>) {
    init(); // Ensure Once is marked as run!
    let flags = val.map(Topic::parse_debug_topics).unwrap_or(0);
    ENABLED_TOPICS.store(flags, Ordering::Relaxed);
}

#[cfg(test)]
#[path = "logging/tests.rs"]
mod tests;
