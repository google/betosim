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

use crate::error::WifiError;
type WifiResult<T> = Result<T, WifiError>;

#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU16, Ordering},
    },
};

use ap_actor::SharedKeyStore;
use netsim_model::ChipId;
#[cfg(unix)]
#[cfg(target_os = "linux")]
use tokio::io::unix::AsyncFd;
use tracing::{debug, error, info};

use crate::{gateway::GatewayTrait, medium::Medium, wifi_actor::WifiActor};

/// Gateway implementation for Host TAP devices.
///
/// This gateway is used when `netsimd` is launched with `--wifi-tap`.
/// It manages a pool of TAP interfaces (e.g. `cvd-etap-06`..`cvd-etap-10`)
/// or a single specific interface, connecting virtual chips directly to
/// the host kernel networking stack.
///
/// Unlike `SlirpGateway`, this requires root/CAP_NET_ADMIN (or pre-created
/// interfaces).
///
/// Flag to indicate TAP stream ID (High bit set)
const TAP_FLAG: u32 = 0x8000_0000;

/// A wrapper around a TAP interface file descriptor.
#[cfg(target_os = "linux")]
#[derive(Debug)]
struct TapInterface {
    name: String,
    poll_fd: AsyncFd<std::fs::File>,
}

#[cfg(target_os = "linux")]
impl TapInterface {
    fn name(&self) -> &str {
        &self.name
    }

    fn new(if_name: &str) -> WifiResult<Self> {
        // Open /dev/net/tun
        // O_RDWR is required. O_NONBLOCK is set later.
        let fd = nix::fcntl::open(
            "/dev/net/tun",
            nix::fcntl::OFlag::O_RDWR,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|e| {
            WifiError::Internal(Box::from(format!("Failed to open /dev/net/tun: {}", e)))
        })?;

        // Prepare ifreq
        // SAFETY: `libc::ifreq` is a C struct that is safe to zero-initialize.
        let mut if_req: libc::ifreq = unsafe { std::mem::zeroed() };

        // Set name
        let bytes = if_name.as_bytes();
        if bytes.len() >= libc::IFNAMSIZ {
            return Err(WifiError::Internal(Box::from("Interface name too long")));
        }
        for (dest, &b) in if_req.ifr_name.iter_mut().zip(bytes) {
            // Reinterpret the u8 byte bits as c_char for the FFI boundary.
            *dest = libc::c_char::from_ne_bytes([b]);
        }

        // Set flags
        // SAFETY: Accessing union field to set flags.
        // We cast the union to a short pointer because the field name `ifr_flags`
        // might vary or be inaccessible in some libc versions/bindgen outputs.
        unsafe {
            let flags = (libc::IFF_TAP | libc::IFF_NO_PI) as libc::c_short;
            *(&raw mut if_req.ifr_ifru).cast::<libc::c_short>() = flags;
        }

        // IOCTL
        // Define the ioctl using nix macro.
        nix::ioctl_write_ptr_bad!(tunsetiff, libc::TUNSETIFF, libc::ifreq);

        // SAFETY: `fd` is a valid open file descriptor for /dev/net/tun.
        // `if_req` is a valid libc::ifreq struct on the stack.
        unsafe { tunsetiff(fd.as_raw_fd(), &if_req) }
            .map_err(|e| WifiError::Internal(Box::from(format!("Failed to TUNSETIFF: {}", e))))?;

        // Set non-blocking
        let flags = nix::fcntl::fcntl(&fd, nix::fcntl::FcntlArg::F_GETFL)
            .map_err(|e| WifiError::Internal(Box::from(format!("Failed to get flags: {}", e))))?;

        let oflag = nix::fcntl::OFlag::from_bits_truncate(flags) | nix::fcntl::OFlag::O_NONBLOCK;

        nix::fcntl::fcntl(&fd, nix::fcntl::FcntlArg::F_SETFL(oflag)).map_err(|e| {
            WifiError::Internal(Box::from(format!("Failed to set non-blocking: {}", e)))
        })?;

        // Create File from OwnedFd
        // AsyncFd takes ownership of the File
        let file = std::fs::File::from(fd);

        // Wrap in AsyncFd
        let poll_fd =
            AsyncFd::new(file).map_err(|e| WifiError::Internal(Box::from(e.to_string())))?;

        info!("Opened TAP interface: {}", if_name);
        Ok(Self { name: if_name.to_string(), poll_fd })
    }
    async fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let mut guard = self.poll_fd.readable().await?;
            match guard.try_io(|inner| match std::io::Read::read(&mut inner.get_ref(), buf) {
                Ok(n) => Ok(n),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    Err(std::io::Error::from(std::io::ErrorKind::WouldBlock))
                }
                Err(e) => Err(e),
            }) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    async fn write(&self, buf: &[u8]) -> std::io::Result<usize> {
        loop {
            let mut guard = self.poll_fd.writable().await?;
            match guard.try_io(|inner| match std::io::Write::write(&mut inner.get_ref(), buf) {
                Ok(n) => Ok(n),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    Err(std::io::Error::from(std::io::ErrorKind::WouldBlock))
                }
                Err(e) => Err(e),
            }) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }
}

/// Gateway managing internal Chip -> TAP connections.
///
/// Since we may have multiple chips and multiple TAPs, this struct manages the
/// mapping. However, for the initial implementation, we might just support a
/// single requested TAP passed via CLI.
///
/// If `wifi_tap` arg is provided, we use that for the Primary Guest/Chip.
/// Future work: Dynamic TAP creation/assignment.
#[derive(Debug)]
pub struct TapGateway {
    // Map ChipId to TapInterface
    #[cfg(target_os = "linux")]
    taps: HashMap<ChipId, Arc<TapInterface>>,
    // Configuration
    if_name_or_pattern: String,
    // Pool management
    pool_range: Option<std::ops::RangeInclusive<u32>>,
    used_indices: HashMap<ChipId, u32>,
    seq: AtomicU16,
}

#[async_trait::async_trait]
impl GatewayTrait for TapGateway {
    async fn send_80211(
        &self,
        chip_id: ChipId,
        ieee80211: &netsim_packets::Ieee80211,
    ) -> Result<usize, crate::error::WifiError> {
        self.send_80211_impl(chip_id, ieee80211).await
    }

    fn should_handle(&self, chip_id: ChipId) -> bool {
        (chip_id.0 & TAP_FLAG) != 0
    }

    fn handle_incoming(
        &self,
        chip_id: ChipId,
        packet: bytes::Bytes,
        medium: &mut Medium,
        shared_keys: &SharedKeyStore,
        out_queue: &mut Vec<(u32, bytes::Bytes)>,
    ) {
        let real_id = chip_id.0 & !TAP_FLAG;
        debug!("TAP_PKT: Chip {} len {}", real_id, packet.len());
        medium.wifi_stats.incr_network_packets_rx();
        medium.wifi_stats.record_download_bytes(
            packet.len().saturating_sub(crate::gateway::ETHERNET_HEADER_LEN),
        );

        let Some(dest_mac_bytes) = packet.first_chunk::<6>() else {
            tracing::warn!("TAP_PKT: packet too short ({} bytes)", packet.len());
            return;
        };
        let dest_mac = netsim_packets::MacAddress::new(*dest_mac_bytes);

        if dest_mac.is_broadcast() || dest_mac.is_multicast() {
            let bssids = shared_keys.bssids.read().clone();
            for bssid in bssids {
                let seq = self.seq.fetch_add(1, Ordering::Relaxed);
                if let Some(bytes) = convert_8023_to_80211(packet.clone(), Some(bssid), seq) {
                    let _ = medium.transmit_from_infra(&bytes, out_queue);
                }
            }
            return;
        }

        let Some(bssid) = shared_keys.get_station_bssid(&dest_mac) else {
            tracing::warn!("Dropping unicast packet to unknown station {}", dest_mac);
            return;
        };

        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let Some(bytes) = convert_8023_to_80211(packet.clone(), Some(bssid), seq) else {
            medium.wifi_stats.log_and_incr_err_count(&crate::error::WifiError::Frame(Box::from(
                "Failed to convert TAP packet to 802.11",
            )));
            return;
        };

        let res = medium.transmit_from_infra(&bytes, out_queue);
        medium.wifi_stats.log_outcome(res, |s, _| {
            // Nothing to do for success of transmit_from_infra
        });
    }

    async fn on_start(&mut self, _ctx: &mut actor_framework::DynContext<WifiActor>) {}

    async fn on_chip_create(
        &mut self,
        chip_id: ChipId,
        ctx: &mut actor_framework::DynContext<WifiActor>,
    ) {
        self.attach(chip_id, ctx);
    }

    async fn on_chip_remove(
        &mut self,
        chip_id: ChipId,
        _ctx: &mut actor_framework::DynContext<WifiActor>,
    ) {
        self.release(chip_id);
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl TapGateway {
    fn parse_config(config: &str) -> (String, Option<std::ops::RangeInclusive<u32>>) {
        if config == "cvd-etap" {
            ("cvd-etap-%02d".to_string(), Some(6..=10))
        } else if config.contains('%') {
            (config.to_string(), Some(6..=10))
        } else {
            (config.to_string(), None)
        }
    }

    pub fn new(config: String) -> Self {
        let (if_name_or_pattern, pool_range) = Self::parse_config(&config);

        // Pre-check permissions to helpful error messages at startup.
        // The main daemon should also perform a pre-flight check and fail fast,
        // but we keep this log for debugging context.
        #[cfg(target_os = "linux")]
        {
            if let Err(e) = nix::fcntl::open(
                "/dev/net/tun",
                nix::fcntl::OFlag::O_RDWR,
                nix::sys::stat::Mode::empty(),
            ) {
                error!("TAP configuration enabled, but failed to open /dev/net/tun: {}", e);
                error!(
                    "Please ensure you have CAP_NET_ADMIN capabilities or access to /dev/net/tun."
                );
                error!("If running locally, you may need 'sudo' or group membership.");
                // We do NOT panic here anymore, trusting netsimd to handle the
                // pre-flight check failure.
            }
        }

        Self {
            #[cfg(target_os = "linux")]
            taps: HashMap::new(),
            if_name_or_pattern,
            pool_range,
            used_indices: HashMap::new(),
            seq: AtomicU16::new(100),
        }
    }

    /// Checks if the TAP environment is valid (e.g. /dev/net/tun exists and is
    /// accessible, and the requested TAP interface can be opened).
    /// Returns `Ok(())` if valid, or an error message if not.
    pub fn preflight_check(config: &str) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            // 1. Check /dev/net/tun access
            if let Err(e) = std::fs::OpenOptions::new().read(true).write(true).open("/dev/net/tun")
            {
                return Err(format!(
                    r#"TAP configuration requested, but failed to access /dev/net/tun: {}.
Please ensure you have CAP_NET_ADMIN capabilities or access to /dev/net/tun.
If running locally, you may need 'sudo' or group membership (e.g. 'cvdnetwork').
If using Cuttlefish, please ensure your environment is correctly set up (e.g. 'launch_cvd' or 'cvd-host_package')."#,
                    e
                ));
            }

            // 2. Check if we can open the requested interface (or first in pool)
            let (pattern, range) = Self::parse_config(config);
            let test_name = if let Some(r) = range {
                // Check first index
                let idx = *r.start();
                if pattern.contains("%02d") {
                    pattern.replace("%02d", &format!("{:02}", idx))
                } else {
                    pattern.replace("%d", &format!("{}", idx))
                }
            } else {
                pattern
            };

            info!("Validating TAP interface: {}", test_name);
            if let Err(e) = TapInterface::new(&test_name) {
                return Err(format!(
                    r#"TAP validation failed for interface '{}': {}.
If using a TAP pool (e.g. cvd-etap), ensure the interfaces are created.
(e.g. check 'ip link show {}')"#,
                    test_name, e, test_name
                ));
            }
            info!("TAP validation succeeded for {}", test_name);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = config;
            return Err("TAP configuration is only supported on Unix systems.".to_string());
        }
        Ok(())
    }

    /// Returns the next available TAP interface from the pool, or the single
    /// configured TAP. Returns `(index, name)` where index is 0 for
    /// single-tap mode.
    fn allocate_next_tap(&self) -> Option<(u32, String)> {
        if let Some(range) = &self.pool_range {
            range
                .clone()
                .find(|&i| !self.used_indices.values().any(|&idx| idx == i))
                .map(|i| (i, self.format_name(i)))
        } else {
            // Single mode: use index 0 as placeholder
            Some((0, self.format_name(0)))
        }
    }

    fn format_name(&self, index: u32) -> String {
        if self.pool_range.is_some() {
            // Pool mode: apply formatting
            if self.if_name_or_pattern.contains("%02d") {
                return self.if_name_or_pattern.replace("%02d", &format!("{:02}", index));
            } else if self.if_name_or_pattern.contains("%d") {
                return self.if_name_or_pattern.replace("%d", &format!("{}", index));
            }
        }
        // Single mode or fallback: return raw name (ignoring index if not a pattern)
        // If it was a shortcut "cvd-etap" which implies matching "cvd-etap-%02d" logic
        // but user passed it as single, we just use it as is?
        // Actually `new` logic handles the defaults.
        self.if_name_or_pattern.clone()
    }

    fn release(&mut self, chip_id: ChipId) {
        if let Some(idx) = self.used_indices.remove(&chip_id) {
            info!("Released TAP index {} for chip {}", idx, chip_id);
        }
        #[cfg(target_os = "linux")]
        self.taps.remove(&chip_id);
    }

    /// Adds a TAP interface for a Chip.
    /// Returns a stream of packets read from the TAP.
    #[cfg(target_os = "linux")]
    pub fn add(
        &mut self,
        chip_id: ChipId,
    ) -> WifiResult<impl futures::Stream<Item = bytes::Bytes> + use<>> {
        // Allocate TAP
        let (index, if_name) = self
            .allocate_next_tap()
            .ok_or_else(|| WifiError::Internal(Box::from("No available TAP interfaces in pool")))?;

        // Record usage if in pool mode
        if self.pool_range.is_some() {
            self.used_indices.insert(chip_id, index);
        }

        let tap = Arc::new(TapInterface::new(&if_name)?);
        self.taps.insert(chip_id, tap.clone());

        // Spawn a reader for this TAP and return it as a stream
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            let mut buf = [0u8; 1500]; // Standard MTU
            loop {
                match tap.read(&mut buf).await {
                    Ok(n) => {
                        if let Some(slice) = buf.get(..n)
                            && !slice.is_empty()
                        {
                            let packet = bytes::Bytes::copy_from_slice(slice);
                            if tx.send(packet).is_err() {
                                // Receiver closed
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        error!("TapGateway: Error reading from tap: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
    }

    /// Attaches a TAP interface to the Chip and Context.
    pub fn attach(&mut self, chip_id: ChipId, ctx: &mut actor_framework::DynContext<WifiActor>) {
        #[cfg(target_os = "linux")]
        match self.add(chip_id) {
            Ok(stream) => {
                ctx.add_stream(ChipId(chip_id.0 | TAP_FLAG), Box::pin(stream));
                // We don't have the if_name easily here without restructuring `add`,
                // but we can look it up in `taps` if we want to log the name.
                if let Some(tap) = self.taps.get(&chip_id) {
                    info!("Attached TAP {} to chip {}", tap.name(), chip_id);
                }
            }
            Err(e) => {
                error!("Failed to attach TAP for chip {}: {}", chip_id, e);
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (chip_id, ctx);
            error!("TAP attach not supported on non-Unix");
        }
    }

    /// Sends an 802.11 frame to the TAP interface (converting to 802.3).
    /// Returns true if sent, false if conversion failed or no TAP configured.
    pub async fn send_80211_impl(
        &self,
        chip_id: ChipId,
        ieee80211: &netsim_packets::Ieee80211,
    ) -> Result<usize, crate::error::WifiError> {
        #[cfg(target_os = "linux")]
        {
            let Some(tap) = self.taps.get(&chip_id) else {
                return Err(crate::error::WifiError::Network(Box::from(format!(
                    "No TAP interface configured for Chip {}",
                    chip_id.0
                ))));
            };

            // Drop QosNodata frames (keep-alives/null data) as they contain no payload
            // and cannot be converted to Ethernet.
            if ieee80211.is_qos_nodata() {
                return Ok(0);
            }

            let eth = ieee80211.to_ieee8023().map_err(|e| {
                crate::error::WifiError::Frame(Box::from(format!("TAP conversion failed: {}", e)))
            })?;
            let payload_len = eth.len().saturating_sub(crate::gateway::ETHERNET_HEADER_LEN);
            let written = tap.write(&eth).await.map_err(|e| {
                crate::error::WifiError::Network(Box::from(format!("TAP write failed: {}", e)))
            });
            written.map(|_| payload_len)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (chip_id, ieee80211);
            Err(crate::error::WifiError::Network(Box::from("TAP not supported on non-Linux")))
        }
    }
}

/// Converts an 802.3 packet (from TAP) to 802.11 for the Medium.
#[allow(clippy::collapsible_if)]
fn convert_8023_to_80211(
    packet: bytes::Bytes,
    bssid: Option<netsim_packets::MacAddress>,
    seq: u16,
) -> Option<bytes::Bytes> {
    use netsim_packets::{FrameDirection, Ieee80211};
    if let Some(bssid) = bssid {
        if let Ok(ieee80211) =
            Ieee80211::from_ieee8023_qos(&packet, bssid, FrameDirection::FromAp, true, seq)
        {
            if let Ok(bytes) = ieee80211.encode_to_vec() {
                return Some(bytes::Bytes::from(bytes));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_config_parsing() {
        // 1. Legacy shortcut
        let gw = TapGateway::new("cvd-etap".to_string());
        assert_eq!(gw.if_name_or_pattern, "cvd-etap-%02d");
        assert_eq!(gw.pool_range, Some(6..=10));

        // 2. Pattern
        let gw = TapGateway::new("tap-%d".to_string());
        assert_eq!(gw.if_name_or_pattern, "tap-%d");
        assert_eq!(gw.pool_range, Some(6..=10));

        // 3. Single interface
        let gw = TapGateway::new("my-tap".to_string());
        assert_eq!(gw.if_name_or_pattern, "my-tap");
        assert_eq!(gw.pool_range, None);
    }

    #[test]
    fn test_format_name() {
        // Pattern with %02d
        let gw = TapGateway::new("cvd-etap-%02d".to_string());
        assert_eq!(gw.format_name(6), "cvd-etap-06");
        assert_eq!(gw.format_name(10), "cvd-etap-10");

        // Pattern with %d
        let gw = TapGateway::new("tap-%d".to_string());
        assert_eq!(gw.format_name(1), "tap-1");
        assert_eq!(gw.format_name(123), "tap-123");

        // Single interface
        let gw = TapGateway::new("eth0".to_string());
        assert_eq!(gw.format_name(999), "eth0");
    }

    #[test]
    fn test_allocate_next_tap() {
        // Pool mode
        let mut gw = TapGateway::new("cvd-etap".to_string()); // 6..=10

        // Allocate first
        let (idx1, name1) = gw.allocate_next_tap().expect("Should allocate");
        assert_eq!(idx1, 6);
        assert_eq!(name1, "cvd-etap-06");
        gw.used_indices.insert(ChipId(123), idx1);

        // Allocate second
        let (idx2, name2) = gw.allocate_next_tap().expect("Should allocate");
        assert_eq!(idx2, 7);
        assert_eq!(name2, "cvd-etap-07");
        gw.used_indices.insert(ChipId(456), idx2);

        // Release first
        gw.release(ChipId(123));

        // Re-allocate should reuse 6 or get 8?
        // Our logic iterates range: 6, 7, 8...
        // 6 is free now. 7 is used.
        let (idx3, name3) = gw.allocate_next_tap().expect("Should allocate");
        assert_eq!(idx3, 6);
        assert_eq!(name3, "cvd-etap-06");
    }

    #[test]
    fn test_allocate_exhausted_pool() {
        let mut gw = TapGateway::new("cvd-etap".to_string()); // 6,7,8,9,10 = 5 slots

        // Fill all
        for i in 6..=10 {
            gw.used_indices.insert(ChipId(i), i);
        }

        // Try allocate
        assert!(gw.allocate_next_tap().is_none());
    }

    #[test]
    fn test_allocate_single_mode() {
        let gw = TapGateway::new("fixed-tap".to_string());

        let (idx, name) = gw.allocate_next_tap().expect("Should allocate");
        assert_eq!(idx, 0);
        assert_eq!(name, "fixed-tap");

        // Single mode doesn't track used_indices for exclusion in the same way
        // (allocate_next_tap just returns 0), but let's verify it behaves consistently.
        // The current implementation for single mode always returns (0, name).
        // It relies on the Caller to not double-allocate if they care,
        // or maybe it supports sharing the same TAP?
        // For now, verification is that it returns the name.
        let (idx2, name2) = gw.allocate_next_tap().expect("Should allocate again");
        assert_eq!(idx2, 0);
        assert_eq!(name2, "fixed-tap");
    }
}
