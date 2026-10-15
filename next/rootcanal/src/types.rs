// Copyright 2025 The Android Open Source Project
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

//! This module defines common Bluetooth data types, such as addresses.

use std::{ffi::c_int, str::FromStr};

use crate::error::{Error, Result};

/// A Bluetooth address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Address {
    /// The 6-byte address.
    pub address: [u8; 6],
}

impl FromStr for Address {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let parts: Vec<_> = s.split(':').collect();
        if parts.len() != 6 {
            return Err(Error::MalformedAddress(s.to_string()));
        }
        let mut address = [0; 6];
        for (dest, part) in address.iter_mut().zip(&parts) {
            *dest = u8::from_str_radix(part, 16)?;
        }
        Ok(Self { address })
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.address[0],
            self.address[1],
            self.address[2],
            self.address[3],
            self.address[4],
            self.address[5]
        )
    }
}

impl Address {
    /// Returns the address as a byte slice.
    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.address
    }

    /// Returns `true` if the address is resolvable.
    pub fn is_resolvable(&self) -> bool {
        (self.address[0] & 0xc0) == 0x40
    }

    /// Returns `true` if the address is non-resolvable.
    pub fn is_non_resolvable(&self) -> bool {
        (self.address[0] & 0xc0) == 0x00
    }

    /// Returns `true` if the address is a static identity address.
    pub fn is_static_identity(&self) -> bool {
        (self.address[0] & 0xc0) == 0xc0
    }
}

/// The physical layer.
#[repr(C)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Phy {
    /// The Low Energy physical layer.
    LowEnergy = 0,
    /// The BR/EDR physical layer.
    BrEdr = 1,
}

impl TryFrom<c_int> for Phy {
    type Error = c_int;
    fn try_from(item: c_int) -> std::result::Result<Self, Self::Error> {
        match item {
            0 => Ok(Phy::LowEnergy),
            1 => Ok(Phy::BrEdr),
            unknown => Err(unknown),
        }
    }
}
