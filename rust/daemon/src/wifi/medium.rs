// Copyright 2023 The Android Open Source Project
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

use crate::args::DebugArgs;
use crate::wifi::error::{WifiError, WifiResult};
use crate::wifi::frame::Frame;
use crate::wifi::hostapd::Hostapd;
use crate::wifi::hwsim_attr_set::HwsimAttrSet;
use crate::wifi::stats::WifiStats;
use bytes::Bytes;
use log::{debug, info, warn};
use netsim_packets::ieee80211::{DataSubType, Ieee80211, MacAddress};
use netsim_packets::mac80211_hwsim::{HwsimCmd, HwsimMsg, HwsimMsgHdr, NlMsgHdr};
use pdl_runtime::Packet;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

const NLMSG_MIN_TYPE: u16 = 0x10;
const NL_AUTO_SEQ: u16 = 0;
const NL_AUTO_PORT: u32 = 0;
// Default values for mac80211_hwsim.
const RX_RATE: u32 = 1;
const SIGNAL: u32 = 4294967246; // -50
const NL_MSG_HDR_LEN: usize = 16;

pub struct Processor {
    pub hostapd: bool,
    pub network: bool,
    pub wmedium: bool,
    pub frame: Frame,
    pub plaintext_ieee80211: Option<Ieee80211>,
}

impl Processor {
    /// Returns the decrypted IEEE 802.11 frame if available.
    /// Otherwise, returns the original IEEE 80211 frame.
    pub fn get_ieee80211(&self) -> &Ieee80211 {
        self.plaintext_ieee80211.as_ref().unwrap_or(&self.frame.ieee80211)
    }

    /// Returns the decrypted IEEE 802.11 frame as bytes if available.
    /// Otherwise, returns the original IEEE 80211 frame as bytes.
    pub fn get_ieee80211_bytes(&self) -> Bytes {
        if let Some(ieee80211) = self.plaintext_ieee80211.as_ref() {
            ieee80211.encode_to_vec().unwrap().into()
        } else {
            self.frame.data.clone().into()
        }
    }
}

#[derive(Clone)]
struct Station {
    // The station's client_id. Equivalent to the chip_id of the WifiChip
    client_id: u32,
    // Ieee80211 source address
    addr: MacAddress,
    // Hwsim virtual address from HWSIM_ATTR_ADDR_TRANSMITTER
    // Used to create the HwsimMsg to stations.
    hwsim_addr: MacAddress,
    // Caches the frequency (HWSIM_ATTR_FREQ) from the latest HwsimMsg request.
    // This value is used to populate the HWSIM_ATTR_FREQ field in HwsimMsg response.
    freq: Arc<AtomicU32>,
}

impl Station {
    fn new(client_id: u32, addr: MacAddress, hwsim_addr: MacAddress) -> Self {
        Self { client_id, addr, hwsim_addr, freq: Arc::new(AtomicU32::new(0)) }
    }

    fn update_freq(&self, freq: u32) {
        self.freq.store(freq, Ordering::Relaxed);
    }
}

#[derive(Clone)]
pub struct Client {
    pub enabled: Arc<AtomicBool>,
    pub tx_count: Arc<AtomicU32>,
    pub rx_count: Arc<AtomicU32>,
}

impl Client {
    fn new() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(true)),
            tx_count: Arc::new(AtomicU32::new(0)),
            rx_count: Arc::new(AtomicU32::new(0)),
        }
    }
}

pub struct Medium {
    callback: HwsimCmdCallback,
    // Map of Ieee80211 source address to Station struct
    stations: RwLock<HashMap<MacAddress, Arc<Station>>>,
    // Map of client_id (equivalent to chip_id) to Client struct
    clients: RwLock<HashMap<u32, Client>>,
    // Simulate the re-transmission of frames sent to hostapd
    ap_simulation: bool,
    hostapd: Arc<Hostapd>,
    wifi_stats: WifiStats,
    debug: Arc<DebugArgs>,
}

type HwsimCmdCallback = fn(u32, &Bytes);
impl Medium {
    pub fn new(
        callback: HwsimCmdCallback,
        hostapd: Arc<Hostapd>,
        wifi_stats: WifiStats,
        debug: Arc<DebugArgs>,
    ) -> Medium {
        Self {
            callback,
            stations: RwLock::new(HashMap::new()),
            clients: RwLock::new(HashMap::new()),
            ap_simulation: true,
            hostapd,
            wifi_stats,
            debug,
        }
    }

    pub fn add(&self, client_id: u32) {
        let _ =
            self.clients.write().expect("RwLock poisoned").entry(client_id).or_insert_with(|| {
                info!("Insert client {client_id}");
                Client::new()
            });
    }

    pub fn remove(&self, client_id: u32) {
        self.stations.write().expect("RwLock poisoned").retain(|_, s| s.client_id != client_id);
        self.clients.write().expect("RwLock poisoned").remove(&client_id);
    }

    pub fn reset(&self, client_id: u32) {
        if let Some(client) = self.clients.read().expect("RwLock poisoned").get(&client_id) {
            client.enabled.store(true, Ordering::Relaxed);
            client.tx_count.store(0, Ordering::Relaxed);
            client.rx_count.store(0, Ordering::Relaxed);
        }
    }

    pub fn get(&self, client_id: u32) -> Option<Client> {
        self.clients.read().expect("RwLock poisoned").get(&client_id).map(|c| c.to_owned())
    }

    fn contains_client(&self, client_id: u32) -> bool {
        self.clients.read().unwrap().contains_key(&client_id)
    }

    fn stations(&self) -> impl Iterator<Item = Arc<Station>> + use<> {
        self.stations.read().expect("RwLock poisoned").clone().into_values()
    }

    fn contains_station(&self, addr: &MacAddress) -> bool {
        self.stations.read().expect("RwLock poisoned").contains_key(addr)
    }

    fn get_station(&self, addr: &MacAddress) -> WifiResult<Arc<Station>> {
        self.stations
            .read()
            .expect("RwLock poisoned")
            .get(addr)
            .cloned()
            .ok_or_else(|| WifiError::Client(format!("Station not found for address: {addr}")))
    }

    fn upsert_station(&self, client_id: u32, frame: &Frame) -> WifiResult<()> {
        let src_addr = frame.ieee80211.get_source();
        let hwsim_addr = frame.transmitter.ok_or(WifiError::Frame(format!(
            "Missing transmitter attribute in frame for client: {client_id}"
        )))?;
        self.stations.write().expect("RwLock poisoned").entry(src_addr).or_insert_with(|| {
            info!(
                "Insert station with client id {client_id}, hwsimaddr: {hwsim_addr}, \
                Ieee80211 addr: {src_addr}"
            );
            Arc::new(Station::new(client_id, src_addr, hwsim_addr))
        });
        if !self.contains_client(client_id) {
            warn!("Client {client_id} is missing");
            self.add(client_id);
        }
        Ok(())
    }

    /// Send Ack frame (TX_INFO_FRAME) back to source.
    pub fn ack_frame(&self, client_id: u32, frame: &Frame) -> WifiResult<()> {
        let hwsim_msg_tx_info = build_tx_info(&frame.hwsim_msg)?.encode_to_vec()?;
        (self.callback)(client_id, &hwsim_msg_tx_info.into());
        self.incr_tx(client_id)
    }

    /// Process commands from the kernel's mac80211_hwsim subsystem.
    ///
    /// This is the processing that will be implemented:
    ///
    /// * The source MacAddress in 802.11 frames is re-mapped to a globally
    ///   unique MacAddress because resumed Emulator AVDs appear with the
    ///   same address.
    ///
    /// * 802.11 frames sent between stations
    ///
    /// * 802.11 multicast frames are re-broadcast to connected stations.
    pub fn get_processor(&self, client_id: u32, packet: &Bytes) -> WifiResult<Processor> {
        let frame = self
            .validate(client_id, packet)
            .map_err(|e| WifiError::Frame(format!("error validate for client {client_id}: {e}")))?;

        if self.debug.debug_no_traffic {
            return Ok(Processor {
                hostapd: false,
                network: false,
                wmedium: false,
                frame,
                plaintext_ieee80211: None,
            });
        }

        self.wifi_stats.incr_hwsim_frames_rx();

        // Creates Stations on the fly when there is no config file.
        // WiFi Direct will use a randomized mac address for probing
        // new networks. This block associates the new mac with the station.
        self.upsert_station(client_id, &frame).map_err(|e| {
            WifiError::Frame(format!("error upsert station for client {client_id}: {e}"))
        })?;

        let plaintext_ieee80211 = self.hostapd.try_decrypt(&frame.ieee80211);

        let mut processor = Processor {
            hostapd: false,
            network: false,
            wmedium: false,
            frame,
            plaintext_ieee80211,
        };

        if processor.get_ieee80211().is_mgmt() {
            self.wifi_stats.incr_mgmt_frames_rx();
        }

        let dest_addr = processor.frame.ieee80211.get_destination();

        if let Some(freq) = processor.frame.attrs.freq {
            self.get_station(&processor.frame.ieee80211.get_source())
                .map(|sta| sta.update_freq(freq))
                .map_err(|e| self.wifi_stats.log_and_incr_err_count(&e))
                .ok();
        };

        if self.contains_station(&dest_addr) {
            if !self.debug.debug_no_wmedium {
                processor.wmedium = true;
            }
            return Ok(processor);
        }
        if dest_addr.is_multicast()
            && !(self.debug.debug_no_wmedium
                || self.debug.debug_no_mdns_wmedium && dest_addr.is_mdns())
        {
            processor.wmedium = true;
        }

        let ieee80211: &Ieee80211 = processor.get_ieee80211();
        // If the BSSID is unicast and does not match the hostapd's BSSID, the packet is not handled by hostapd. Skip further checks.
        if let Some(bssid) = ieee80211.get_bssid() {
            if !bssid.is_multicast() && bssid != self.hostapd.get_bssid() {
                return Ok(processor);
            }
        }
        // Data frames
        if ieee80211.is_data() {
            // EAPoL is used in Wi-Fi 4-way handshake.
            let is_eapol = ieee80211.is_eapol().unwrap_or_else(|e| {
                debug!("Failed to get ether type for is_eapol(): {e}");
                false
            });
            if is_eapol {
                processor.hostapd = true;
            } else if ieee80211.is_to_ap() {
                // Don't forward Null Data frames to slirp because they are used to maintain an active connection and carry no user data.
                if ieee80211.stype() != DataSubType::Nodata.into() {
                    processor.network = if self.debug.debug_no_network
                        || self.debug.debug_no_guest_to_host_mdns && dest_addr.is_mdns()
                    {
                        false
                    } else if self.enabled(client_id).unwrap() {
                        true
                    } else {
                        // If the client is disabled, block all packets to the internet so it can connect to the AP but has no internet access.
                        let destination = ieee80211.get_destination();
                        destination.is_multicast() || destination == self.hostapd.get_bssid()
                    };
                }
            }
        } else {
            // Mgmt or Ctrl frames.
            // TODO: Refactor this check after verifying all packets sent to hostapd are of ToAP type.
            let addr1 = ieee80211.get_addr1();
            if addr1.is_multicast() || addr1.is_broadcast() || addr1 == self.hostapd.get_bssid() {
                processor.hostapd = true;
            }
        }
        Ok(processor)
    }

    fn validate(&self, client_id: u32, packet: &Bytes) -> WifiResult<Frame> {
        let hwsim_msg = HwsimMsg::decode_full(packet)?;

        // The virtio handler only accepts HWSIM_CMD_FRAME, HWSIM_CMD_TX_INFO_FRAME and HWSIM_CMD_REPORT_PMSR
        // in https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/drivers/net/wireless/virtual/mac80211_hwsim.c
        match hwsim_msg.hwsim_hdr.hwsim_cmd {
            HwsimCmd::Frame => {
                let frame = Frame::parse(&hwsim_msg)?;
                // Incoming frame must contain transmitter, flag, cookie, and tx_info fields.
                if frame.transmitter.is_none()
                    || frame.flags.is_none()
                    || frame.cookie.is_none()
                    || frame.tx_info.is_none()
                {
                    return Err(WifiError::Frame(format!(
                        "Missing Hwsim attributes for incoming packet for client: {client_id}"
                    )));
                }
                // Use as receiver for outgoing HwsimMsg.
                let hwsim_addr = frame.transmitter.ok_or(WifiError::Frame(format!(
                    "Missing transmitter attribute in frame for client: {client_id}"
                )))?;
                let flags = frame.flags.ok_or(WifiError::Frame(format!(
                    "Missing flags attribute in frame for client: {client_id}"
                )))?;
                let cookie = frame.cookie.ok_or(WifiError::Frame(format!(
                    "Missing cookie attribute in frame for client: {client_id}"
                )))?;
                debug!(
                    "Frame chip {}, transmitter {}, flags {}, cookie {}, ieee80211 {}",
                    client_id, hwsim_addr, flags, cookie, frame.ieee80211
                );
                Ok(frame)
            }
            _ => Err(WifiError::Frame(format!(
                "Another command found {hwsim_msg:?} for client: {client_id}"
            ))),
        }
    }

    /// Handle Wi-Fi Ieee802.3 frame from network.
    /// Convert to HwsimMsg and send to clients.
    pub fn process_ieee8023_response(&self, packet: &Bytes) -> WifiResult<()> {
        if self.debug.debug_no_traffic || self.debug.debug_no_network {
            return Ok(());
        }
        Ieee80211::from_ieee8023(packet, self.hostapd.get_bssid())
            .map_err(|e| WifiError::Frame(format!("Failed to process IEEE 802.3 response: {e}")))
            .and_then(|ieee80211| self.handle_ieee80211_response(ieee80211))
    }

    /// Handle Wi-Fi Ieee802.11 frame from hostapd.
    /// Convert to HwsimMsg and send to clients.
    pub fn process_ieee80211_response(&self, packet: &Bytes) -> WifiResult<()> {
        if self.debug.debug_no_traffic {
            return Ok(());
        }
        Ieee80211::decode_full(packet)
            .map_err(|e| WifiError::Frame(format!("Failed to process IEEE 802.11 response: {e}")))
            .and_then(|ieee80211| self.handle_ieee80211_response(ieee80211))
    }

    /// Determine the client id based on destination and send to client.
    fn handle_ieee80211_response(&self, mut ieee80211: Ieee80211) -> WifiResult<()> {
        if let Some(encrypted_ieee80211) = self.hostapd.try_encrypt(&ieee80211) {
            ieee80211 = encrypted_ieee80211;
        }
        let dest_addr = ieee80211.get_destination();
        if let Ok(destination) = self.get_station(&dest_addr) {
            self.send_ieee80211_response(&ieee80211, &destination)?;
        } else if dest_addr.is_multicast() {
            // Deduplicates based on (hwsim_addr, freq) as these are used to construct
            // the HwsimMsg for the destination.
            let mut sent_to_hwsim_addrs_freq = std::collections::HashSet::new();
            for destination in self.stations() {
                let freq = destination.freq.load(Ordering::Relaxed);
                if sent_to_hwsim_addrs_freq.insert((destination.hwsim_addr, freq)) {
                    self.send_ieee80211_response(&ieee80211, &destination)?;
                }
            }
        } else {
            return Err(WifiError::Transmission(format!(
                "Send frame response to unknown destination: {dest_addr}"
            )));
        }
        Ok(())
    }

    fn send_ieee80211_response(
        &self,
        ieee80211: &Ieee80211,
        destination: &Station,
    ) -> WifiResult<()> {
        if !self.enabled(destination.client_id)? {
            debug!("Dropping frame to disabled client {}", destination.client_id);
            return Ok(());
        }

        let hwsim_msg = self.create_hwsim_msg_from_ieee80211(ieee80211, destination)?;
        self.wifi_stats.incr_hwsim_frames_tx();
        (self.callback)(destination.client_id, &hwsim_msg.encode_to_vec()?.into());
        self.incr_rx(destination.client_id)?;
        Ok(())
    }

    fn create_hwsim_msg_from_ieee80211(
        &self,
        ieee80211: &Ieee80211,
        destination: &Station,
    ) -> WifiResult<HwsimMsg> {
        let mut builder = HwsimAttrSet::builder();
        // Attributes required by mac80211_hwsim.
        builder.receiver(&destination.hwsim_addr.to_vec());
        let frame_bytes = ieee80211.encode_to_vec()?;
        builder.frame(&frame_bytes);
        builder.rx_rate(RX_RATE);
        builder.signal(SIGNAL);
        builder.freq(destination.freq.load(Ordering::Relaxed));
        let attributes = builder.build()?.attributes;
        let hwsim_hdr = HwsimMsgHdr { hwsim_cmd: HwsimCmd::Frame, hwsim_version: 0, reserved: 0 };
        let nlmsg_len = (NL_MSG_HDR_LEN + hwsim_hdr.encoded_len() + attributes.len()) as u32;
        let nl_hdr = NlMsgHdr {
            nlmsg_len,
            nlmsg_type: NLMSG_MIN_TYPE,
            nlmsg_flags: NL_AUTO_SEQ,
            nlmsg_seq: NL_AUTO_PORT,
            nlmsg_pid: 0,
        };
        Ok(HwsimMsg { nl_hdr, hwsim_hdr, attributes })
    }

    pub fn set_enabled(&self, client_id: u32, enabled: bool) {
        if let Some(client) = self.clients.read().expect("RwLock poisoned").get(&client_id) {
            client.enabled.store(enabled, Ordering::Relaxed);
        }
    }

    fn enabled(&self, client_id: u32) -> WifiResult<bool> {
        self.clients
            .read()
            .expect("RwLock poisoned")
            .get(&client_id)
            .map(|c| c.enabled.load(Ordering::Relaxed))
            .ok_or_else(|| WifiError::Client(format!("client {client_id} is missing")))
    }

    fn incr_tx(&self, client_id: u32) -> WifiResult<()> {
        self.clients.read().expect("RwLock poisoned").get(&client_id).map_or(
            Err(WifiError::Client(format!("client {client_id} is missing for incr_tx"))),
            |c| {
                c.tx_count.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        )
    }

    fn incr_rx(&self, client_id: u32) -> WifiResult<()> {
        self.clients.read().expect("RwLock poisoned").get(&client_id).map_or(
            Err(WifiError::Client(format!("client {client_id} is missing for incr_rx"))),
            |c| {
                c.rx_count.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        )
    }

    // Send an 802.11 frame from a station to a station after wrapping in HwsimMsg.
    // The HwsimMsg is processed by mac80211_hwsim framework in the guest OS.
    //
    // Simulates transmission through hostapd.
    fn send_from_sta_frame(
        &self,
        frame: &Frame,
        ieee80211: &Ieee80211,
        source: &Station,
        destination: &Station,
    ) -> WifiResult<()> {
        if source.client_id != destination.client_id
            && self.enabled(source.client_id)?
            && self.enabled(destination.client_id)?
        {
            match self.create_hwsim_msg(frame, ieee80211, &destination.hwsim_addr) {
                Ok(packet) => {
                    self.wifi_stats.incr_wmedium_frames_tx();
                    self.wifi_stats.incr_hwsim_frames_tx();
                    self.incr_rx(destination.client_id)?;
                    (self.callback)(destination.client_id, &packet.encode_to_vec()?.into());
                    log_hwsim_msg(frame, source.client_id, destination.client_id);
                }
                Err(e) => self.wifi_stats.log_and_incr_err_count(&e),
            }
        }
        Ok(())
    }

    // Broadcast an 802.11 frame to all stations.
    // TODO: Compare with the implementations in mac80211_hwsim.c and wmediumd.c.
    fn broadcast_from_sta_frame(
        &self,
        frame: &Frame,
        ieee80211: &Ieee80211,
        source: &Station,
    ) -> WifiResult<()> {
        // Deduplicates based on (hwsim_addr, freq) as these are used to construct
        // the HwsimMsg for the destination.
        let mut sent_to_hwsim_addrs_freq = std::collections::HashSet::new();
        for destination in self.stations().filter(|sta| sta.addr != source.addr) {
            let current_freq = destination.freq.load(Ordering::Relaxed);
            if sent_to_hwsim_addrs_freq.insert((destination.hwsim_addr, current_freq)) {
                self.send_from_sta_frame(frame, ieee80211, source, &destination)?;
            }
        }
        Ok(())
    }

    /// Queues the frame for sending to medium.
    ///
    /// The `frame` contains an `ieee80211` field, but it might be encrypted. This function uses the provided `ieee80211` parameter directly, as it's expected to be decrypted if necessary.
    pub fn queue_frame(&self, frame: Frame, ieee80211: Ieee80211) -> WifiResult<()> {
        let source = self.get_station(&ieee80211.get_source())?;
        let dest_addr = ieee80211.get_destination();
        if self.contains_station(&dest_addr) {
            // Unicast to another station
            debug!("Frame deliver unicast from {} to {}", source.addr, dest_addr);
            self.wifi_stats.incr_wmedium_unicast_frames_tx();
            let destination = self.get_station(&dest_addr)?;
            self.send_from_sta_frame(&frame, &ieee80211, &source, &destination)?;
            return Ok(());
        } else if dest_addr.is_multicast() {
            // Broadcast/Multicast from a station
            debug!("Frame multicast {ieee80211}");
            if dest_addr.is_mdns() {
                self.wifi_stats.incr_mdns_count();
            }
            self.broadcast_from_sta_frame(&frame, &ieee80211, &source)?;
            return Ok(());
        }

        Err(WifiError::Transmission(format!(
            "Dropped packet from {} to {}",
            source.addr, dest_addr
        )))
    }

    // Simulate transmission through hostapd by rewriting frames with 802.11 ToDS
    // and hostapd_bssid to frames with FromDS set.
    fn create_hwsim_attr(
        &self,
        frame: &Frame,
        ieee80211: &Ieee80211,
        dest_hwsim_addr: &MacAddress,
    ) -> WifiResult<Vec<u8>> {
        // Encrypt Ieee80211 if needed
        let attrs = &frame.attrs;
        let mut ieee80211_response = match self.ap_simulation
            && ieee80211.is_to_ap()
            && ieee80211.get_bssid() == Some(self.hostapd.get_bssid())
        {
            true => ieee80211
                .into_from_ap()
                .map_err(|e| WifiError::Frame(format!("{e}")))?
                .try_into()
                .map_err(|e| WifiError::Frame(format!("{e}")))?,
            false => ieee80211.clone(),
        };
        if let Some(encrypted_ieee80211) = self.hostapd.try_encrypt(&ieee80211_response) {
            ieee80211_response = encrypted_ieee80211;
        }
        let frame_bytes = ieee80211_response.encode_to_vec()?;

        let mut builder = HwsimAttrSet::builder();

        // Attributes required by mac80211_hwsim.
        builder.receiver(&dest_hwsim_addr.to_vec());
        builder.frame(&frame_bytes);
        // Incoming HwsimMsg don't have rx_rate and signal.
        builder.rx_rate(attrs.rx_rate_idx.unwrap_or(RX_RATE));
        builder.signal(attrs.signal.unwrap_or(SIGNAL));

        attrs.flags.map(|v| builder.flags(v));
        attrs.freq.map(|v| builder.freq(v));
        attrs.tx_info.as_ref().map(|v| builder.tx_info(v));
        attrs.tx_info_flags.as_ref().map(|v| builder.tx_info_flags(v));

        Ok(builder.build()?.attributes)
    }

    // Simulates transmission through hostapd.
    fn create_hwsim_msg(
        &self,
        frame: &Frame,
        ieee80211: &Ieee80211,
        dest_hwsim_addr: &MacAddress,
    ) -> WifiResult<HwsimMsg> {
        let hwsim_msg = &frame.hwsim_msg;
        assert_eq!(hwsim_msg.hwsim_hdr.hwsim_cmd, HwsimCmd::Frame);
        let attributes = self
            .create_hwsim_attr(frame, ieee80211, dest_hwsim_addr)
            .map_err(|e| WifiError::Frame(format!("Failed to create from_ap attributes. {e}")))?;

        let nlmsg_len = hwsim_msg.nl_hdr.nlmsg_len + attributes.len() as u32
            - hwsim_msg.attributes.len() as u32;
        let new_hwsim_msg = HwsimMsg {
            nl_hdr: NlMsgHdr {
                nlmsg_len,
                nlmsg_type: NLMSG_MIN_TYPE,
                nlmsg_flags: hwsim_msg.nl_hdr.nlmsg_flags,
                nlmsg_seq: 0,
                nlmsg_pid: 0,
            },
            hwsim_hdr: hwsim_msg.hwsim_hdr.clone(),
            attributes,
        };
        Ok(new_hwsim_msg)
    }
}

fn log_hwsim_msg(frame: &Frame, client_id: u32, dest_client_id: u32) {
    debug!(
        "Sent hwsim_msg from client {} to {}. flags {:?}, ieee80211 {}",
        client_id, dest_client_id, frame.flags, frame.ieee80211,
    );
}

/// Build TxInfoFrame HwsimMsg from CmdFrame HwsimMsg.
///
/// Reference to ackLocalFrame() in external/qemu/android-qemu2-glue/emulation/VirtioWifiForwarder.cpp
fn build_tx_info(hwsim_msg: &HwsimMsg) -> WifiResult<HwsimMsg> {
    let attrs = HwsimAttrSet::parse(&hwsim_msg.attributes)?;

    let hwsim_hdr = &hwsim_msg.hwsim_hdr;
    let nl_hdr = &hwsim_msg.nl_hdr;
    let mut new_attr_builder = HwsimAttrSet::builder();
    const HWSIM_TX_STAT_ACK: u32 = 1 << 2;

    new_attr_builder
        .transmitter(
            &attrs
                .transmitter
                .ok_or(WifiError::Frame("Missing transmitter in HwsimAttrSet".into()))?
                .into(),
        )
        .flags(
            attrs.flags.ok_or(WifiError::Frame("Missing flags in HwsimAttrSet".into()))?
                | HWSIM_TX_STAT_ACK,
        )
        .cookie(attrs.cookie.ok_or(WifiError::Frame("Missing cookie in HwsimAttrSet".into()))?)
        .signal(attrs.signal.unwrap_or(SIGNAL))
        .tx_info(
            attrs
                .tx_info
                .ok_or(WifiError::Frame("Missing tx_info in HwsimAttrSet".into()))?
                .as_slice(),
        );

    let new_attr = new_attr_builder.build()?;
    let nlmsg_len =
        nl_hdr.nlmsg_len + new_attr.attributes.len() as u32 - attrs.attributes.len() as u32;
    let new_hwsim_msg = HwsimMsg {
        attributes: new_attr.attributes,
        hwsim_hdr: HwsimMsgHdr {
            hwsim_cmd: HwsimCmd::TxInfoFrame,
            hwsim_version: 0,
            reserved: hwsim_hdr.reserved,
        },
        nl_hdr: NlMsgHdr {
            nlmsg_len,
            nlmsg_type: NLMSG_MIN_TYPE,
            nlmsg_flags: nl_hdr.nlmsg_flags,
            nlmsg_seq: 0,
            nlmsg_pid: 0,
        },
    };
    Ok(new_hwsim_msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wifi::hostapd;
    use netsim_packets::ieee80211::{FrameType, Ieee80211, Ieee80211ToAp, parse_mac_address};

    #[test]
    fn test_get_plaintext_ieee80211() {
        // Test Data (802.11 frame with LLC/SNAP)
        let bssid = parse_mac_address("0:0:0:0:0:0").unwrap();
        let source = parse_mac_address("1:1:1:1:1:1").unwrap();
        let destination = parse_mac_address("2:2:2:2:2:2").unwrap();
        let ieee80211: Ieee80211 = Ieee80211ToAp {
            duration_id: 0,
            ftype: FrameType::Data,
            more_data: 0,
            more_frags: 0,
            order: 0,
            pm: 0,
            protected: 0,
            retry: 0,
            stype: 0,
            version: 0,
            bssid,
            source,
            destination,
            seq_ctrl: 0,
            payload: Vec::new(),
        }
        .try_into()
        .unwrap();

        let packet: Vec<u8> = include!("test_packets/hwsim_cmd_frame_mdns.csv");
        let hwsim_msg = HwsimMsg::decode_full(&packet).unwrap();
        let frame1 = Frame::parse(&hwsim_msg).unwrap();
        let frame2 = Frame::parse(&hwsim_msg).unwrap();

        // Case 1: plaintext_ieee80211 is None
        let processor = Processor {
            hostapd: false,
            network: false,
            wmedium: false,
            frame: frame1,
            plaintext_ieee80211: None,
        };
        assert_eq!(processor.get_ieee80211(), &processor.frame.ieee80211);
        assert_eq!(processor.get_ieee80211_bytes(), Bytes::from(processor.frame.data.clone()));

        // Case 2: plaintext_ieee80211 has a value
        let processor = Processor {
            hostapd: false,
            network: false,
            wmedium: false,
            frame: frame2,
            plaintext_ieee80211: Some(ieee80211),
        };
        assert_eq!(processor.get_ieee80211(), processor.plaintext_ieee80211.as_ref().unwrap());
        assert_eq!(
            processor.get_ieee80211_bytes(),
            Bytes::from(processor.plaintext_ieee80211.as_ref().unwrap().encode_to_vec().unwrap())
        );
    }

    #[tokio::test]
    async fn test_remove() {
        let test_client_id: u32 = 1234;
        let other_client_id: u32 = 5678;
        let addr: MacAddress = parse_mac_address("00:0b:85:71:20:00").unwrap();
        let other_addr: MacAddress = parse_mac_address("00:0b:85:71:20:01").unwrap();
        let hwsim_addr: MacAddress = parse_mac_address("00:0b:85:71:20:ce").unwrap();
        let other_hwsim_addr: MacAddress = parse_mac_address("00:0b:85:71:20:cf").unwrap();

        let hostapd_options = netsim_proto::config::HostapdOptions::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let hostapd_result = hostapd::hostapd_run(hostapd_options, tx, None).await;
        let hostapd = Arc::new(hostapd_result.expect("hostapd_run failed"));

        // Create a test Medium object
        let callback: HwsimCmdCallback = |_, _| {};
        let wifi_stats = WifiStats::default();
        let medium = Medium {
            callback,
            stations: RwLock::new(HashMap::from([
                (
                    addr,
                    Arc::new(Station {
                        client_id: test_client_id,
                        addr,
                        hwsim_addr,
                        freq: Arc::new(AtomicU32::new(0)),
                    }),
                ),
                (
                    other_addr,
                    Arc::new(Station {
                        client_id: other_client_id,
                        addr: other_addr,
                        hwsim_addr: other_hwsim_addr,
                        freq: Arc::new(AtomicU32::new(0)),
                    }),
                ),
            ])),
            clients: RwLock::new(HashMap::from([
                (test_client_id, Client::new()),
                (other_client_id, Client::new()),
            ])),
            ap_simulation: true,
            hostapd,
            wifi_stats,
            debug: Arc::new(DebugArgs::default()),
        };

        medium.remove(test_client_id);

        assert!(!medium.contains_station(&addr));
        assert!(medium.contains_station(&other_addr));
        assert!(!medium.contains_client(test_client_id));
        assert!(medium.contains_client(other_client_id));
    }

    #[test]
    fn test_is_mdns_packet() {
        let packet: Vec<u8> = include!("test_packets/hwsim_cmd_frame_mdns.csv");
        let hwsim_msg = HwsimMsg::decode_full(&packet).unwrap();
        let mdns_frame_result = Frame::parse(&hwsim_msg);
        assert!(mdns_frame_result.is_ok());
        let mdns_frame = mdns_frame_result.unwrap();
        assert!(!mdns_frame.ieee80211.get_source().is_multicast());
        assert!(mdns_frame.ieee80211.get_destination().is_multicast());
        // Check against the constant
        assert!(mdns_frame.ieee80211.get_destination().is_mdns());
    }

    #[test]
    fn test_build_tx_info_reconstruct() -> WifiResult<()> {
        let packet: Vec<u8> = include!("test_packets/hwsim_cmd_tx_info.csv");
        let hwsim_msg = HwsimMsg::decode_full(&packet).unwrap();
        assert_eq!(hwsim_msg.hwsim_hdr().hwsim_cmd, HwsimCmd::TxInfoFrame);

        let new_hwsim_msg_result = build_tx_info(&hwsim_msg);
        assert!(new_hwsim_msg_result.is_ok());
        let new_hwsim_msg = new_hwsim_msg_result.unwrap();
        assert_eq!(hwsim_msg, new_hwsim_msg);
        Ok(())
    }

    #[test]
    fn test_build_tx_info() -> WifiResult<()> {
        let packet: Vec<u8> = include!("test_packets/hwsim_cmd_frame.csv");
        let hwsim_msg = HwsimMsg::decode_full(&packet).unwrap();
        let hwsim_msg_tx_info_result = build_tx_info(&hwsim_msg);
        assert!(hwsim_msg_tx_info_result.is_ok());
        let hwsim_msg_tx_info = hwsim_msg_tx_info_result.unwrap();
        assert_eq!(hwsim_msg_tx_info.hwsim_hdr().hwsim_cmd, HwsimCmd::TxInfoFrame);
        Ok(())
    }

    fn build_tx_info_and_compare(
        frame_bytes: &Bytes,
        tx_info_expected_bytes: &Bytes,
    ) -> WifiResult<()> {
        let frame = HwsimMsg::decode_full(frame_bytes).unwrap();
        let tx_info_result = build_tx_info(&frame);
        assert!(tx_info_result.is_ok());
        let tx_info = tx_info_result.unwrap();

        let tx_info_expected = HwsimMsg::decode_full(tx_info_expected_bytes).unwrap();

        assert_eq!(tx_info.hwsim_hdr(), tx_info_expected.hwsim_hdr());
        assert_eq!(tx_info.nl_hdr(), tx_info_expected.nl_hdr());

        let attrs_result = HwsimAttrSet::parse(tx_info.attributes());
        assert!(attrs_result.is_ok());
        let attrs = attrs_result.unwrap();
        let attrs_expected_result = HwsimAttrSet::parse(tx_info_expected.attributes());
        assert!(attrs_expected_result.is_ok());
        let attrs_expected = attrs_expected_result.unwrap();

        // NOTE: TX info is different and the counts are all zeros in the TX info packet generated by WifiService.
        // TODO: Confirm if the behavior is intended in WifiService.
        assert_eq!(attrs.transmitter, attrs_expected.transmitter);
        assert_eq!(attrs.flags, attrs_expected.flags);
        assert_eq!(attrs.cookie, attrs_expected.cookie);
        assert_eq!(attrs.signal, attrs_expected.signal);
        Ok(())
    }

    #[test]
    fn test_build_tx_info_and_compare() -> WifiResult<()> {
        let frame_bytes = Bytes::from(include!("test_packets/hwsim_cmd_frame_request.csv"));
        let tx_info_expected_bytes =
            Bytes::from(include!("test_packets/hwsim_cmd_tx_info_response.csv"));
        build_tx_info_and_compare(&frame_bytes, &tx_info_expected_bytes)?;
        Ok(())
    }

    #[test]
    fn test_build_tx_info_and_compare_mdns() -> WifiResult<()> {
        let frame_bytes = Bytes::from(include!("test_packets/hwsim_cmd_frame_request_mdns.csv"));
        let tx_info_expected_bytes =
            Bytes::from(include!("test_packets/hwsim_cmd_tx_info_response_mdns.csv"));
        build_tx_info_and_compare(&frame_bytes, &tx_info_expected_bytes)?;
        Ok(())
    }
}
