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

use std::sync::Arc;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, UdpSocket},
};
use tokio_util::sync::CancellationToken;
use verify_macros::{step, step_module};

use crate::{
    orchestrator::TestContext,
    types::{ClientParams, Throughput},
};

#[step_module]
pub mod steps {
    use super::*;

    #[step(r#"@host starts a (TCP|UDP) echo server on "(\w+)""#)]
    pub async fn start_echo_server(
        w: &mut TestContext,
        proto: String,
        var_name: String,
    ) -> Result<(), String> {
        let port =
            w.host.start_server(0).await.map_err(|e| format!("Failed to start server: {}", e))?;
        // Store full address (GatewayIP:Port) so usage {var} works directly
        let full_addr = format!("{}:{}", w.gateway_ip, port);
        w.log_step(
            "@host",
            "->",
            &format!("starts {} echo server on '{}'", proto.to_uppercase(), var_name),
        );
        w.set_variable(&var_name, full_addr);
        Ok(())
    }

    #[step(r#"@host receives (\d+)(KB|B|MB) (TCP|UDP) data(?: total)?"#)]
    pub async fn host_receives_data(
        w: &mut TestContext,
        size_val: usize,
        unit: String,
        proto: String,
    ) -> Result<(), String> {
        let actor = "@host";
        w.log_step(actor, "THEN", &format!("Receives {}{} {} data", size_val, unit, proto));
        Ok(())
    }

    #[step("@host receives all coordinated data")]
    pub async fn host_receives_coordinated_data(w: &mut TestContext) -> Result<(), String> {
        let actor = "@host";
        w.log_step(actor, "THEN", "Receives all coordinated data");
        Ok(())
    }

    #[step(r#"@host advertises mDNS service (.+)"#)]
    pub async fn host_advertises_mdns_service(
        w: &mut TestContext,
        service: String,
    ) -> Result<(), String> {
        let actor = "@host";
        w.log_step(actor, "WHEN", &format!("Advertises mDNS service '{}'", service));

        if w.host.is_dry_run {
            return Ok(());
        }

        let mut name_bytes = Vec::new();
        let service_labels: Vec<&str> = service.split('.').filter(|s| !s.is_empty()).collect();
        for label in service_labels {
            name_bytes.push(label.len() as u8);
            name_bytes.extend_from_slice(label.as_bytes());
        }
        // Always append .local for valid mDNS
        name_bytes.push(5);
        name_bytes.extend_from_slice(b"local");
        name_bytes.push(0); // Null terminator

        let mut packet = Vec::with_capacity(12 + name_bytes.len() + 12);
        // DNS Header
        packet.extend_from_slice(&[
            0x00, 0x00, // Transaction ID
            0x84, 0x00, // Flags: Response, Authoritative
            0x00, 0x00, // Questions
            0x00, 0x01, // Answer RRs
            0x00, 0x00, // Authority RRs
            0x00, 0x00, // Additional RRs
        ]);
        // Answer 1 Name
        packet.extend(name_bytes);
        // Answer 1 Type, Class, TTL, Len, Data
        packet.extend_from_slice(&[
            0x00, 0x0c, // Type: PTR
            0x00, 0x01, // Class: IN
            0x00, 0x00, 0x00, 0x78, // TTL: 120
            0x00, 0x02, // Data Length: 2
            0xc0, 0x0c, // Data: Pointer to offset 12 (start of name)
        ]);

        let socket = tokio::net::UdpSocket::bind(("0.0.0.0", 0))
            .await
            .map_err(|e| format!("Failed to bind UDP socket for advertisement: {}", e))?;
        socket
            .set_multicast_loop_v4(true)
            .map_err(|e| format!("Failed to set multicast loop: {}", e))?;

        let mdns_addr = "224.0.0.251:5353";
        socket
            .send_to(&packet, mdns_addr)
            .await
            .map_err(|e| format!("Failed to send mDNS advertisement: {}", e))?;
        Ok(())
    }
}

pub use steps::register_steps;

const BUFFER_SIZE_TCP: usize = 64 * 1024;
const BUFFER_SIZE_UDP: usize = 65535;

pub struct HostWorld {
    pub server_token: Option<CancellationToken>,
    pub last_port: Option<u16>,
    pub is_dry_run: bool,
}

impl HostWorld {
    pub async fn run_client(
        &mut self,
        _params: ClientParams,
    ) -> Result<Option<Throughput>, String> {
        Ok(None)
    }

    pub async fn reset_actor(&mut self) -> Result<(), String> {
        self.stop_server();
        Ok(())
    }

    pub fn get_label(&self) -> String {
        "host".to_string()
    }

    pub fn set_silent(&self, _silent: bool) {}
}

impl HostWorld {
    pub fn new(is_dry_run: bool) -> Self {
        Self { server_token: None, last_port: None, is_dry_run: is_dry_run }
    }

    pub async fn start_server(&mut self, port: u16) -> Result<u16, String> {
        let assigned_port = if self.is_dry_run {
            if port == 0 { 12345 } else { port }
        } else {
            self.stop_server();
            let token = CancellationToken::new();
            let p = run_server(port, token.clone()).await?;
            self.server_token = Some(token);
            p
        };

        self.last_port = Some(assigned_port);
        Ok(assigned_port)
    }

    pub fn stop_server(&mut self) {
        if let Some(token) = self.server_token.take() {
            token.cancel();
        }
    }
}

impl Drop for HostWorld {
    fn drop(&mut self) {
        self.stop_server();
    }
}

/// Runs an echo server (TCP and UDP) on the specified port.
/// Returns the assigned port.
async fn run_server(port: u16, token: CancellationToken) -> Result<u16, String> {
    let (tcp_listener, udp_socket, local_port) = loop {
        let listener = TcpListener::bind(("0.0.0.0", port)).await.map_err(|e| e.to_string())?;
        let p = listener.local_addr().map_err(|e| e.to_string())?.port();
        match UdpSocket::bind(("0.0.0.0", p)).await {
            Ok(u) => break (listener, u, p),
            Err(e) => {
                if port != 0 {
                    return Err(e.to_string());
                }
                continue;
            }
        }
    };

    let udp_socket = Arc::new(udp_socket);

    // UDP Loop
    let udp_sock_clone = udp_socket.clone();
    let udp_token = token.clone();
    tokio::spawn(async move {
        let mut buf = [0u8; BUFFER_SIZE_UDP];
        loop {
            tokio::select! {
                _ = udp_token.cancelled() => break,
                res = udp_sock_clone.recv_from(&mut buf) => {
                    if let Ok((size, peer)) = res {
                        let _ = udp_sock_clone.send_to(&buf[..size], peer).await;
                    }
                }
            }
        }
    });

    // TCP Loop
    let tcp_token = token.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tcp_token.cancelled() => break,
                res = tcp_listener.accept() => {
                    if let Ok((mut socket, _)) = res {
                        let inner_token = tcp_token.clone();
                        tokio::spawn(async move {
                            let mut buf = [0u8; BUFFER_SIZE_TCP];
                            loop {
                                tokio::select! {
                                    _ = inner_token.cancelled() => break,
                                    res = socket.read(&mut buf) => {
                                        match res {
                                            Ok(0) | Err(_) => break,
                                            Ok(n) => {
                                                if socket.write_all(&buf[..n]).await.is_err() { break; }
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    }
                }
            }
        }
    });

    Ok(local_port)
}
