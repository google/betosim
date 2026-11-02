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

use std::fmt;

use crate::types::CtecTechnology;

const DEFAULT_CELL_BANDWIDTH_KHZ: u32 = 5_000;
const NR_CELL_BANDWIDTH_KHZ: u32 = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum CellConnectionStatus {
    None = 0,
    PrimaryServing = 1,
    SecondaryServing = 2,
}

impl CellConnectionStatus {
    pub(super) const SERVING: [Self; 2] = [Self::PrimaryServing, Self::SecondaryServing];
}

impl From<CellConnectionStatus> for u8 {
    fn from(status: CellConnectionStatus) -> Self {
        status as u8
    }
}

impl TryFrom<u8> for CellConnectionStatus {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::PrimaryServing),
            2 => Ok(Self::SecondaryServing),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum FrequencyRange {
    Unknown = 0,
    Low = 1,
    Mid = 2,
    High = 3,
    Mmwave = 4,
}

impl From<FrequencyRange> for u8 {
    fn from(freq: FrequencyRange) -> Self {
        freq as u8
    }
}

impl TryFrom<u8> for FrequencyRange {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Unknown),
            1 => Ok(Self::Low),
            2 => Ok(Self::Mid),
            3 => Ok(Self::High),
            4 => Ok(Self::Mmwave),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalChannelConfig {
    status: CellConnectionStatus,
    bandwidth_khz: u32,
    tech: CtecTechnology,
    freq: FrequencyRange,
    cid: u8,
}

impl PhysicalChannelConfig {
    #[must_use]
    pub(super) const fn for_serving_cell(
        status: CellConnectionStatus,
        tech: CtecTechnology,
        cid: u8,
    ) -> Self {
        let (bandwidth_khz, freq) = match tech {
            CtecTechnology::Nr => (NR_CELL_BANDWIDTH_KHZ, FrequencyRange::Mmwave),
            CtecTechnology::Gsm
            | CtecTechnology::Wcdma
            | CtecTechnology::Cdma
            | CtecTechnology::Evdo
            | CtecTechnology::Tdscdma
            | CtecTechnology::Lte => (DEFAULT_CELL_BANDWIDTH_KHZ, FrequencyRange::Unknown),
        };
        Self { status, bandwidth_khz, tech, freq, cid }
    }
}

impl fmt::Display for PhysicalChannelConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Goldfish RIL (`AtResponse.cpp:1760`) casts `<mtech>` directly to 0-based
        // `ModemTechnology` (`0..=6`); Cuttlefish's bitmask reads out of bounds as EDGE.
        write!(
            f,
            "%CGFPCCFG: {},{},{},{},{}\r\n",
            u8::from(self.status),
            self.bandwidth_khz,
            self.tech.modem_tech_index(),
            u8::from(self.freq),
            self.cid
        )
    }
}

/// Monotonic token identifying a scheduled `%CGFPCCFG` timer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysicalChannelToken(u64);

impl PhysicalChannelToken {
    fn next(&mut self) -> Self {
        self.0 = self.0.wrapping_add(1);
        *self
    }
}

/// State for the coalesced post-`AT+CGCONTRDP` `%CGFPCCFG` timer.
#[derive(Debug, Default)]
pub(super) struct PhysicalChannelTimer {
    pending: Option<PhysicalChannelToken>,
    next: PhysicalChannelToken,
}

impl PhysicalChannelTimer {
    #[must_use]
    pub(super) const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    #[must_use]
    pub(super) fn schedule(&mut self) -> Option<PhysicalChannelToken> {
        if self.is_pending() {
            return None;
        }
        let token = self.next.next();
        self.pending = Some(token);
        Some(token)
    }

    pub(super) fn cancel(&mut self) {
        self.pending = None;
    }

    #[must_use]
    pub(super) fn take_if_matching(&mut self, token: PhysicalChannelToken) -> bool {
        if self.pending == Some(token) {
            self.pending = None;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_connection_status_round_trip() {
        let cases = [
            (0, CellConnectionStatus::None),
            (1, CellConnectionStatus::PrimaryServing),
            (2, CellConnectionStatus::SecondaryServing),
        ];
        for (wire, status) in cases {
            assert_eq!(u8::from(status), wire);
            assert_eq!(CellConnectionStatus::try_from(wire), Ok(status));
        }
        assert_eq!(CellConnectionStatus::try_from(3), Err(()));
    }

    #[test]
    fn test_cell_connection_status_serving_constants() {
        assert_eq!(
            CellConnectionStatus::SERVING,
            [CellConnectionStatus::PrimaryServing, CellConnectionStatus::SecondaryServing]
        );
    }

    #[test]
    fn test_frequency_range_round_trip() {
        let cases = [
            (0, FrequencyRange::Unknown),
            (1, FrequencyRange::Low),
            (2, FrequencyRange::Mid),
            (3, FrequencyRange::High),
            (4, FrequencyRange::Mmwave),
        ];
        for (wire, freq) in cases {
            assert_eq!(u8::from(freq), wire);
            assert_eq!(FrequencyRange::try_from(wire), Ok(freq));
        }
        assert_eq!(FrequencyRange::try_from(5), Err(()));
    }

    #[test]
    fn test_for_serving_cell_wire_output() {
        let primary_lte = PhysicalChannelConfig::for_serving_cell(
            CellConnectionStatus::PrimaryServing,
            CtecTechnology::Lte,
            1,
        );
        assert_eq!(primary_lte.to_string(), "%CGFPCCFG: 1,5000,5,0,1\r\n");

        let secondary_nr = PhysicalChannelConfig::for_serving_cell(
            CellConnectionStatus::SecondaryServing,
            CtecTechnology::Nr,
            2,
        );
        assert_eq!(secondary_nr.to_string(), "%CGFPCCFG: 2,50000,6,4,2\r\n");
    }

    #[test]
    fn test_physical_channel_timer_lifecycle() {
        let mut timer = PhysicalChannelTimer::default();
        assert!(!timer.is_pending());

        let token1 = timer.schedule().expect("first schedule should return token");
        assert!(timer.is_pending());
        assert_eq!(timer.schedule(), None);

        let wrong_token = PhysicalChannelToken::default();
        assert!(!timer.take_if_matching(wrong_token));
        assert!(timer.is_pending());

        assert!(timer.take_if_matching(token1));
        assert!(!timer.is_pending());

        let token2 = timer.schedule().expect("schedule after take should return token");
        assert_ne!(token1, token2);
        assert!(timer.is_pending());
        timer.cancel();
        assert!(!timer.is_pending());
    }
}
