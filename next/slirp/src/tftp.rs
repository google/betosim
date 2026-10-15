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
    collections::HashMap,
    fs::File,
    io::Read,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};

use bytes::Bytes;
use log::{debug, error, info, warn};
use netsim_packets::MacAddr;
use zerocopy::FromBytes;

use crate::api::{Config, SlirpResponse};

const TFTP_PORT: u16 = 69;
const TFTP_BLOCK_SIZE: usize = 512;

const OP_RRQ: u16 = 1;
const OP_WRQ: u16 = 2;
const OP_DATA: u16 = 3;
const OP_ACK: u16 = 4;
const OP_ERROR: u16 = 5;

const ERR_NOT_FOUND: u16 = 1;
const ERR_ACCESS_VIOLATION: u16 = 2;
const ERR_ILLEGAL_OP: u16 = 4;
const ERR_UNKNOWN_TID: u16 = 5;

struct TftpSession {
    client_addr: SocketAddr,
    file: File,
    block_num: u16,
    last_block_sent: Vec<u8>,
    is_last_block: bool,
}

pub struct TftpManager {
    sessions: HashMap<u16, TftpSession>,
    next_tid: u16,
}

impl Default for TftpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TftpManager {
    pub fn new() -> Self {
        Self { sessions: HashMap::new(), next_tid: 50000 }
    }

    pub fn has_session_with_tid(&self, tid: u16) -> bool {
        self.sessions.contains_key(&tid)
    }

    pub fn handle_packet(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        packet: &[u8],
        src_addr: SocketAddr,
        local_port: u16,
        guest_mac: MacAddr,
    ) {
        let Some((&op_bytes, payload)) = packet.split_first_chunk::<2>() else {
            warn!("TFTP: Packet too short");
            return;
        };
        let opcode = u16::from_be_bytes(op_bytes);

        match opcode {
            OP_RRQ => self.handle_rrq(responses, config, payload, src_addr, guest_mac),
            OP_WRQ => self.send_error(
                responses,
                config,
                src_addr,
                TFTP_PORT,
                ERR_ACCESS_VIOLATION,
                "Write not supported",
                guest_mac,
            ),
            OP_ACK => self.handle_ack(responses, config, payload, src_addr, local_port, guest_mac),
            OP_ERROR => self.handle_error(payload, src_addr),
            _ => self.send_error(
                responses,
                config,
                src_addr,
                TFTP_PORT,
                ERR_ILLEGAL_OP,
                "Illegal TFTP operation",
                guest_mac,
            ),
        }
    }

    fn handle_rrq(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        payload: &[u8],
        src_addr: SocketAddr,
        guest_mac: MacAddr,
    ) {
        let Some(tftp_root) = &config.tftp_root else {
            warn!("TFTP: Received RRQ but tftp_root is not configured");
            self.send_error(
                responses,
                config,
                src_addr,
                TFTP_PORT,
                ERR_ACCESS_VIOLATION,
                "TFTP server disabled",
                guest_mac,
            );
            return;
        };

        // Parse filename and mode (null-terminated strings)
        let Some((filename, remaining)) = parse_null_terminated_string(payload) else {
            self.send_error(
                responses,
                config,
                src_addr,
                TFTP_PORT,
                ERR_ILLEGAL_OP,
                "Malformed RRQ",
                guest_mac,
            );
            return;
        };
        let Some((mode, _)) = parse_null_terminated_string(remaining) else {
            self.send_error(
                responses,
                config,
                src_addr,
                TFTP_PORT,
                ERR_ILLEGAL_OP,
                "Malformed RRQ",
                guest_mac,
            );
            return;
        };

        debug!("TFTP: RRQ for file='{filename}', mode='{mode}' from {src_addr}");

        // Security check: validate path to prevent directory traversal
        let safe_path = match validate_and_resolve_path(tftp_root, filename) {
            Ok(path) => path,
            Err(e) => {
                warn!("TFTP: Security check failed for filename '{filename}': {e}");
                self.send_error(
                    responses,
                    config,
                    src_addr,
                    TFTP_PORT,
                    ERR_ACCESS_VIOLATION,
                    "Access violation",
                    guest_mac,
                );
                return;
            }
        };

        // Open the file
        let mut file = match File::open(&safe_path) {
            Ok(f) => f,
            Err(_) => {
                warn!("TFTP: File not found: {}", safe_path.display());
                self.send_error(
                    responses,
                    config,
                    src_addr,
                    TFTP_PORT,
                    ERR_NOT_FOUND,
                    "File not found",
                    guest_mac,
                );
                return;
            }
        };

        // Allocate a unique TID (local port)
        let tid = self.allocate_tid();

        // Read the first block
        let mut block_data = vec![0u8; TFTP_BLOCK_SIZE];
        let n = match file.read(&mut block_data) {
            Ok(bytes_read) => bytes_read,
            Err(e) => {
                error!("TFTP: Failed to read file: {e}");
                self.send_error(
                    responses,
                    config,
                    src_addr,
                    TFTP_PORT,
                    ERR_ACCESS_VIOLATION,
                    "Read error",
                    guest_mac,
                );
                return;
            }
        };
        block_data.truncate(n);

        let is_last_block = n < TFTP_BLOCK_SIZE;

        // Create session
        let session = TftpSession {
            client_addr: src_addr,
            file,
            block_num: 1,
            last_block_sent: block_data.clone(),
            is_last_block,
        };
        self.sessions.insert(tid, session);

        info!("TFTP: Started session tid={tid} for file='{filename}' to {src_addr}");

        // Send DATA block 1
        self.send_data(responses, config, src_addr, tid, 1, &block_data, guest_mac);
    }

    fn handle_ack(
        &mut self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        payload: &[u8],
        src_addr: SocketAddr,
        local_port: u16,
        guest_mac: MacAddr,
    ) {
        let Some(session) = self.sessions.get_mut(&local_port) else {
            warn!("TFTP: Received ACK for unknown TID {local_port}");
            self.send_error(
                responses,
                config,
                src_addr,
                local_port,
                ERR_UNKNOWN_TID,
                "Unknown TID",
                guest_mac,
            );
            return;
        };

        if session.client_addr != src_addr {
            warn!(
                "TFTP: Received ACK from unexpected client {}; expected {}",
                src_addr, session.client_addr
            );
            self.send_error(
                responses,
                config,
                src_addr,
                local_port,
                ERR_UNKNOWN_TID,
                "Unexpected client",
                guest_mac,
            );
            return;
        }

        let Some(&chunk) = payload.first_chunk::<2>() else {
            warn!("TFTP: Malformed ACK payload");
            return;
        };
        let block_num = u16::from_be_bytes(chunk);
        debug!("TFTP: Received ACK for block {block_num} from {src_addr} on tid={local_port}");

        // If the ACK matches the last block we sent
        if block_num == session.block_num {
            if session.is_last_block {
                info!("TFTP: Transfer complete for tid={local_port} to {src_addr}");
                self.sessions.remove(&local_port);
                return;
            }

            // Read the next block
            let mut block_data = vec![0u8; TFTP_BLOCK_SIZE];
            let n = match session.file.read(&mut block_data) {
                Ok(bytes_read) => bytes_read,
                Err(e) => {
                    error!("TFTP: Failed to read file: {e}");
                    self.send_error(
                        responses,
                        config,
                        src_addr,
                        local_port,
                        ERR_ACCESS_VIOLATION,
                        "Read error",
                        guest_mac,
                    );
                    self.sessions.remove(&local_port);
                    return;
                }
            };
            block_data.truncate(n);

            session.block_num = session.block_num.wrapping_add(1);
            session.last_block_sent = block_data.clone();
            session.is_last_block = n < TFTP_BLOCK_SIZE;

            let next_block_num = session.block_num;
            // Send next DATA block
            self.send_data(
                responses,
                config,
                src_addr,
                local_port,
                next_block_num,
                &block_data,
                guest_mac,
            );
        } else if block_num == session.block_num.wrapping_sub(1) {
            // Duplicate ACK for the previous block, indicating packet loss!
            // Retransmit the last sent block!
            debug!(
                "TFTP: Retransmitting block {} to {} on tid={}",
                session.block_num, src_addr, local_port
            );
            let last_block = session.last_block_sent.clone();
            let last_block_num = session.block_num;
            self.send_data(
                responses,
                config,
                src_addr,
                local_port,
                last_block_num,
                &last_block,
                guest_mac,
            );
        } else {
            warn!(
                "TFTP: Received ACK for unexpected block {}; expected {}",
                block_num, session.block_num
            );
        }
    }

    fn handle_error(&mut self, payload: &[u8], src_addr: SocketAddr) {
        let Some((&err_bytes, msg)) = payload.split_first_chunk::<2>() else {
            warn!("TFTP: ERROR packet too short");
            return;
        };
        let err_code = u16::from_be_bytes(err_bytes);
        let (err_msg, _) = parse_null_terminated_string(msg).unwrap_or(("unknown", &[]));
        warn!("TFTP: Received ERROR from client {src_addr}: code={err_code}, msg='{err_msg}'");

        // Terminate the session if it matches client's TID
        let session_to_remove = self.sessions.iter().find_map(|(&local_port, session)| {
            if session.client_addr == src_addr { Some(local_port) } else { None }
        });
        if let Some(local_port) = session_to_remove {
            info!("TFTP: Terminating session tid={local_port} due to client error");
            self.sessions.remove(&local_port);
        }
    }

    fn allocate_tid(&mut self) -> u16 {
        let tid = self.next_tid;
        self.next_tid = if self.next_tid == 60000 { 50000 } else { self.next_tid + 1 };
        tid
    }

    // Addressing params (config/dest_addr/src_port/guest_mac) are threaded
    // through from the caller; bundling them would churn all 14 call sites.
    #[allow(clippy::too_many_arguments)]
    fn send_data(
        &self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        dest_addr: SocketAddr,
        src_port: u16,
        block_num: u16,
        data: &[u8],
        guest_mac: MacAddr,
    ) {
        let packet = [&OP_DATA.to_be_bytes()[..], &block_num.to_be_bytes(), data].concat();
        let udp_packet = build_udp_packet(config, dest_addr, src_port, &packet, guest_mac);
        if !udp_packet.is_empty() {
            responses.push(SlirpResponse::Packet(Bytes::from(udp_packet)));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn send_error(
        &self,
        responses: &mut Vec<SlirpResponse>,
        config: &Config,
        dest_addr: SocketAddr,
        src_port: u16,
        err_code: u16,
        err_msg: &str,
        guest_mac: MacAddr,
    ) {
        let packet =
            [&OP_ERROR.to_be_bytes()[..], &err_code.to_be_bytes(), err_msg.as_bytes(), &[0]]
                .concat();
        let udp_packet = build_udp_packet(config, dest_addr, src_port, &packet, guest_mac);
        if !udp_packet.is_empty() {
            responses.push(SlirpResponse::Packet(Bytes::from(udp_packet)));
        }
    }
}

fn parse_null_terminated_string(payload: &[u8]) -> Option<(&str, &[u8])> {
    let null_idx = payload.iter().position(|&b| b == 0)?;
    let s = std::str::from_utf8(payload.get(..null_idx)?).ok()?;
    Some((s, payload.get(null_idx + 1..)?))
}

fn validate_and_resolve_path(root: &Path, filename: &str) -> Result<PathBuf, String> {
    // 1. Normalize the filename (replace backslashes with forward slashes)
    let normalized = filename.replace('\\', "/");

    // 2. Prevent absolute paths or directory traversal
    let path = Path::new(&normalized);
    if path.is_absolute() {
        return Err("Absolute paths not allowed".to_string());
    }

    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                return Err("Directory traversal not allowed (..)".to_string());
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err("Absolute paths or prefixes not allowed".to_string());
            }
            _ => {}
        }
    }

    // 3. Join with root and canonicalize
    let resolved = root.join(path);

    let abs_root = root.canonicalize().map_err(|e| format!("Invalid root path: {e}"))?;

    if resolved.exists() {
        let abs_resolved =
            resolved.canonicalize().map_err(|e| format!("Failed to resolve path: {e}"))?;
        if !abs_resolved.starts_with(&abs_root) {
            return Err("Path escapes root directory (symlink)".to_string());
        }
        Ok(abs_resolved)
    } else {
        Ok(resolved)
    }
}

fn build_udp_packet(
    config: &Config,
    dest_addr: SocketAddr,
    src_port: u16,
    payload: &[u8],
    guest_mac: MacAddr,
) -> Vec<u8> {
    use netsim_packets::{EthernetFrame, Ipv4Builder, Ipv6Builder, UdpPacketBuilder};

    let eth_header_len = 14;
    let is_ipv6 = dest_addr.is_ipv6();
    let ip_header_len = if is_ipv6 { 40 } else { 20 };
    let udp_header_len = 8;
    let header_len = eth_header_len + ip_header_len + udp_header_len;

    let mut buffer = vec![0u8; header_len + payload.len()];

    // 1. Build Ethernet Header
    let (eth_header, eth_payload) = buffer.split_at_mut(eth_header_len);
    let eth_frame = EthernetFrame::mut_from_bytes(eth_header).unwrap();
    eth_frame.dst_addr = guest_mac;
    eth_frame.src_addr = config.gateway_mac;
    eth_frame.ethertype = if is_ipv6 { 0x86DD } else { 0x0800 }.into();

    // 2. Build IP Header
    let (ip_header_slice, ip_payload) = eth_payload.split_at_mut(ip_header_len);
    match dest_addr.ip() {
        IpAddr::V4(guest_ip) => {
            let mut ipv4_builder = Ipv4Builder::new(
                ip_header_slice,
                netsim_packets::IP_P_UDP,
                config.host_ipv4, // gateway IP
                guest_ip,
            )
            .unwrap();
            ipv4_builder.payload_len(udp_header_len + payload.len());
            ipv4_builder.build();
        }
        IpAddr::V6(guest_ip) => {
            let mut ipv6_builder = Ipv6Builder::new(
                ip_header_slice,
                netsim_packets::IP_P_UDP,
                config.host_ipv6, // gateway IPv6
                guest_ip,
            )
            .unwrap();
            ipv6_builder.payload_len(udp_header_len + payload.len());
            ipv6_builder.build();
        }
    }

    // 3. Build UDP Header and Payload
    let mut udp_builder = UdpPacketBuilder::new(ip_payload, src_port, dest_addr.port()).unwrap();
    udp_builder.payload_mut().copy_from_slice(payload);
    udp_builder.build();

    buffer
}
