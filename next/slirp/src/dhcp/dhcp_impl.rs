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

//! DHCP protocol implementation.

use std::{
    net::Ipv4Addr,
    time::{Duration, Instant},
};

use bytes::Bytes;
use log::{info, warn};
use netsim_packets::{EthernetFrame, Ipv4Builder, MacAddr, UdpPacketBuilder};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Ref, Unaligned};

use crate::{Config, SlirpResponse, clock::Clock, dns::create_dns_search_option};

pub(crate) const DHCP_MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
const LEASE_TIME: Duration = Duration::from_secs(24 * 3600);
const MAX_LEASES: usize = 16;

#[derive(FromBytes, KnownLayout, Unaligned, Immutable, IntoBytes)]
#[repr(C)]
pub(crate) struct DhcpPacket {
    pub op: u8,
    pub htype: u8,
    pub hlen: u8,
    pub hops: u8,
    pub xid: [u8; 4],
    pub secs: [u8; 2],
    pub flags: [u8; 2],
    pub ciaddr: [u8; 4],
    pub yiaddr: [u8; 4],
    pub siaddr: [u8; 4],
    pub giaddr: [u8; 4],
    pub chaddr: [u8; 16],
    pub sname: [u8; 64],
    pub file: [u8; 128],
    pub magic_cookie: [u8; 4],
    pub options: [u8; 308],
}

pub(crate) type DhcpPacketRef<'a> = Ref<&'a [u8], DhcpPacket>;

impl DhcpPacket {
    pub(crate) fn parse(bytes: &[u8]) -> Option<(DhcpPacketRef<'_>, &[u8])> {
        Ref::from_prefix(bytes).ok()
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub(crate) enum DhcpMessageType {
    Discover = 1,
    Offer = 2,
    Request = 3,
    Ack = 5,
    Nak = 6,
    Release = 7,
    Inform = 8,
}

#[derive(Clone)]
pub(super) struct DhcpLease {
    mac_addr: MacAddr,
    pub(super) ip_addr: Ipv4Addr,
    expires_at: Instant,
}

impl Serialize for DhcpLease {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let remaining = self.expires_at.saturating_duration_since(Instant::now()).as_secs();
        (self.mac_addr, self.ip_addr, remaining).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DhcpLease {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let (mac_addr, ip_addr, remaining_secs): (MacAddr, Ipv4Addr, u64) =
            Deserialize::deserialize(deserializer)?;
        let expires_at = Instant::now() + Duration::from_secs(remaining_secs);
        Ok(Self { mac_addr, ip_addr, expires_at })
    }
}

#[derive(Serialize, Deserialize)]
pub struct DhcpManager {
    pub(super) leases: Vec<DhcpLease>,
}

impl Default for DhcpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DhcpManager {
    pub fn new() -> Self {
        Self { leases: Vec::with_capacity(MAX_LEASES) }
    }

    pub fn handle_packet(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        clock: &dyn Clock,
        packet: &[u8],
    ) {
        if let Some((dhcp_packet, _)) = DhcpPacket::parse(packet) {
            if dhcp_packet.magic_cookie != DHCP_MAGIC_COOKIE {
                return;
            }
            if let Some(message_type) = Self::get_message_type(&dhcp_packet.options) {
                match message_type {
                    DhcpMessageType::Discover => {
                        self.handle_discover(responses, config, clock, &dhcp_packet);
                    }
                    DhcpMessageType::Request => {
                        self.handle_request(responses, config, clock, &dhcp_packet);
                    }
                    DhcpMessageType::Release => {
                        self.handle_release(&dhcp_packet);
                    }
                    DhcpMessageType::Inform => {
                        self.handle_inform(responses, config, &dhcp_packet);
                    }
                    _ => {}
                }
            } else if dhcp_packet.op == 1 {
                // Legacy BOOTP request! (op == 1 (BootRequest), but no DHCP
                // Message Type option!)
                self.handle_bootp_request(responses, config, clock, &dhcp_packet);
            }
        }
    }

    fn get_mac_addr(packet: &DhcpPacket) -> MacAddr {
        let [a, b, c, d, e, f, ..] = packet.chaddr;
        MacAddr::from([a, b, c, d, e, f])
    }

    pub(super) fn find_lease_by_mac(&self, mac: &MacAddr, clock: &dyn Clock) -> Option<&DhcpLease> {
        self.leases.iter().find(|l| l.mac_addr == *mac && l.expires_at > clock.now())
    }

    fn find_available_ip(&self, config: &Config, clock: &dyn Clock) -> Option<Ipv4Addr> {
        let start_addr = u32::from_be_bytes(config.guest_ipv4.octets());
        for i in 0..MAX_LEASES {
            let ip_addr = Ipv4Addr::from(start_addr + i as u32);
            if !self.leases.iter().any(|l| l.ip_addr == ip_addr && l.expires_at > clock.now()) {
                return Some(ip_addr);
            }
        }
        None
    }

    fn is_ip_in_pool(&self, config: &Config, ip: Ipv4Addr) -> bool {
        let start_addr = u32::from_be_bytes(config.guest_ipv4.octets());
        let ip_addr = u32::from_be_bytes(ip.octets());
        ip_addr >= start_addr && ip_addr < start_addr + MAX_LEASES as u32
    }

    fn handle_discover(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        clock: &dyn Clock,
        discover_packet: &DhcpPacket,
    ) {
        let mac_addr = Self::get_mac_addr(discover_packet);
        let ip_to_offer = match self.find_lease_by_mac(&mac_addr, clock) {
            Some(lease) => lease.ip_addr,
            None => match self.find_available_ip(config, clock) {
                Some(ip) => ip,
                None => return, // No IPs available
            },
        };
        self.send_offer(responses, config, discover_packet, ip_to_offer);
    }

    fn handle_request(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        clock: &dyn Clock,
        request_packet: &DhcpPacket,
    ) {
        let mac_addr = Self::get_mac_addr(request_packet);
        let requested_ip =
            Ipv4Addr::from(u32::from_be_bytes(Self::get_requested_ip(&request_packet.options)));

        if !self.is_ip_in_pool(config, requested_ip) {
            self.send_nak(responses, config, request_packet);
            return;
        }

        // Find an existing valid lease for this MAC
        if let Some(lease) = self.find_lease_by_mac(&mac_addr, clock) {
            // Client is requesting the IP it already has, this is a renewal.
            if lease.ip_addr == requested_ip {
                self.send_ack(responses, config, request_packet, lease.ip_addr);
            } else {
                // Client is requesting a different IP than it was assigned.
                self.send_nak(responses, config, request_packet);
            }
        } else if self.leases.len() < MAX_LEASES {
            // This is a new lease request.
            let lease =
                DhcpLease { mac_addr, ip_addr: requested_ip, expires_at: clock.now() + LEASE_TIME };
            self.leases.push(lease);
            self.send_ack(responses, config, request_packet, requested_ip);
        } else {
            // No lease for this MAC, and no new leases available.
            self.send_nak(responses, config, request_packet);
        }
    }

    fn handle_inform(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        request_packet: &DhcpPacket,
    ) {
        let mut ack_packet = DhcpPacket {
            op: 2, // BootReply
            htype: 1,
            hlen: 6,
            hops: 0,
            xid: request_packet.xid,
            secs: [0; 2],
            flags: [0; 2],
            ciaddr: request_packet.ciaddr, // client IP
            yiaddr: [0; 4],                // yiaddr must be 0 for INFORM
            siaddr: config.host_ipv4.octets(),
            giaddr: [0; 4],
            chaddr: request_packet.chaddr,
            sname: [0; 64],
            file: [0; 128],
            magic_cookie: DHCP_MAGIC_COOKIE,
            options: [0; 308],
        };

        let mut options = Vec::new();
        options.extend_from_slice(&[53, 1, DhcpMessageType::Ack as u8]);
        options.extend_from_slice(&[1, 4, 255, 255, 255, 0]); // Subnet mask
        options.extend_from_slice(&[3, 4]);
        options.extend_from_slice(&config.host_ipv4.octets());
        options.extend_from_slice(&[6, 4]);
        options.extend_from_slice(&config.host_ipv4.octets());
        if let Some(domain_name) = &config.domain_name {
            options.extend_from_slice(&[15, domain_name.len() as u8]);
            options.extend_from_slice(domain_name.as_bytes());
        }
        if let Some(dns_search_option) = create_dns_search_option(config) {
            options.extend_from_slice(&dns_search_option);
        }
        options.push(255); // End option

        let Some(dest) = ack_packet.options.get_mut(..options.len()) else {
            return;
        };
        dest.copy_from_slice(&options);
        self.send_packet(responses, config, &ack_packet);
    }

    fn handle_release(&mut self, request_packet: &DhcpPacket) {
        let mac_addr = Self::get_mac_addr(request_packet);
        let client_ip = Ipv4Addr::from(u32::from_be_bytes(request_packet.ciaddr));

        let initial_len = self.leases.len();
        self.leases.retain(|l| !(l.mac_addr == mac_addr && l.ip_addr == client_ip));

        if self.leases.len() < initial_len {
            info!("DHCP: Released lease for MAC {mac_addr} and IP {client_ip}");
        } else {
            warn!(
                "DHCP: Received RELEASE for MAC {mac_addr} and IP {client_ip}, but no lease was found"
            );
        }
    }

    fn handle_bootp_request(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        clock: &dyn Clock,
        bootp_packet: &DhcpPacket,
    ) {
        let mac_addr = Self::get_mac_addr(bootp_packet);
        let ip_to_assign = match self.find_lease_by_mac(&mac_addr, clock) {
            Some(lease) => lease.ip_addr,
            None => match self.find_available_ip(config, clock) {
                Some(ip) => {
                    let lease =
                        DhcpLease { mac_addr, ip_addr: ip, expires_at: clock.now() + LEASE_TIME };
                    self.leases.push(lease);
                    ip
                }
                None => return, // No IPs available
            },
        };

        info!("BOOTP: Handling legacy request from MAC {mac_addr}, assigning IP {ip_to_assign}");

        let mut reply_packet = DhcpPacket {
            op: 2, // BootReply
            htype: 1,
            hlen: 6,
            hops: 0,
            xid: bootp_packet.xid,
            secs: [0; 2],
            flags: bootp_packet.flags,
            ciaddr: [0; 4],
            yiaddr: ip_to_assign.octets(),
            siaddr: config.host_ipv4.octets(),
            giaddr: [0; 4],
            chaddr: bootp_packet.chaddr,
            sname: [0; 64],
            file: [0; 128],
            magic_cookie: DHCP_MAGIC_COOKIE,
            options: [0; 308],
        };

        let mut options = Vec::new();
        options.extend_from_slice(&[1, 4, 255, 255, 255, 0]); // Subnet mask
        options.extend_from_slice(&[3, 4]);
        options.extend_from_slice(&config.host_ipv4.octets());
        options.extend_from_slice(&[6, 4]);
        options.extend_from_slice(&config.host_ipv4.octets());
        if let Some(tftp_server_name) = &config.tftp_server_name {
            options.extend_from_slice(&[66, tftp_server_name.len() as u8]);
            options.extend_from_slice(tftp_server_name.as_bytes());
        }
        if let Some(boot_file) = &config.boot_file {
            options.extend_from_slice(&[67, boot_file.len() as u8]);
            options.extend_from_slice(boot_file.as_bytes());
        }
        options.push(255); // End option

        let Some(dest) = reply_packet.options.get_mut(..options.len()) else {
            return;
        };
        dest.copy_from_slice(&options);

        // Copy boot file and tftp server name to legacy header fields
        if let Some(boot_file) = &config.boot_file {
            let max_len = reply_packet.file.len().saturating_sub(1);
            for (dst, &src) in reply_packet.file.iter_mut().take(max_len).zip(boot_file.as_bytes())
            {
                *dst = src;
            }
        }
        if let Some(sname) = &config.tftp_server_name {
            let max_len = reply_packet.sname.len().saturating_sub(1);
            for (dst, &src) in reply_packet.sname.iter_mut().take(max_len).zip(sname.as_bytes()) {
                *dst = src;
            }
        }

        self.send_packet(responses, config, &reply_packet);
    }

    fn find_dhcp_option(options: &[u8], code: u8) -> Option<&[u8]> {
        let mut rem = options;
        while let Some((&opt_code, rest)) = rem.split_first() {
            if opt_code == 255 {
                break;
            }
            if opt_code == 0 {
                rem = rest;
                continue;
            }
            let (&opt_len, payload) = rest.split_first()?;
            let opt_len = opt_len as usize;
            let (opt_val, next_rem) = payload.split_at_checked(opt_len)?;
            if opt_code == code {
                return Some(opt_val);
            }
            rem = next_rem;
        }
        None
    }

    fn get_message_type(options: &[u8]) -> Option<DhcpMessageType> {
        let opt = Self::find_dhcp_option(options, 53)?;
        match *opt {
            [1] => Some(DhcpMessageType::Discover),
            [2] => Some(DhcpMessageType::Offer),
            [3] => Some(DhcpMessageType::Request),
            [5] => Some(DhcpMessageType::Ack),
            [6] => Some(DhcpMessageType::Nak),
            [7] => Some(DhcpMessageType::Release),
            [8] => Some(DhcpMessageType::Inform),
            _ => None,
        }
    }

    fn get_requested_ip(options: &[u8]) -> [u8; 4] {
        if let Some(&[a, b, c, d]) = Self::find_dhcp_option(options, 50) {
            return [a, b, c, d];
        }
        [0; 4]
    }

    fn send_reply_packet(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        xid: [u8; 4],
        chaddr: [u8; 16],
        yiaddr: Ipv4Addr,
        msg_type: DhcpMessageType,
    ) {
        let mut reply_packet = DhcpPacket {
            op: 2, // BootReply
            htype: 1,
            hlen: 6,
            hops: 0,
            xid,
            secs: [0; 2],
            flags: [0; 2],
            ciaddr: [0; 4],
            yiaddr: yiaddr.octets(),
            siaddr: config.host_ipv4.octets(),
            giaddr: [0; 4],
            chaddr,
            sname: [0; 64],
            file: [0; 128],
            magic_cookie: DHCP_MAGIC_COOKIE,
            options: [0; 308],
        };

        let mut options = Vec::new();
        options.extend_from_slice(&[53, 1, msg_type as u8]);
        options.extend_from_slice(&[1, 4, 255, 255, 255, 0]); // Subnet mask
        options.extend_from_slice(&[3, 4]);
        options.extend_from_slice(&config.host_ipv4.octets());
        options.extend_from_slice(&[6, 4]);
        options.extend_from_slice(&config.host_ipv4.octets());
        if let Some(domain_name) = &config.domain_name {
            options.extend_from_slice(&[15, domain_name.len() as u8]);
            options.extend_from_slice(domain_name.as_bytes());
        }
        if let Some(dns_search_option) = create_dns_search_option(config) {
            options.extend_from_slice(&dns_search_option);
        }
        if let Some(hostname) = &config.client_hostname {
            options.extend_from_slice(&[12, hostname.len() as u8]);
            options.extend_from_slice(hostname.as_bytes());
        }
        if let Some(tftp_server_name) = &config.tftp_server_name {
            options.extend_from_slice(&[66, tftp_server_name.len() as u8]);
            options.extend_from_slice(tftp_server_name.as_bytes());
        }
        if let Some(boot_file) = &config.boot_file {
            options.extend_from_slice(&[67, boot_file.len() as u8]);
            options.extend_from_slice(boot_file.as_bytes());
        }
        options.push(255); // End option

        let Some(dest) = reply_packet.options.get_mut(..options.len()) else {
            return;
        };
        dest.copy_from_slice(&options);
        self.send_packet(responses, config, &reply_packet);
    }

    fn send_offer(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        discover_packet: &DhcpPacket,
        offer_ip: Ipv4Addr,
    ) {
        self.send_reply_packet(
            responses,
            config,
            discover_packet.xid,
            discover_packet.chaddr,
            offer_ip,
            DhcpMessageType::Offer,
        );
    }

    fn send_ack(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        request_packet: &DhcpPacket,
        ack_ip: Ipv4Addr,
    ) {
        self.send_reply_packet(
            responses,
            config,
            request_packet.xid,
            request_packet.chaddr,
            ack_ip,
            DhcpMessageType::Ack,
        );
    }

    fn send_nak(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        request_packet: &DhcpPacket,
    ) {
        let mut nak_packet = DhcpPacket {
            op: 2, // BootReply
            htype: 1,
            hlen: 6,
            hops: 0,
            xid: request_packet.xid,
            secs: [0; 2],
            flags: [0; 2],
            ciaddr: [0; 4],
            yiaddr: [0; 4],
            siaddr: [0; 4],
            giaddr: [0; 4],
            chaddr: request_packet.chaddr,
            sname: [0; 64],
            file: [0; 128],
            magic_cookie: DHCP_MAGIC_COOKIE,
            options: [0; 308],
        };

        let mut options = Vec::new();
        options.extend_from_slice(&[53, 1, DhcpMessageType::Nak as u8]);
        options.push(255); // End option

        let Some(dest) = nak_packet.options.get_mut(..options.len()) else {
            return;
        };
        dest.copy_from_slice(&options);
        self.send_packet(responses, config, &nak_packet);
    }

    fn send_packet(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        dhcp_packet: &DhcpPacket,
    ) {
        let mut buffer = vec![0u8; 1500];
        let eth_header_len = 14;
        let ipv4_header_len = 20;
        let dhcp_len = std::mem::size_of::<DhcpPacket>();

        let (eth_header_slice, eth_payload) = buffer.split_at_mut(eth_header_len);
        let eth_frame = EthernetFrame::mut_from_bytes(eth_header_slice).unwrap();
        eth_frame.dst_addr = MacAddr { bytes: [0xff; 6] };
        eth_frame.src_addr = config.gateway_mac;
        eth_frame.ethertype = 0x0800.into();

        let (ipv4_header_slice, ipv4_payload) = eth_payload.split_at_mut(ipv4_header_len);
        let mut ipv4_builder =
            Ipv4Builder::new(ipv4_header_slice, 17, config.host_ipv4, [255; 4].into()).unwrap();

        let mut udp_builder = UdpPacketBuilder::new(ipv4_payload, 67, 68).unwrap();
        let Some(dest) = udp_builder.payload_mut().get_mut(..dhcp_len) else {
            return;
        };
        dest.copy_from_slice(dhcp_packet.as_bytes());
        udp_builder.payload_len(dhcp_len);
        let udp_len = udp_builder.build();
        ipv4_builder.payload_len(udp_len);
        ipv4_builder.build();

        let packet_len = eth_header_len + ipv4_header_len + udp_len;
        let Some(packet_bytes) = buffer.get(..packet_len) else {
            return;
        };
        responses.push(SlirpResponse::Packet(Bytes::copy_from_slice(packet_bytes)));
    }
}
