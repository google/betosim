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

//! # Device Actor Utilities
//!
//! This module provides utility functions for the device actor,
//! including network kind conversion and capture stream wrapping.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

use bytes::Bytes;
use capture_api::{
    CaptureCreate, CaptureSender,
    io::{CapturedSink, CapturedStream},
};
use futures::{SinkExt, StreamExt};
use netsim_model::{ChipId, ChipKind, PacketSink, PacketStream};
use netsim_proto::protobuf::Enum;
use tracing::warn;

#[derive(Debug)]
pub(crate) struct StreamStats {
    /// Total packets transmitted by the chip (RX from transport perspective)
    tx_packets: AtomicU64,
    /// Total packets received by the chip (TX from transport perspective)
    rx_packets: AtomicU64,
    /// Total bytes transmitted by the chip
    tx_bytes: AtomicU64,
    /// Total bytes received by the chip
    rx_bytes: AtomicU64,
    pub(crate) start_time: Instant,
}

impl PartialEq for StreamStats {
    fn eq(&self, other: &Self) -> bool {
        self.tx_packets.load(Ordering::Relaxed) == other.tx_packets.load(Ordering::Relaxed)
            && self.rx_packets.load(Ordering::Relaxed) == other.rx_packets.load(Ordering::Relaxed)
            && self.tx_bytes.load(Ordering::Relaxed) == other.tx_bytes.load(Ordering::Relaxed)
            && self.rx_bytes.load(Ordering::Relaxed) == other.rx_bytes.load(Ordering::Relaxed)
            && self.start_time == other.start_time
    }
}

impl Default for StreamStats {
    fn default() -> Self {
        Self {
            tx_packets: AtomicU64::new(0),
            rx_packets: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }
}

pub(crate) async fn create_capture_and_wrap_streams(
    capture_client: Option<Arc<dyn CaptureSender>>,
    chip_id: ChipId,
    chip_kind: ChipKind,
    device_name: String,
    packet_stream: Option<PacketStream>,
    packet_sink: Option<PacketSink>,
) -> (Option<PacketStream>, Option<PacketSink>, Option<Arc<StreamStats>>) {
    // Default to disabled, the capture actor is responsible for enabling it
    let enabled_flag = Arc::new(AtomicBool::new(false));

    if let Some(client) = &capture_client {
        let capture_create = CaptureCreate {
            chip_kind,
            device_name: device_name.to_string(),
            enabled_flag: enabled_flag.clone(),
        };
        let _ = client.create_capture(chip_id, capture_create).await;
    }

    match (packet_stream, packet_sink) {
        (Some(in_stream), Some(in_sink)) => {
            let stats = Arc::new(StreamStats::default());
            let stats_clone_rx = stats.clone();
            let stats_clone_tx = stats.clone();

            let mut sender = None;
            if let Some(client) = &capture_client {
                match client.packet_sender(chip_id).await {
                    Ok(s) => {
                        sender = Some(s);
                    }
                    Err(error) => {
                        warn!(%error, "Failed to start capture for chip {}", chip_id);
                    }
                }
            }

            // Wrap stream for stats (Rx from Transport perspective)
            let in_stream = in_stream.inspect(move |bytes| {
                stats_clone_rx.rx_packets.fetch_add(1, Ordering::Relaxed);
                stats_clone_rx.rx_bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            });

            // Wrap sink for stats (Tx to Transport perspective)
            let in_sink = in_sink.with(move |bytes: Bytes| {
                stats_clone_tx.tx_packets.fetch_add(1, Ordering::Relaxed);
                stats_clone_tx.tx_bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                futures::future::ready(Ok(bytes))
            });

            let (wrapped_stream, wrapped_sink) = (
                CapturedStream::new(Box::new(in_stream), sender.clone(), enabled_flag.clone()),
                CapturedSink::new(Box::pin(in_sink), sender, enabled_flag),
            );

            let out_stream: PacketStream =
                Box::new(wrapped_stream.filter_map(|item| futures::future::ready(Some(item))));

            let out_sink: PacketSink = Box::pin(wrapped_sink.sink_map_err(std::io::Error::other));

            (Some(out_stream), Some(out_sink), Some(stats))
        }
        (s, k) => (s, k, Some(Arc::new(StreamStats::default()))),
    }
}

/// Merges StreamStats (atomic counters) into generic RadioStats if the client
/// reported zero values (which often happens when the client is just a
/// wrapper or crashed).
/// Distributes StreamStats (bytes/counts) across a list of RadioStats.
///
/// If the actor reports counts, we use those to distribute the total bytes from
/// StreamStats proportional to the counts. We do NOT overwrite the actor's
/// counts in this case.
///
/// If the actor reports NO counts (total = 0), we fallback to using StreamStats
/// for both counts and bytes, distributing them evenly (or to the first entry)
/// as a best-effort.
pub(crate) fn distribute_stream_stats(
    radio_stats_list: &mut [netsim_model::NetsimRadioStats],
    stream_stats: &StreamStats,
) {
    if radio_stats_list.is_empty() {
        return;
    }

    let total_tx_count: u64 = radio_stats_list.iter().map(|s| s.tx_count).sum();
    let total_rx_count: u64 = radio_stats_list.iter().map(|s| s.rx_count).sum();

    // Stream RX is Chip TX
    let stream_tx_bytes = std::cmp::min(stream_stats.rx_bytes.load(Ordering::Relaxed), u64::MAX);
    let stream_tx_count = std::cmp::min(stream_stats.rx_packets.load(Ordering::Relaxed), u64::MAX);
    // Stream TX is Chip RX
    let stream_rx_bytes = std::cmp::min(stream_stats.tx_bytes.load(Ordering::Relaxed), u64::MAX);
    let stream_rx_count = std::cmp::min(stream_stats.tx_packets.load(Ordering::Relaxed), u64::MAX);

    if total_tx_count > 0 {
        // Distribute TX bytes proportional to TX counts
        for stat in radio_stats_list.iter_mut() {
            if stat.tx_bytes == 0 {
                stat.tx_bytes = (stream_tx_bytes as u128 * stat.tx_count as u128
                    / total_tx_count as u128) as u64;
            }
        }
    } else if stream_tx_count > 0 {
        // Fallback: Actor reports 0 counts, but Stream sees traffic.
        // Assign all to the first entry (or we could split, but first is safer
        // default).
        if let Some(first) = radio_stats_list.first_mut() {
            if first.tx_bytes == 0 {
                first.tx_bytes = stream_tx_bytes;
            }
            if first.tx_count == 0 {
                first.tx_count = stream_tx_count;
            }
        }
    }

    if total_rx_count > 0 {
        // Distribute RX bytes proportional to RX counts
        for stat in radio_stats_list.iter_mut() {
            if stat.rx_bytes == 0 {
                stat.rx_bytes = (stream_rx_bytes as u128 * stat.rx_count as u128
                    / total_rx_count as u128) as u64;
            }
        }
    } else if stream_rx_count > 0 {
        // Fallback: Actor reports 0 counts, but Stream sees traffic.
        if let Some(first) = radio_stats_list.first_mut() {
            if first.rx_bytes == 0 {
                first.rx_bytes = stream_rx_bytes;
            }
            if first.rx_count == 0 {
                first.rx_count = stream_rx_count;
            }
        }
    }
}

/// Helper to saturate u64 to i32::MAX
fn saturate_cast(val: u64) -> i32 {
    std::cmp::min(val, i32::MAX as u64) as i32
}

/// Converts internal Model stats to Proto stats for persistence/RPC.
pub(crate) fn to_proto_stats(
    m: netsim_model::NetsimRadioStats,
) -> netsim_proto::stats::NetsimRadioStats {
    let mut p = netsim_proto::stats::NetsimRadioStats::new();
    p.set_device_id(m.id);
    // Convert RadioKind to i32 for proto
    let k_i32: i32 = m.kind.into();
    if let Some(k) = netsim_proto::stats::netsim_radio_stats::Kind::from_i32(k_i32) {
        p.set_kind(k);
    } else {
        p.set_kind(netsim_proto::stats::netsim_radio_stats::Kind::UNSPECIFIED);
    }
    p.set_duration_secs(m.duration_secs);
    p.set_tx_count(saturate_cast(m.tx_count));
    p.set_rx_count(saturate_cast(m.rx_count));
    p.set_tx_bytes(saturate_cast(m.tx_bytes));
    p.set_rx_bytes(saturate_cast(m.rx_bytes));
    p.set_p2p_tx_count(saturate_cast(m.p2p_tx_count));
    p.set_p2p_rx_count(saturate_cast(m.p2p_rx_count));
    for pkt in m.invalid_packets() {
        let mut proto_pkt = netsim_proto::stats::InvalidPacket::new();
        let reason = match pkt.reason {
            netsim_model::InvalidPacketReason::ParseError => {
                netsim_proto::stats::invalid_packet::Reason::PARSE_ERROR
            }
            netsim_model::InvalidPacketReason::Unsupported => {
                netsim_proto::stats::invalid_packet::Reason::UNSUPPORTED
            }
            netsim_model::InvalidPacketReason::Others => {
                netsim_proto::stats::invalid_packet::Reason::OTHERS
            }
            netsim_model::InvalidPacketReason::Delayed => {
                netsim_proto::stats::invalid_packet::Reason::DELAYED
            }
            netsim_model::InvalidPacketReason::Unknown => {
                netsim_proto::stats::invalid_packet::Reason::UNKNOWN
            }
        };
        proto_pkt.set_reason(reason);
        proto_pkt.set_description(pkt.description.clone());
        p.invalid_packets.push(proto_pkt);
    }
    p
}

/// Creates a Proto RadioStats object directly from StreamStats.
/// Used for fallback when the actor is not reporting stats.
pub(crate) fn stream_to_proto_stats(
    device_id: u32,
    kind: netsim_model::RadioKind,
    duration_secs: u64,
    stream_stats: &StreamStats,
) -> netsim_proto::stats::NetsimRadioStats {
    let model_stats = stream_to_model_stats(device_id, kind, duration_secs, stream_stats);
    to_proto_stats(model_stats)
}

/// Creates a Model RadioStats object directly from StreamStats.
/// Used for fallback when the actor is not reporting stats.
pub(crate) fn stream_to_model_stats(
    device_id: u32,
    kind: netsim_model::RadioKind,
    duration_secs: u64,
    stream_stats: &StreamStats,
) -> netsim_model::NetsimRadioStats {
    let mut stats = netsim_model::NetsimRadioStats::default();
    stats.id = device_id;
    stats.name = "".to_string(); // or derive from kind?
    stats.kind = kind;
    stats.duration_secs = duration_secs;
    stats.tx_count = stream_stats.rx_packets.load(Ordering::Relaxed);
    stats.rx_count = stream_stats.tx_packets.load(Ordering::Relaxed);
    stats.tx_bytes = stream_stats.rx_bytes.load(Ordering::Relaxed);
    stats.rx_bytes = stream_stats.tx_bytes.load(Ordering::Relaxed);
    stats
}

pub(crate) fn to_proto_device_stats(
    device_id: u32,
    info: &netsim_model::DeviceInfo,
) -> netsim_proto::stats::NetsimDeviceStats {
    let mut stats = netsim_proto::stats::NetsimDeviceStats::new();
    stats.set_device_id(device_id);
    stats.set_kind(info.kind.clone());
    stats.set_version(info.version.clone());
    stats.set_sdk_version(info.sdk_version.clone());
    stats.set_build_id(info.build_id.clone());
    stats.set_variant(info.variant.clone());
    stats.set_arch(info.arch.clone());
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_proto_stats_converts_invalid_packets() {
        let mut model_stats = netsim_model::NetsimRadioStats::default();
        model_stats.push_invalid_packet(netsim_model::InvalidPacket {
            reason: netsim_model::InvalidPacketReason::ParseError,
            description: "Failed to parse header".to_string(),
        });
        model_stats.push_invalid_packet(netsim_model::InvalidPacket {
            reason: netsim_model::InvalidPacketReason::Unsupported,
            description: "Opcode 0x1234 not supported".to_string(),
        });
        model_stats.push_invalid_packet(netsim_model::InvalidPacket {
            reason: netsim_model::InvalidPacketReason::Others,
            description: "Internal buffer overflow".to_string(),
        });
        model_stats.push_invalid_packet(netsim_model::InvalidPacket {
            reason: netsim_model::InvalidPacketReason::Delayed,
            description: "Packet expired".to_string(),
        });
        model_stats.push_invalid_packet(netsim_model::InvalidPacket {
            reason: netsim_model::InvalidPacketReason::Unknown,
            description: "Unknown error".to_string(),
        });

        let proto_stats = to_proto_stats(model_stats);
        assert_eq!(proto_stats.invalid_packets.len(), 5);

        assert_eq!(
            proto_stats.invalid_packets[0].reason(),
            netsim_proto::stats::invalid_packet::Reason::PARSE_ERROR
        );
        assert_eq!(proto_stats.invalid_packets[0].description(), "Failed to parse header");

        assert_eq!(
            proto_stats.invalid_packets[1].reason(),
            netsim_proto::stats::invalid_packet::Reason::UNSUPPORTED
        );
        assert_eq!(proto_stats.invalid_packets[1].description(), "Opcode 0x1234 not supported");

        assert_eq!(
            proto_stats.invalid_packets[2].reason(),
            netsim_proto::stats::invalid_packet::Reason::OTHERS
        );
        assert_eq!(proto_stats.invalid_packets[2].description(), "Internal buffer overflow");

        assert_eq!(
            proto_stats.invalid_packets[3].reason(),
            netsim_proto::stats::invalid_packet::Reason::DELAYED
        );
        assert_eq!(proto_stats.invalid_packets[3].description(), "Packet expired");

        assert_eq!(
            proto_stats.invalid_packets[4].reason(),
            netsim_proto::stats::invalid_packet::Reason::UNKNOWN
        );
        assert_eq!(proto_stats.invalid_packets[4].description(), "Unknown error");
    }
}
