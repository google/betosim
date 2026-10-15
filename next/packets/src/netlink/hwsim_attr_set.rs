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

//! Hwsim attributes parsing and building.

use std::fmt;

use zerocopy::{Immutable, IntoBytes};

use crate::{
    ethernet::MacAddr,
    netlink::{HwsimAttrEnum, NlAttrHdr, TxRate, TxRateFlag, mac80211_hwsim},
};

/// Error type for Hwsim attribute parsing.
#[derive(Debug)]
pub enum HwsimError {
    /// Failed to parse a frame or attribute.
    Frame(String),
    /// Failed to parse using zerocopy or other libraries.
    ParseError(&'static str, Box<dyn std::error::Error + Send + Sync>),
    /// Other errors.
    Other(String),
}

impl fmt::Display for HwsimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HwsimError::Frame(msg) => write!(f, "Frame error: {}", msg),
            HwsimError::ParseError(msg, err) => write!(f, "Parse error ({}): {}", msg, err),
            HwsimError::Other(msg) => write!(f, "Other error: {}", msg),
        }
    }
}

impl std::error::Error for HwsimError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            HwsimError::ParseError(_, err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

/// Result type for Hwsim operations.
pub type HwsimResult<T> = Result<T, HwsimError>;

/// Aligns a length to the specified alignment boundary (`NLA_ALIGNTO`).
fn nla_align(array_length: usize) -> usize {
    const NLA_ALIGNTO: usize = 4;
    array_length.wrapping_add(NLA_ALIGNTO - 1) & !(NLA_ALIGNTO - 1)
}

/// Builder for `HwsimAttrSet`.
#[derive(Default)]
pub struct HwsimAttrSetBuilder {
    transmitter: Option<MacAddr>,
    receiver: Option<MacAddr>,
    frame: Option<Vec<u8>>,
    flags: Option<u32>,
    rx_rate_idx: Option<u32>,
    signal: Option<u32>,
    cookie: Option<u64>,
    freq: Option<u32>,
    tx_info: Option<Vec<TxRate>>,
    tx_info_flags: Option<Vec<TxRateFlag>>,
    attributes: Vec<u8>,
}

/// Set of parsed Hwsim attributes.
#[derive(Debug, Clone)]
pub struct HwsimAttrSet {
    /// Transmitter MAC address.
    pub transmitter: Option<MacAddr>,
    /// Receiver MAC address.
    pub receiver: Option<MacAddr>,
    /// Raw frame data.
    pub frame: Option<Vec<u8>>,
    /// Flags.
    pub flags: Option<u32>,
    /// Receive rate index.
    pub rx_rate_idx: Option<u32>,
    /// Signal strength.
    pub signal: Option<u32>,
    /// Cookie.
    pub cookie: Option<u64>,
    /// Frequency.
    pub freq: Option<u32>,
    /// Transmit info (rates).
    pub tx_info: Option<Vec<TxRate>>,
    /// Transmit info flags.
    pub tx_info_flags: Option<Vec<TxRateFlag>>,
    /// Raw attributes bytes.
    pub attributes: Vec<u8>,
}

impl HwsimAttrSetBuilder {
    fn extend_attributes(&mut self, bytes: &[u8]) {
        let mut vec = bytes.to_vec();
        let nla_padding = nla_align(vec.len()) - vec.len();
        vec.extend(vec![0; nla_padding]);
        self.attributes.extend(vec);
    }

    /// Sets the transmitter address.
    pub fn transmitter(&mut self, transmitter: &[u8; 6]) -> &mut Self {
        let attr = mac80211_hwsim::HwsimAttrAddrTransmitter {
            header: NlAttrHdr::new(10, HwsimAttrEnum::AddrTransmitter as u16),
            address: *transmitter,
        };
        self.extend_attributes(attr.as_bytes());
        self.transmitter = Some(MacAddr::from(*transmitter));
        self
    }

    /// Sets the receiver address.
    pub fn receiver(&mut self, receiver: &[u8; 6]) -> &mut Self {
        let attr = mac80211_hwsim::HwsimAttrAddrReceiver {
            header: NlAttrHdr::new(10, HwsimAttrEnum::AddrReceiver as u16),
            address: *receiver,
        };
        self.extend_attributes(attr.as_bytes());
        self.receiver = Some(MacAddr::from(*receiver));
        self
    }

    /// Sets the frame data.
    pub fn frame(&mut self, frame: &[u8]) -> &mut Self {
        let len = (4 + frame.len()) as u16;
        let header = NlAttrHdr::new(len, HwsimAttrEnum::Frame as u16);
        self.attributes.extend_from_slice(header.as_bytes());
        self.attributes.extend_from_slice(frame);
        let nla_padding = nla_align(len as usize) - len as usize;
        self.attributes.extend(vec![0; nla_padding]);
        self.frame = Some(frame.to_vec());
        self
    }

    fn extend_attr<T: IntoBytes + Immutable>(&mut self, attr: T) {
        self.extend_attributes(attr.as_bytes());
    }

    /// Sets the flags.
    pub fn flags(&mut self, flags: u32) -> &mut Self {
        self.extend_attr(mac80211_hwsim::HwsimAttrFlags {
            header: NlAttrHdr::new(8, HwsimAttrEnum::Flags as u16),
            flags,
        });
        self.flags = Some(flags);
        self
    }

    /// Sets the RX rate index.
    pub fn rx_rate(&mut self, rx_rate_idx: u32) -> &mut Self {
        self.extend_attr(mac80211_hwsim::HwsimAttrRxRate {
            header: NlAttrHdr::new(8, HwsimAttrEnum::RxRate as u16),
            rx_rate_idx,
        });
        self.rx_rate_idx = Some(rx_rate_idx);
        self
    }

    /// Sets the signal strength.
    pub fn signal(&mut self, signal: u32) -> &mut Self {
        self.extend_attr(mac80211_hwsim::HwsimAttrSignal {
            header: NlAttrHdr::new(8, HwsimAttrEnum::Signal as u16),
            signal,
        });
        self.signal = Some(signal);
        self
    }

    /// Sets the cookie.
    pub fn cookie(&mut self, cookie: u64) -> &mut Self {
        self.extend_attr(mac80211_hwsim::HwsimAttrCookie {
            header: NlAttrHdr::new(12, HwsimAttrEnum::Cookie as u16),
            cookie,
        });
        self.cookie = Some(cookie);
        self
    }

    /// Sets the frequency.
    pub fn freq(&mut self, freq: u32) -> &mut Self {
        self.extend_attr(mac80211_hwsim::HwsimAttrFreq {
            header: NlAttrHdr::new(8, HwsimAttrEnum::Freq as u16),
            freq,
        });
        self.freq = Some(freq);
        self
    }

    /// Sets the TX info.
    pub fn tx_info(&mut self, tx_info: &[TxRate]) -> &mut Self {
        let mut bytes = Vec::new();
        for rate in tx_info {
            bytes.extend_from_slice(rate.as_bytes());
        }
        let len = (4 + bytes.len()) as u16;
        let header = NlAttrHdr::new(len, HwsimAttrEnum::TxInfo as u16);
        self.attributes.extend_from_slice(header.as_bytes());
        self.attributes.extend_from_slice(&bytes);
        let nla_padding = nla_align(len as usize) - len as usize;
        self.attributes.extend(vec![0; nla_padding]);
        self.tx_info = Some(tx_info.to_vec());
        self
    }

    /// Sets the TX info flags.
    pub fn tx_info_flags(&mut self, tx_rate_flags: &[TxRateFlag]) -> &mut Self {
        let mut bytes = Vec::new();
        for flag in tx_rate_flags {
            bytes.extend_from_slice(flag.as_bytes());
        }
        let len = (4 + bytes.len()) as u16;
        let header = NlAttrHdr::new(len, HwsimAttrEnum::TxInfoFlags as u16);
        self.attributes.extend_from_slice(header.as_bytes());
        self.attributes.extend_from_slice(&bytes);
        let nla_padding = nla_align(len as usize) - len as usize;
        self.attributes.extend(vec![0; nla_padding]);
        self.tx_info_flags = Some(tx_rate_flags.to_vec());
        self
    }

    /// Builds the `HwsimAttrSet`.
    pub fn build(self) -> HwsimResult<HwsimAttrSet> {
        Ok(HwsimAttrSet {
            transmitter: self.transmitter,
            receiver: self.receiver,
            cookie: self.cookie,
            flags: self.flags,
            rx_rate_idx: self.rx_rate_idx,
            signal: self.signal,
            frame: self.frame,
            freq: self.freq,
            tx_info: self.tx_info,
            tx_info_flags: self.tx_info_flags,
            attributes: self.attributes,
        })
    }
}

impl fmt::Display for HwsimAttrSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{ ")?;
        if let Some(v) = self.transmitter {
            write!(f, "transmitter: {v}, ")?
        }
        if let Some(v) = self.receiver {
            write!(f, "receiver: {v}, ")?
        }
        if let Some(v) = self.cookie {
            write!(f, "cookie: {v}, ")?
        }
        if let Some(v) = self.flags {
            write!(f, "flags: {v}, ")?
        }
        if let Some(v) = self.rx_rate_idx {
            write!(f, "rx_rate_idx: {v}, ")?
        }
        if let Some(v) = self.signal {
            write!(f, "signal: {v}, ")?
        }
        if let Some(v) = &self.frame {
            write!(f, "frame: {:?}, ", v)?
        }
        if let Some(v) = self.freq {
            write!(f, "freq: {v}, ")?
        }
        if let Some(v) = &self.tx_info {
            write!(f, "tx_info: {:?}, ", v)?
        }
        if let Some(v) = &self.tx_info_flags {
            write!(f, "tx_info_flags: {:?}, ", v)?
        }
        write!(f, "}}")
    }
}

impl HwsimAttrSet {
    /// Creates a new `HwsimAttrSetBuilder`.
    pub fn builder() -> HwsimAttrSetBuilder {
        HwsimAttrSetBuilder::default()
    }

    /// Parse and validates the attributes from a HwsimMsg command.
    pub fn parse(attributes: &[u8]) -> HwsimResult<HwsimAttrSet> {
        Self::parse_with_frame_transmitter(attributes, Option::None, Option::None)
    }

    /// Parse and validates the attributes from a HwsimMsg command.
    /// Update frame and transmitter if provided.
    pub fn parse_with_frame_transmitter(
        attributes: &[u8],
        frame: Option<&[u8]>,
        transmitter: Option<&[u8; 6]>,
    ) -> HwsimResult<HwsimAttrSet> {
        let mut index: usize = 0;
        let mut builder = HwsimAttrSet::builder();
        while index < attributes.len() {
            let hdr_bytes = attributes
                .get(index..index + 4)
                .ok_or_else(|| HwsimError::Frame("Buffer too short for NlAttrHdr".into()))?;
            let nla_hdr = NlAttrHdr::decode_full(hdr_bytes).map_err(|err| {
                HwsimError::ParseError("Failed to decode NlAttrHdr", Box::from(err))
            })?;
            let nla_len = nla_hdr.nla_len.get() as usize;
            let attr_bytes = attributes
                .get(index..index + nla_len)
                .ok_or_else(|| HwsimError::Frame("Attribute length exceeds buffer".into()))?;
            let Some((_, attr_payload)) = attr_bytes.split_first_chunk::<4>() else {
                return Err(HwsimError::Frame("Invalid NlAttrHdr length".to_string()));
            };
            let attr_type = num_traits::FromPrimitive::from_u16(nla_hdr.type_());

            match attr_type {
                Some(HwsimAttrEnum::AddrTransmitter) => {
                    let attr = zerocopy::Ref::<&[u8], mac80211_hwsim::HwsimAttrAddrTransmitter>::from_prefix(attr_bytes).map_err(|err| {
                        HwsimError::ParseError("Failed to read AddrTransmitter", Box::from(err.to_string()))
                    })?.0;
                    builder.transmitter(&attr.address);
                }
                Some(HwsimAttrEnum::AddrReceiver) => {
                    let attr =
                        zerocopy::Ref::<&[u8], mac80211_hwsim::HwsimAttrAddrReceiver>::from_prefix(
                            attr_bytes,
                        )
                        .map_err(|err| {
                            HwsimError::ParseError(
                                "Failed to read AddrReceiver",
                                Box::from(err.to_string()),
                            )
                        })?
                        .0;
                    builder.receiver(&attr.address);
                }
                Some(HwsimAttrEnum::Frame) => {
                    if !attr_payload.is_empty() {
                        builder.frame(attr_payload);
                    }
                }
                Some(HwsimAttrEnum::Flags) => {
                    let attr = zerocopy::Ref::<&[u8], mac80211_hwsim::HwsimAttrFlags>::from_prefix(
                        attr_bytes,
                    )
                    .map_err(|err| {
                        HwsimError::ParseError("Failed to read Flags", Box::from(err.to_string()))
                    })?
                    .0;
                    builder.flags(attr.flags);
                }
                Some(HwsimAttrEnum::RxRate) => {
                    let attr =
                        zerocopy::Ref::<&[u8], mac80211_hwsim::HwsimAttrRxRate>::from_prefix(
                            attr_bytes,
                        )
                        .map_err(|err| {
                            HwsimError::ParseError(
                                "Failed to read RxRate",
                                Box::from(err.to_string()),
                            )
                        })?
                        .0;
                    builder.rx_rate(attr.rx_rate_idx);
                }
                Some(HwsimAttrEnum::Signal) => {
                    let attr =
                        zerocopy::Ref::<&[u8], mac80211_hwsim::HwsimAttrSignal>::from_prefix(
                            attr_bytes,
                        )
                        .map_err(|err| {
                            HwsimError::ParseError(
                                "Failed to read Signal",
                                Box::from(err.to_string()),
                            )
                        })?
                        .0;
                    builder.signal(attr.signal);
                }
                Some(HwsimAttrEnum::Cookie) => {
                    let attr =
                        zerocopy::Ref::<&[u8], mac80211_hwsim::HwsimAttrCookie>::from_prefix(
                            attr_bytes,
                        )
                        .map_err(|err| {
                            HwsimError::ParseError(
                                "Failed to read Cookie",
                                Box::from(err.to_string()),
                            )
                        })?
                        .0;
                    builder.cookie(attr.cookie);
                }
                Some(HwsimAttrEnum::Freq) => {
                    let attr = zerocopy::Ref::<&[u8], mac80211_hwsim::HwsimAttrFreq>::from_prefix(
                        attr_bytes,
                    )
                    .map_err(|err| {
                        HwsimError::ParseError("Failed to read Freq", Box::from(err.to_string()))
                    })?
                    .0;
                    builder.freq(attr.freq);
                }
                Some(HwsimAttrEnum::TxInfo) => {
                    if let Ok(rates) = zerocopy::Ref::<&[u8], [TxRate]>::from_bytes(attr_payload) {
                        builder.tx_info(&rates);
                    }
                }
                Some(HwsimAttrEnum::TxInfoFlags) => {
                    if let Ok(flags) =
                        zerocopy::Ref::<&[u8], [TxRateFlag]>::from_bytes(attr_payload)
                    {
                        builder.tx_info_flags(&flags);
                    }
                }
                _ => {}
            }
            index += nla_align(nla_len);
        }

        // Overrides
        if let Some(f) = frame {
            builder.frame(f);
        }
        if let Some(t) = transmitter {
            builder.transmitter(t);
        }

        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hwsim_attr_set_parse_tx_info_flags() {
        // Construct a manual HwsimAttrSet byte array simulating what the kernel sends
        let mut bytes = Vec::new();

        // NLA Header for TxInfoFlags: Length = 4 (header) + 3 (1 TxRateFlag) = 7
        // Type = HwsimAttrEnum::TxInfoFlags (17)
        let nla_len: u16 = 7;
        let nla_type: u16 = HwsimAttrEnum::TxInfoFlags as u16;

        bytes.extend_from_slice(&nla_len.to_le_bytes()); // Length
        bytes.extend_from_slice(&nla_type.to_le_bytes()); // Type

        // 1 TxRateFlag (3 bytes: idx=2, mcs=1, flags=0x04)
        bytes.extend_from_slice(&[2, 1, 0x04]);

        // NLA padding (must align to 4 bytes: 7 ends at 8 -> 1 byte of padding)
        bytes.push(0);

        // Parse the attributes back out
        let attr_set = HwsimAttrSet::parse(&bytes).expect("Failed to parse HwsimAttrSet bytes");

        // Assert the tx_info_flags array was captured correctly and not dropped
        let flags =
            attr_set.tx_info_flags.expect("tx_info_flags was missing from parsed HwsimAttrSet");
        assert_eq!(flags.len(), 1);

        let flag = &flags[0];
        let idx = flag.idx;
        let p_flags = flag.flags;
        assert_eq!(idx, 2);
        assert_eq!(p_flags, 0x0401);
    }

    #[test]
    fn test_hwsim_attr_set_builder_scalar_attributes() {
        let mut builder = HwsimAttrSet::builder();
        builder.flags(0x1234_5678).rx_rate(42).signal(99).cookie(0xfeed_face_cafe_beef).freq(5180);
        let attr_set = builder.build().expect("Failed to build HwsimAttrSet");

        assert_eq!(attr_set.flags, Some(0x1234_5678));
        assert_eq!(attr_set.rx_rate_idx, Some(42));
        assert_eq!(attr_set.signal, Some(99));
        assert_eq!(attr_set.cookie, Some(0xfeed_face_cafe_beef));
        assert_eq!(attr_set.freq, Some(5180));

        // Verify that attributes can be parsed back and roundtrip correctly
        let parsed =
            HwsimAttrSet::parse(&attr_set.attributes).expect("Failed to parse built attributes");
        assert_eq!(parsed.flags, Some(0x1234_5678));
        assert_eq!(parsed.rx_rate_idx, Some(42));
        assert_eq!(parsed.signal, Some(99));
        assert_eq!(parsed.cookie, Some(0xfeed_face_cafe_beef));
        assert_eq!(parsed.freq, Some(5180));
    }
}
