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

use std::collections::HashSet;

use bytes::Bytes;
use netsim_packets::{FrameDirection, HwsimFrame, HwsimMsg, Ieee80211};
use tracing::debug;

const MAC_MDNS_IPV4: [u8; 6] = [0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB];
const MAC_MDNS_IPV6: [u8; 6] = [0x33, 0x33, 0x00, 0x00, 0x00, 0xFB];

fn is_m2u_candidate(dest_mac: &netsim_packets::MacAddress) -> bool {
    dest_mac.bytes == MAC_MDNS_IPV4 || dest_mac.bytes == MAC_MDNS_IPV6
}

use crate::{
    error::WifiError,
    medium::{
        core::Medium,
        types::{Station, WifiResult},
        utils::{self, build_tx_info},
    },
    stats::WifiApi,
};

// Packets flowing from Medium to Guest (Destination)
impl Medium {
    /// Encodes a HwsimMsg and pushes it to the output queue.
    ///
    /// This helper simplifies the error handling and queue management for
    /// outgoing packets.
    fn push_packet(
        &mut self,
        client_id: u32,
        msg: &HwsimMsg,
        out_queue: &mut Vec<(u32, Bytes)>,
    ) -> WifiResult<()> {
        let packet = msg.encode_to_vec().map_err(|e| WifiError::Internal(Box::from(e)))?.into();
        out_queue.push((client_id, packet));
        Ok(())
    }

    /// Resolves targets for a given destination address.
    ///
    /// - If unicast and known: returns the specific station.
    /// - If multicast: returns all subscribed stations.
    /// - If unknown: returns empty list.
    fn resolve_targets(&self, dest_addr: &netsim_packets::MacAddress) -> Vec<Station> {
        let mut targets = Vec::new();
        if self.contains_station(dest_addr) {
            if let Ok(station) = self.get_station(dest_addr) {
                targets.push(station.clone());
            }
        } else if dest_addr.is_multicast() {
            let mut visited = HashSet::new();
            for station in self.stations.values() {
                if visited.insert((station.hwsim_addr, station.freq)) {
                    targets.push(station.clone());
                }
            }
        } else {
            // Fallback: Android DHCP frames systematically preserve hwsimaddr inside
            // payloads. Slirp returns these dynamically forcing Destination MAC
            // outside standard Random MAC arrays.
            for station in self.stations.values() {
                if &station.hwsim_addr == dest_addr {
                    targets.push(station.clone());
                    break;
                }
            }
        }
        targets
    }

    pub fn ack_frame(
        &mut self,
        client_id: u32,
        frame: &HwsimFrame,
        acked: bool,
        out_queue: &mut Vec<(u32, Bytes)>,
    ) -> WifiResult<()> {
        if !acked
            && let Some(client) = self.clients.get_mut(&client_id)
            && client.in_p2p_action
        {
            client.in_p2p_listen = true;
            client.in_p2p_action = false;
        }
        let tx_info = build_tx_info(&frame.hwsim_msg, acked)?;
        self.push_packet(client_id, &tx_info, out_queue)?;
        self.incr_tx(client_id)
    }

    /// Processes an IEEE 802.3 packet (Ethernet frame), typically from Slirp or
    /// a Tun interface.
    ///
    /// Converts the Ethernet frame to an IEEE 802.11 frame and routes it.
    pub fn process_ieee8023_response(
        &mut self,
        packet: &Bytes,
        out_queue: &mut Vec<(u32, Bytes)>,
    ) -> WifiResult<()> {
        if self.debug.debug_no_traffic || self.debug.debug_no_network {
            return Ok(());
        }
        // Try to determine the destination MacAddress from the Ethernet header
        let Some(dest_mac_bytes) = packet.first_chunk::<6>() else {
            return Err(WifiError::Internal(Box::from(
                "Failed to parse Ethernet Destination MAC: packet too short",
            )));
        };
        let dest_mac = netsim_packets::MacAddress::new(*dest_mac_bytes);

        if dest_mac.is_broadcast() || dest_mac.is_multicast() {
            let bssids = self.key_store.bssids.read().clone();
            for bssid in bssids {
                let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if let Ok(ieee80211) = netsim_packets::Ieee80211::from_ieee8023_qos(
                    packet,
                    bssid,
                    FrameDirection::FromAp,
                    true,
                    seq,
                ) {
                    let _ = self.route_infra_packet(ieee80211, out_queue);
                }
            }
            return Ok(());
        }

        let Some(bssid) = self.key_store.get_station_bssid(&dest_mac) else {
            tracing::warn!("Dropping unicast packet to unknown station {}", dest_mac);
            return Ok(());
        };

        let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let ieee80211 = netsim_packets::Ieee80211::from_ieee8023_qos(
            packet,
            bssid,
            FrameDirection::FromAp,
            true,
            seq,
        )
        .map_err(|e| WifiError::Internal(Box::from(e)))?;
        self.route_infra_packet(ieee80211, out_queue)
    }

    /// Entry point for transmitting raw IEEE 802.11 bytes from the
    /// infrastructure.
    ///
    /// Decodes the packet and delegates to `route_infra_packet`.
    pub fn transmit_from_infra(
        &mut self,
        packet: &Bytes,
        out_queue: &mut Vec<(u32, Bytes)>,
    ) -> WifiResult<()> {
        if self.debug.debug_no_traffic {
            return Ok(());
        }
        let ieee80211 = Ieee80211::decode_full(packet).map_err(|e| {
            WifiError::Internal(Box::from(format!("Failed to process IEEE 802.11 response: {e}")))
        })?;
        self.route_infra_packet(ieee80211, out_queue)
    }

    /// Routes a decoded IEEE 802.11 frame from the infrastructure to the
    /// appropriate stations.
    ///
    /// Handles encryption (if applicable), resolves targets, and delivers the
    /// packet.
    pub(crate) fn route_infra_packet(
        &mut self,
        ieee80211: Ieee80211,
        out_queue: &mut Vec<(u32, Bytes)>,
    ) -> WifiResult<()> {
        let dest_addr = ieee80211.get_destination();
        let mut targets = self.resolve_targets(&dest_addr);

        if targets.is_empty() && !dest_addr.is_multicast() {
            // Unknown Unicast Flooding: Deliver to all enabled stations
            for station in self.stations.values() {
                targets.push(station.clone());
            }
        }

        // Consider a frame as infra if it is coming from an AP (FromDS=1)
        // OR if it aligns with a hosted AP in key_store.
        let is_infra = ieee80211.is_from_ds()
            || ieee80211.get_bssid().is_some_and(|b| self.key_store.has_bssid(&b));

        let is_m2u_conversion = is_infra
            && (is_m2u_candidate(&dest_addr) || (!dest_addr.is_multicast() && targets.len() > 1));

        for dest in targets {
            if self.enabled(dest.client_id)? {
                let mut target_frame = ieee80211.clone();
                if is_m2u_conversion {
                    let target_mac = netsim_packets::MacAddress::new(dest.addr.into());
                    target_frame.set_destination(&target_mac);
                }

                let frame_to_send =
                    match self.prepare_frame_for_delivery(&target_frame, &ieee80211, is_infra) {
                        Some(f) => f,
                        None => continue,
                    };

                let msg = utils::create_hwsim_msg_from_frame(
                    &frame_to_send,
                    &dest.hwsim_addr,
                    dest.freq,
                    None,
                )?;
                self.wifi_stats.incr_hwsim_frames_tx();
                self.push_packet(dest.client_id, &msg, out_queue)?;
                self.incr_rx(dest.client_id)?;
            }
        }
        Ok(())
    }

    fn prepare_frame_for_delivery(
        &self,
        frame: &Ieee80211,
        original_frame: &Ieee80211,
        is_infra: bool,
    ) -> Option<Ieee80211> {
        if !is_infra {
            return Some(frame.clone());
        }

        if let Some(encrypted_bytes) = self.key_store.try_encrypt(frame) {
            match Ieee80211::decode(&encrypted_bytes) {
                Ok(decoded) => Some(decoded),
                Err(error) => {
                    tracing::error!(%error, "Failed to decode encrypted frame");
                    None
                }
            }
        } else if original_frame.get_bssid().is_some_and(|b| self.key_store.get_gtk(&b).is_some())
            && original_frame.get_destination().is_multicast()
        {
            // Secure network fallback (keys not ready)
            Some(original_frame.clone())
        } else {
            // Open network or fallback
            Some(frame.clone())
        }
    }

    pub fn transmit(
        &mut self,
        frame: &HwsimFrame,
        ieee80211: &Ieee80211,
        out_queue: &mut Vec<(u32, Bytes)>,
    ) -> WifiResult<bool> {
        self.parse_action_frame(ieee80211);
        let source_addr = ieee80211.get_source();
        let source = self.get_station(&source_addr)?.clone();
        let dest_addr = ieee80211.get_destination();
        let via_infra_ap = ieee80211.get_bssid().is_some_and(|b| self.key_store.has_bssid(&b));

        let mut targets = self.resolve_targets(&dest_addr);
        let mut flooded = false;
        if targets.is_empty() && !dest_addr.is_multicast() {
            // Flood unknown non-infra unicast (e.g. unlearned virtual P2P interface MAC) to peer radios.
            if !via_infra_ap && dest_addr.bytes != [0; 6] {
                let mut visited = HashSet::new();
                for station in self.stations.values() {
                    if station.client_id != source.client_id && visited.insert(station.hwsim_addr) {
                        targets.push(station.clone());
                    }
                }
                flooded = !targets.is_empty();
            }
            if targets.is_empty() {
                return Err(WifiError::Internal(Box::from(format!(
                    "Dropped packet from {} to {}",
                    source.addr, dest_addr
                ))));
            }
        }

        if dest_addr.is_multicast() {
            debug!("Frame multicast {ieee80211}");
            if dest_addr.is_mdns() {
                self.wifi_stats.incr_mdns_count();
            }
        } else {
            debug!("Frame deliver unicast from {} to {}", source.addr, dest_addr);
            self.wifi_stats.incr_wmedium_unicast_frames_tx();
        }

        // Consider a frame as infra if it is coming from an AP (FromDS=1)
        // OR if it aligns with a hosted AP in key_store.
        let is_infra = ieee80211.is_from_ds() || via_infra_ap;

        // RESTRICT: Only run M2U optimizations for infrastructure networks
        let is_m2u_conversion = is_infra
            && (is_m2u_candidate(&dest_addr)
                || (!dest_addr.is_multicast() && !flooded && targets.len() > 1));

        let is_p2p_payload_data = !via_infra_ap && ieee80211.is_payload_bearing_data();
        let src_enabled = self.enabled(source.client_id)?;
        let mut acked = dest_addr.is_multicast() && src_enabled;
        let targets_len = targets.len();

        for dest in targets {
            // Drop unicast packets destined to the sender itself (invalid for hwsim)
            // But ALLOW multicast packets to reach the sender (AP Reflection) if enabled
            if dest.addr == source.addr
                && (!dest_addr.is_multicast() || !self.simulate_ap_reflection)
            {
                continue;
            }
            let dst_enabled = self.enabled(dest.client_id)?;
            if src_enabled && dst_enabled {
                // 1. Apply AP Reflection first on the original broadcast frame
                let reflected_frame =
                    if self.simulate_ap_reflection && ieee80211.is_to_ap() && is_infra {
                        ieee80211
                            .clone()
                            .into_from_ap()
                            .map_err(|e: String| WifiError::Internal(Box::from(e)))?
                            .try_into()
                            .map_err(|e: String| WifiError::Internal(Box::from(e)))?
                    } else {
                        ieee80211.clone()
                    };

                // 2. Create the targeted frame (rewrite MAC if M2U)
                let mut target_frame = reflected_frame.clone();
                if is_m2u_conversion && dest.addr != source_addr {
                    let target_mac = netsim_packets::MacAddress::new(dest.addr.into());
                    target_frame.set_destination(&target_mac);
                }

                // 3. Encryption and Fallback
                let frame_to_send = match self.prepare_frame_for_delivery(
                    &target_frame,
                    &reflected_frame,
                    is_infra,
                ) {
                    Some(f) => f,
                    None => continue, // Skip on decode failure in transmit
                };

                let msg_result =
                    utils::create_hwsim_msg_with_attrs(frame, &frame_to_send, &dest.hwsim_addr);

                match msg_result {
                    Ok(msg) => {
                        self.wifi_stats.incr_wmedium_frames_tx();
                        self.wifi_stats.incr_hwsim_frames_tx();
                        self.push_packet(dest.client_id, &msg, out_queue)?;
                        self.incr_rx(dest.client_id)?;
                        if (!flooded || targets_len == 1)
                            && self
                                .clients
                                .get(&dest.client_id)
                                .is_some_and(|c| c.is_reachable_on_freq(frame.attrs.freq))
                        {
                            acked = true;
                        }

                        if is_p2p_payload_data
                            && !flooded
                            && source.client_id != dest.client_id
                            && !dest_addr.is_multicast()
                        {
                            self.incr_p2p_tx(source.client_id);
                            self.incr_p2p_rx(dest.client_id);
                        }
                    }
                    Err(e) => self.wifi_stats.log_and_incr_err_count(&e),
                }
            }
        }
        Ok(acked)
    }

    pub(crate) fn parse_action_frame(&mut self, ieee80211: &Ieee80211) {
        use netsim_packets::{
            ActionHeader, VendorSpecificPublicActionHeader, category, dpp, management_subtype, nan,
            p2p, public_action,
        };
        use zerocopy::FromBytes;

        if ieee80211.is_mgmt() {
            if ieee80211.stype() == management_subtype::ACTION {
                let payload = ieee80211.get_payload();
                #[allow(clippy::collapsible_if)]
                if let Ok((action_hdr, rest_bytes)) = ActionHeader::ref_from_prefix(&payload) {
                    if action_hdr.category == category::PUBLIC {
                        match action_hdr.action {
                            public_action::FTM_REQUEST => {
                                self.wifi_stats.incr_wifi_api(WifiApi::WifiRttManagerStartRanging);
                            }
                            public_action::FINE_TIMING_MEASUREMENT => {
                                self.wifi_stats
                                    .incr_wifi_api(WifiApi::WifiRttManagerOnRangingResults);
                            }
                            public_action::GAS_INITIAL_REQUEST
                            | public_action::GAS_INITIAL_RESPONSE => {
                                self.wifi_stats
                                    .incr_wifi_api(WifiApi::WifiP2pManagerDiscoverServices);
                            }
                            public_action::VENDOR_SPECIFIC => {
                                if let Ok((vs_hdr, sharing_payload)) =
                                    VendorSpecificPublicActionHeader::ref_from_prefix(rest_bytes)
                                {
                                    match (vs_hdr.oui, vs_hdr.oui_type) {
                                        (nan::OUI, nan::OUI_TYPE) => {
                                            self.parse_nan_action(sharing_payload)
                                        }
                                        (p2p::OUI, p2p::OUI_TYPE) => {
                                            self.parse_p2p_action(sharing_payload)
                                        }
                                        (dpp::OUI, dpp::OUI_TYPE) => {
                                            self.parse_dpp_action(sharing_payload)
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            } else {
                let stype = ieee80211.stype();
                if (stype == management_subtype::PROBE_REQUEST
                    || stype == management_subtype::PROBE_RESPONSE)
                    && self.has_p2p_ie(ieee80211)
                {
                    if stype == management_subtype::PROBE_REQUEST {
                        let source_addr = ieee80211.get_source();
                        self.active_p2p_groups.remove(&source_addr);
                        if let Some(listen_freq) = self.p2p_listen_freq(ieee80211)
                            && let Some(client_id) = self.get_station_chip_id(&source_addr)
                            && let Some(client) = self.clients.get_mut(&client_id)
                        {
                            client.listen_freq = Some(listen_freq);
                        }
                    }
                    self.wifi_stats.incr_wifi_api(WifiApi::WifiP2pManagerDiscoverPeers);
                }
                if (stype == management_subtype::BEACON
                    || stype == management_subtype::PROBE_RESPONSE)
                    && self.is_p2p_go(ieee80211)
                {
                    let source_addr = ieee80211.get_source();
                    if let Some(client_id) = self.get_station_chip_id(&source_addr)
                        && let Some(client) = self.clients.get_mut(&client_id)
                    {
                        client.listen_freq = None;
                        client.in_p2p_listen = false;
                        client.in_p2p_action = false;
                    }
                    if self.active_p2p_groups.insert(source_addr) {
                        self.wifi_stats.incr_wifi_api(WifiApi::WifiP2pManagerCreateGroup);
                    }
                }
            }
        }
    }

    fn p2p_listen_freq(&self, ieee80211: &Ieee80211) -> Option<u32> {
        use netsim_packets::{IeIterator, p2p, tags};

        for ie in IeIterator::new(&ieee80211.get_payload()) {
            if ie.id == tags::VENDOR_SPECIFIC
                && let Some((&p2p::OUI, &[p2p::OUI_TYPE, ref rest @ ..])) =
                    ie.body.split_first_chunk::<3>()
            {
                let mut attr_bytes = rest;
                while let &[attr_id, len_l, len_h, ref remaining @ ..] = attr_bytes {
                    let attr_len = u16::from_le_bytes([len_l, len_h]) as usize;
                    let Some((attr_val, next)) = remaining.split_at_checked(attr_len) else {
                        break;
                    };
                    // P2P_ATTR_LISTEN_CHANNEL (ID 6): Country(3) + RegClass(1) + Channel(1)
                    const P2P_ATTR_LISTEN_CHANNEL: u8 = 6;
                    if attr_id == P2P_ATTR_LISTEN_CHANNEL
                        && let &[_, _, _, _, ch @ 1..=13, ..] = attr_val
                    {
                        return Some(2407 + 5 * u32::from(ch));
                    }
                    attr_bytes = next;
                }
            }
        }
        None
    }

    pub(crate) fn has_p2p_ie(&self, ieee80211: &Ieee80211) -> bool {
        use netsim_packets::{IeIterator, management_subtype, p2p, tags};

        let payload = ieee80211.get_payload();
        let offset = match ieee80211.stype() {
            management_subtype::PROBE_REQUEST => 0,
            management_subtype::PROBE_RESPONSE => 12,
            _ => return false,
        };

        let Some(ies) = payload.get(offset..) else {
            return false;
        };

        for ie in IeIterator::new(ies) {
            if ie.id == tags::VENDOR_SPECIFIC
                && let Some((&p2p::OUI, &[p2p::OUI_TYPE, ..])) = ie.body.split_first_chunk::<3>()
            {
                return true;
            }
        }
        false
    }

    fn is_p2p_go(&self, ieee80211: &Ieee80211) -> bool {
        use netsim_packets::{IeIterator, management_subtype, p2p, tags};

        let payload = ieee80211.get_payload();
        let offset = match ieee80211.stype() {
            management_subtype::BEACON => 12,
            management_subtype::PROBE_RESPONSE => 12,
            _ => return false,
        };

        let Some(ies) = payload.get(offset..) else {
            return false;
        };

        for ie in IeIterator::new(ies) {
            if ie.id == tags::VENDOR_SPECIFIC
                && let Some((&p2p::OUI, &[p2p::OUI_TYPE, ref rest @ ..])) =
                    ie.body.split_first_chunk::<3>()
            {
                let mut attr_bytes = rest;
                while let &[attr_id, len_l, len_h, ref remaining @ ..] = attr_bytes {
                    let attr_len = u16::from_le_bytes([len_l, len_h]) as usize;
                    let Some((attr_val, next)) = remaining.split_at_checked(attr_len) else {
                        break;
                    };
                    if attr_id == 2
                        && attr_val.get(1).is_some_and(|&group_cap| group_cap & 0x01 != 0)
                    {
                        return true;
                    }
                    attr_bytes = next;
                }
            }
        }
        false
    }

    fn parse_nan_action(&mut self, sharing_payload: &[u8]) {
        use netsim_packets::{NanAttributeIterator, NanNdpHeader, NanSdaHeader, nan};
        use zerocopy::FromBytes;

        for attr in NanAttributeIterator::new(sharing_payload) {
            match attr.id {
                nan::attr::SDA => {
                    if let Ok((sda_hdr, _)) = NanSdaHeader::ref_from_prefix(attr.val) {
                        let service_type = sda_hdr.control & 0x03;
                        match service_type {
                            nan::service_type::PUBLISH => {
                                let publish_type = (sda_hdr.control >> 2) & 0x01;
                                if publish_type == 0 {
                                    self.wifi_stats
                                        .incr_wifi_api(WifiApi::WifiAwareSessionPublishUnsolicited);
                                } else {
                                    self.wifi_stats
                                        .incr_wifi_api(WifiApi::WifiAwareSessionPublishSolicited);
                                }
                            }
                            nan::service_type::SUBSCRIBE => {
                                let subscribe_type = (sda_hdr.control >> 2) & 0x01;
                                if subscribe_type == 1 {
                                    self.wifi_stats
                                        .incr_wifi_api(WifiApi::WifiAwareSessionSubscribeActive);
                                } else {
                                    self.wifi_stats
                                        .incr_wifi_api(WifiApi::WifiAwareSessionSubscribePassive);
                                }
                            }
                            nan::service_type::FOLLOW_UP => {
                                self.wifi_stats
                                    .incr_wifi_api(WifiApi::WifiAwareDiscoverySessionSendMessage);
                            }
                            _ => {}
                        }
                    }
                }
                nan::attr::NDP => {
                    if let Ok((ndp_hdr, _)) = NanNdpHeader::ref_from_prefix(attr.val) {
                        let ndp_type = ndp_hdr.type_and_status & 0x0F;
                        match ndp_type {
                            nan::ndp_type::REQUEST => {
                                self.wifi_stats.incr_wifi_api(WifiApi::WifiAwareNdpRequest);
                            }
                            nan::ndp_type::RESPONSE => {
                                self.wifi_stats.incr_wifi_api(WifiApi::WifiAwareNdpResponse);
                            }
                            nan::ndp_type::CONFIRM => {
                                self.wifi_stats.incr_wifi_api(WifiApi::WifiAwareNdpConfirm);
                            }
                            nan::ndp_type::TERMINATE => {
                                self.wifi_stats.incr_wifi_api(WifiApi::WifiAwareNdpTerminate);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn parse_p2p_action(&mut self, sharing_payload: &[u8]) {
        use netsim_packets::p2p;

        if let Some(&p2p_subtype) = sharing_payload.first() {
            match p2p_subtype {
                p2p::action_type::GO_NEG_REQ
                | p2p::action_type::GO_NEG_RESP
                | p2p::action_type::GO_NEG_CONF => {
                    self.wifi_stats.incr_wifi_api(WifiApi::WifiP2pManagerConnect);
                }
                p2p::action_type::INVITATION_REQ | p2p::action_type::INVITATION_RESP => {
                    self.wifi_stats.incr_wifi_api(WifiApi::WifiP2pManagerConnect);
                }
                p2p::action_type::PROV_DISC_REQ | p2p::action_type::PROV_DISC_RESP => {
                    self.wifi_stats.incr_wifi_api(WifiApi::WifiP2pManagerConnect);
                }
                _ => {}
            }
        }
    }

    fn parse_dpp_action(&mut self, sharing_payload: &[u8]) {
        use netsim_packets::dpp;

        if let Some(&dpp_subtype) = sharing_payload.first() {
            match dpp_subtype {
                dpp::action_type::AUTH_REQ => {
                    self.wifi_stats.incr_wifi_api(WifiApi::WifiEasyConnectAuthRequest);
                }
                dpp::action_type::AUTH_RESP => {
                    self.wifi_stats.incr_wifi_api(WifiApi::WifiEasyConnectAuthResponse);
                }
                dpp::action_type::AUTH_CONF => {
                    self.wifi_stats.incr_wifi_api(WifiApi::WifiEasyConnectAuthConfirm);
                }
                _ => {}
            }
        }
    }
}
