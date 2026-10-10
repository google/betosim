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

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use netsim_packets::MacAddr;

pub use crate::proxy::{ProxyConnect, ProxyManager};
use crate::tcp;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionArgs {
    Tcp(TcpConnectionArgs),
    Udp(UdpConnectionArgs),
    Icmp(IcmpConnectionArgs),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TcpConnectionArgs {
    pub destination: SocketAddr,
    pub guest_ip: IpAddr,
    pub guest_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UdpConnectionArgs {
    pub destination: SocketAddr,
    pub guest_ip: IpAddr,
    pub guest_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IcmpConnectionArgs {
    pub destination: IpAddr,
    pub guest_ip: IpAddr,
    pub guest_id: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionInfo {
    Tcp(TcpConnectionInfo),
    Udp(UdpConnectionInfo),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpConnectionInfo {
    pub local_addr: SocketAddr,
    pub peer_addr: SocketAddr,
    pub state: tcp::State,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UdpConnectionInfo {
    pub local_addr: SocketAddr,
    pub peer_addr: SocketAddr,
}

pub enum SlirpRequest {
    Packet(Bytes),
    WriteComplete(u64),
    Data(u64, Bytes),
    ConnectionClosed(u64),
    RemoteClosed(u64),
    Timer,
    DeactivateFastPath { conn_id: u64 },
    AcceptIncoming { conn_id: u64, host_addr: SocketAddr, guest_addr: SocketAddr },
    SaveState(std::sync::mpsc::Sender<Vec<u8>>),
    RestoreState(Bytes),
}

impl Clone for SlirpRequest {
    fn clone(&self) -> Self {
        match self {
            SlirpRequest::Packet(p) => SlirpRequest::Packet(p.clone()),
            SlirpRequest::WriteComplete(id) => SlirpRequest::WriteComplete(*id),
            SlirpRequest::Data(id, d) => SlirpRequest::Data(*id, d.clone()),
            SlirpRequest::ConnectionClosed(id) => SlirpRequest::ConnectionClosed(*id),
            SlirpRequest::RemoteClosed(id) => SlirpRequest::RemoteClosed(*id),
            SlirpRequest::Timer => SlirpRequest::Timer,
            SlirpRequest::DeactivateFastPath { conn_id } => {
                SlirpRequest::DeactivateFastPath { conn_id: *conn_id }
            }
            SlirpRequest::AcceptIncoming { conn_id, host_addr, guest_addr } => {
                SlirpRequest::AcceptIncoming {
                    conn_id: *conn_id,
                    host_addr: *host_addr,
                    guest_addr: *guest_addr,
                }
            }
            SlirpRequest::SaveState(sender) => SlirpRequest::SaveState(sender.clone()),
            SlirpRequest::RestoreState(bytes) => SlirpRequest::RestoreState(bytes.clone()),
        }
    }
}

impl std::fmt::Debug for SlirpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlirpRequest::Packet(p) => write!(f, "Packet({} bytes)", p.len()),
            SlirpRequest::WriteComplete(id) => write!(f, "WriteComplete({id})"),
            SlirpRequest::Data(id, d) => write!(f, "Data({id}, {} bytes)", d.len()),
            SlirpRequest::ConnectionClosed(id) => write!(f, "ConnectionClosed({id})"),
            SlirpRequest::RemoteClosed(id) => write!(f, "RemoteClosed({id})"),
            SlirpRequest::Timer => write!(f, "Timer"),
            SlirpRequest::DeactivateFastPath { conn_id } => {
                write!(f, "DeactivateFastPath {{ conn_id: {conn_id} }}")
            }
            SlirpRequest::AcceptIncoming { conn_id, host_addr, guest_addr } => {
                write!(
                    f,
                    "AcceptIncoming {{ conn_id: {conn_id}, host_addr: {host_addr}, guest_addr: {guest_addr} }}"
                )
            }
            SlirpRequest::SaveState(_) => write!(f, "SaveState"),
            SlirpRequest::RestoreState(bytes) => {
                write!(f, "RestoreState({} bytes)", bytes.len())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SlirpResponse {
    Packet(Bytes),
    EstablishConnection(u64, ConnectionArgs),
    WriteToConnection(u64, Bytes),
    CloseConnection { conn_id: u64, guest_addr: SocketAddr },
    Shutdown,
    SetTimer(Duration),
    ActivateFastPath { conn_id: u64, guest_addr: SocketAddr, host_addr: SocketAddr },
    ConnectionEstablished(u64),
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Proto {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GuestFwdRule {
    pub virtual_addr: SocketAddr,
    pub host_addr: SocketAddr,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostFwdRule {
    pub proto: Proto,
    pub host_addr: SocketAddr,
    pub guest_addr: SocketAddr,
}

#[derive(Clone)]
pub struct Config {
    pub guest_mac: MacAddr,
    pub gateway_mac: MacAddr,
    pub guest_ipv4: Ipv4Addr,
    pub host_ipv4: Ipv4Addr,
    pub guest_ipv6: Ipv6Addr,
    pub host_ipv6: Ipv6Addr,
    pub boot_file: Option<String>,
    pub tftp_server_name: Option<String>,
    pub domain_name: Option<String>,
    pub dns_search: Option<Vec<String>>,
    pub client_hostname: Option<String>,
    pub dns_servers: Vec<IpAddr>,
    pub guestfwd: Vec<GuestFwdRule>,
    pub hostfwd: Vec<HostFwdRule>,
    pub tftp_root: Option<std::path::PathBuf>,
    pub socks5_proxy: Option<SocketAddr>,
    pub http_proxy: Option<String>,
    pub proxy_manager: Option<Arc<dyn ProxyManager>>,
    /// Opaque handle to the proxy manager's TX channel to avoid circular crate
    /// dependencies.
    pub proxy_tx: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("guest_mac", &self.guest_mac)
            .field("gateway_mac", &self.gateway_mac)
            .field("guest_ipv4", &self.guest_ipv4)
            .field("host_ipv4", &self.host_ipv4)
            .field("guest_ipv6", &self.guest_ipv6)
            .field("host_ipv6", &self.host_ipv6)
            .field("boot_file", &self.boot_file)
            .field("tftp_server_name", &self.tftp_server_name)
            .field("domain_name", &self.domain_name)
            .field("dns_search", &self.dns_search)
            .field("client_hostname", &self.client_hostname)
            .field("dns_servers", &self.dns_servers)
            .field("guestfwd", &self.guestfwd)
            .field("hostfwd", &self.hostfwd)
            .field("tftp_root", &self.tftp_root)
            .field("socks5_proxy", &self.socks5_proxy)
            .field("http_proxy", &self.http_proxy)
            .field("proxy_manager", &self.proxy_manager.is_some())
            .field("proxy_tx", &self.proxy_tx.is_some())
            .finish()
    }
}

pub const DEFAULT_DNS_SERVER: IpAddr = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));

impl Default for Config {
    fn default() -> Self {
        Self {
            guest_mac: MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34] },
            gateway_mac: MacAddr { bytes: [0xde, 0xad, 0xbe, 0xef, 0x12, 0x35] },
            guest_ipv4: Ipv4Addr::new(10, 0, 2, 15),
            host_ipv4: Ipv4Addr::new(10, 0, 2, 2),
            guest_ipv6: "fec0::1".parse().unwrap(),
            host_ipv6: "fec0::2".parse().unwrap(),
            boot_file: None,
            tftp_server_name: None,
            domain_name: None,
            dns_search: None,
            client_hostname: None,
            dns_servers: vec![DEFAULT_DNS_SERVER],
            guestfwd: Vec::new(),
            hostfwd: Vec::new(),
            tftp_root: None,
            socks5_proxy: None,
            http_proxy: None,
            proxy_manager: None,
            proxy_tx: None,
        }
    }
}
