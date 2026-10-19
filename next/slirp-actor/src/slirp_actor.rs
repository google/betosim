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

use std::{
    collections::HashMap,
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

use netsim_model::{ChipId, PacketSink, PacketStream};
use netsim_packets::MacAddress;
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tracing::info;

type ClientId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlirpIpv4Lease {
    pub ip_address: Ipv4Addr,
    pub prefixlen: u8,
    pub gateway: Ipv4Addr,
    pub dns: Ipv4Addr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlirpIpv6Lease {
    pub ip_address: Ipv6Addr,
    pub prefixlen: u8,
    pub gateway: Ipv6Addr,
    pub dns: Ipv6Addr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlirpLease {
    pub v4: SlirpIpv4Lease,
    pub v6: Option<SlirpIpv6Lease>,
}

struct SlirpSubnetParams {
    network: Ipv4Addr,
    netmask: Ipv4Addr,
    gateway: Ipv4Addr,
    dns: Ipv4Addr,
    dhcp_start: Ipv4Addr,
    in6_enabled: bool,
    prefix_addr6: Ipv6Addr,
    prefix_len: u8,
    gateway6: Ipv6Addr,
    dns6: Ipv6Addr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlirpBackend {
    CFfi,
    Native,
}

#[allow(clippy::derivable_impls)]
impl Default for SlirpBackend {
    fn default() -> Self {
        #[cfg(feature = "cuttlefish")]
        {
            SlirpBackend::Native
        }
        #[cfg(not(feature = "cuttlefish"))]
        {
            SlirpBackend::CFfi
        }
    }
}

#[cfg(not(feature = "cuttlefish"))]
pub type SlirpConfig = libslirp_rs::SlirpConfig;

#[cfg(feature = "cuttlefish")]
pub type SlirpConfig = slirp::Config;

pub enum SlirpReq {
    SendPacket(bytes::Bytes),
    Register {
        client_id: ClientId,
        stream: std::pin::Pin<Box<dyn tokio_stream::Stream<Item = bytes::Bytes> + Sync + Send>>,
        sink: netsim_model::PacketSink,
        notifier: Option<tokio_mpsc::UnboundedSender<netsim_model::ChipId>>,
        isolated: bool,
    },
    Unregister {
        client_id: ClientId,
    },
    AllocateLease {
        chip_id: ChipId,
        respond_to: oneshot::Sender<Option<SlirpLease>>,
    },
    ReleaseLease {
        chip_id: ChipId,
    },
    SwitchBackend(SlirpBackend),
}

impl std::fmt::Debug for SlirpReq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlirpReq::SendPacket(_) => write!(f, "SlirpReq::SendPacket(...)"),
            SlirpReq::Register { client_id, isolated, .. } => {
                write!(
                    f,
                    "SlirpReq::Register {{ client_id: {client_id}, isolated: {isolated}, ... }}"
                )
            }
            SlirpReq::Unregister { client_id } => {
                write!(f, "SlirpReq::Unregister {{ client_id: {client_id} }}")
            }
            SlirpReq::AllocateLease { chip_id, .. } => {
                write!(f, "SlirpReq::AllocateLease {{ chip_id: {chip_id} }}")
            }
            SlirpReq::ReleaseLease { chip_id } => {
                write!(f, "SlirpReq::ReleaseLease {{ chip_id: {chip_id} }}")
            }
            SlirpReq::SwitchBackend(backend) => {
                write!(f, "SlirpReq::SwitchBackend({:?})", backend)
            }
        }
    }
}

pub struct SlirpCreate {
    pub packet_stream: Option<PacketStream>,
    pub packet_sink: Option<PacketSink>,
}

impl fmt::Debug for SlirpCreate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlirpCreate")
            .field("packet_stream", &self.packet_stream.is_some())
            .field("packet_sink", &self.packet_sink.is_some())
            .finish()
    }
}

pub(crate) struct ClientInfo {
    sink: tokio_mpsc::UnboundedSender<bytes::Bytes>,
    notifier: Option<tokio_mpsc::UnboundedSender<netsim_model::ChipId>>,
    isolated: bool,
}

impl ClientInfo {
    pub(crate) fn new(
        sink: tokio_mpsc::UnboundedSender<bytes::Bytes>,
        notifier: Option<tokio_mpsc::UnboundedSender<netsim_model::ChipId>>,
        isolated: bool,
    ) -> Self {
        Self { sink, notifier, isolated }
    }

    pub(crate) fn send(&self, msg: bytes::Bytes) {
        let _ = self.sink.send(msg);
    }

    pub(crate) fn notify_disconnected(&self, id: u32) {
        if let Some(notifier) = &self.notifier {
            let _ = notifier.send(netsim_model::ChipId(id));
        }
    }

    pub(crate) fn is_isolated(&self) -> bool {
        self.isolated
    }
}

pub(crate) enum SlirpBackendInstance {
    #[cfg(not(feature = "cuttlefish"))]
    CFfi(libslirp_rs::LibSlirp),
    Native {
        uplink_tx: tokio_mpsc::UnboundedSender<bytes::Bytes>,
    },
}

impl SlirpBackendInstance {
    pub(crate) fn input(&self, msg: bytes::Bytes) {
        match self {
            #[cfg(not(feature = "cuttlefish"))]
            SlirpBackendInstance::CFfi(slirp) => {
                slirp.input(msg);
            }
            SlirpBackendInstance::Native { uplink_tx } => {
                let _ = uplink_tx.send(msg);
            }
        }
    }

    pub(crate) fn shutdown(self) {
        match self {
            #[cfg(not(feature = "cuttlefish"))]
            SlirpBackendInstance::CFfi(slirp) => {
                slirp.shutdown();
            }
            SlirpBackendInstance::Native { .. } => {}
        }
    }
}

pub struct SlirpActor {
    pub(crate) backend_type: SlirpBackend,
    pub(crate) backend_instance: Option<SlirpBackendInstance>,
    pub(crate) config: SlirpConfig,
    pub(crate) http_proxy: Option<String>,
    pub(crate) clients: HashMap<ClientId, ClientInfo>,
    pub(crate) static_ips: HashMap<ChipId, Ipv4Addr>,
    pub(crate) mac_table: HashMap<MacAddress, ClientId>,
    pub(crate) next_stream_id: usize,
    pub(crate) active_stream_id: Option<usize>,
    pub(crate) next_task_id: u32,
    pub(crate) active_task_id: Option<u32>,
}

async fn resolve_dns_servers(host_dns: &str) -> Vec<IpAddr> {
    let mut resolved = Vec::new();
    if host_dns.is_empty() {
        return resolved;
    }
    let futures = host_dns.split(',').map(|addr| {
        let addr = addr.trim().to_string();
        async move {
            if let Ok(ip) = addr.parse::<IpAddr>() {
                return vec![ip];
            }
            if let Ok(socket) = addr.parse::<SocketAddr>() {
                return vec![socket.ip()];
            }
            let host_port = if addr.contains(':') { addr.clone() } else { format!("{}:53", addr) };
            match tokio::net::lookup_host(host_port).await {
                Ok(addrs) => addrs.map(|s| s.ip()).collect::<Vec<_>>(),
                Err(error) => {
                    tracing::warn!(%error, "Failed to resolve DNS host '{addr}'");
                    vec![]
                }
            }
        }
    });
    let results = futures::future::join_all(futures).await;
    for ip in results.into_iter().flatten() {
        resolved.push(ip);
    }
    resolved
}

#[derive(Clone, Debug)]
pub struct SlirpStatus {
    pub initialized: bool,
}

impl SlirpActor {
    pub async fn new(
        config: SlirpConfig,
        http_proxy: Option<String>,
        host_dns: Option<String>,
    ) -> Self {
        Self::new_with_backend(config, http_proxy, host_dns, SlirpBackend::default()).await
    }

    pub async fn new_with_backend(
        mut config: SlirpConfig,
        http_proxy: Option<String>,
        host_dns: Option<String>,
        backend_type: SlirpBackend,
    ) -> Self {
        let resolved_dns = if let Some(ref host_dns_str) = host_dns {
            resolve_dns_servers(host_dns_str).await
        } else {
            Vec::new()
        };

        if !resolved_dns.is_empty() {
            #[cfg(not(feature = "cuttlefish"))]
            {
                config.host_dns = resolved_dns.iter().map(|ip| SocketAddr::new(*ip, 53)).collect();
            }
            #[cfg(feature = "cuttlefish")]
            {
                config.dns_servers = resolved_dns.clone();
            }
        }

        Self {
            backend_type,
            backend_instance: None,
            config,
            http_proxy,
            clients: HashMap::new(),
            static_ips: HashMap::new(),
            mac_table: HashMap::new(),
            next_stream_id: 1,
            active_stream_id: None,
            next_task_id: 1,
            active_task_id: None,
        }
    }

    pub fn backend(&self) -> SlirpBackend {
        self.backend_type
    }

    #[cfg(not(feature = "cuttlefish"))]
    fn subnet_params(&self) -> SlirpSubnetParams {
        SlirpSubnetParams {
            network: self.config.vnetwork,
            netmask: self.config.vnetmask,
            gateway: self.config.vhost,
            dns: self.config.vnameserver,
            dhcp_start: self.config.vdhcp_start,
            in6_enabled: self.config.in6_enabled,
            prefix_addr6: self.config.vprefix_addr6,
            prefix_len: self.config.vprefix_len,
            gateway6: self.config.vhost6,
            dns6: self.config.vnameserver6,
        }
    }

    #[cfg(feature = "cuttlefish")]
    fn subnet_params(&self) -> SlirpSubnetParams {
        // The native DHCP server always advertises a /24 and the RA a /64 of guest_ipv6.
        let netmask = Ipv4Addr::new(255, 255, 255, 0);
        let prefix_addr6 = Ipv6Addr::from(u128::from(self.config.guest_ipv6) & (!0u128 << 64));
        SlirpSubnetParams {
            network: Ipv4Addr::from(u32::from(self.config.host_ipv4) & u32::from(netmask)),
            netmask,
            gateway: self.config.host_ipv4,
            dns: self.config.host_ipv4,
            dhcp_start: Ipv4Addr::new(10, 0, 2, 16),
            in6_enabled: true,
            prefix_addr6,
            prefix_len: 64,
            gateway6: self.config.host_ipv6,
            dns6: self.config.host_ipv6,
        }
    }

    pub(crate) fn allocate_lease(&mut self, chip_id: ChipId) -> Option<SlirpLease> {
        let params = self.subnet_params();
        let prefixlen = u32::from(params.netmask).count_ones() as u8;

        let host = if let Some(&ip_address) = self.static_ips.get(&chip_id) {
            ip_address.octets()[3]
        } else {
            // Reserve 16 addresses for libslirp's BOOTP/DHCP pool (NB_BOOTP_CLIENTS = 16).
            let [o0, o1, o2, _] = params.network.octets();
            let start_host = params.dhcp_start.octets()[3].checked_add(16)?;
            let Some(ip_address) =
                (start_host..=254u8).map(|h| Ipv4Addr::new(o0, o1, o2, h)).find(|&ip| {
                    ip != params.gateway
                        && ip != params.dns
                        && !self.static_ips.values().any(|&used| used == ip)
                })
            else {
                tracing::warn!("Slirp static IP pool exhausted for chip {chip_id}");
                return None;
            };
            let host = ip_address.octets()[3];
            self.static_ips.insert(chip_id, ip_address);
            host
        };

        let ip_address = Ipv4Addr::new(
            params.network.octets()[0],
            params.network.octets()[1],
            params.network.octets()[2],
            host,
        );
        let v4 = SlirpIpv4Lease { ip_address, prefixlen, gateway: params.gateway, dns: params.dns };

        let v6 = if params.in6_enabled {
            Some(SlirpIpv6Lease {
                ip_address: ipv6_lease_addr(params.prefix_addr6, host),
                prefixlen: params.prefix_len,
                gateway: params.gateway6,
                dns: params.dns6,
            })
        } else {
            None
        };

        Some(SlirpLease { v4, v6 })
    }

    pub(crate) fn release_lease(&mut self, chip_id: ChipId) {
        self.static_ips.remove(&chip_id);
    }

    pub(crate) fn matches_static_ipv6(&self, target: Ipv6Addr) -> bool {
        let params = self.subnet_params();
        if params.in6_enabled {
            return self
                .static_ips
                .values()
                .any(|&ip4| ipv6_lease_addr(params.prefix_addr6, ip4.octets()[3]) == target);
        }
        false
    }
}

fn ipv6_lease_addr(prefix_addr6: Ipv6Addr, host: u8) -> Ipv6Addr {
    let mut octets = prefix_addr6.octets();
    octets[14] = 0;
    octets[15] = host;
    Ipv6Addr::from(octets)
}

impl Drop for SlirpActor {
    fn drop(&mut self) {
        if let Some(instance) = self.backend_instance.take() {
            info!("Shutting down Slirp backend instance");
            instance.shutdown();
        }
    }
}
