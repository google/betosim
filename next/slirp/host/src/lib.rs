// Copyright 2023-2026 The Android Open Source Project
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
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use bytes::Bytes;
use log::{debug, error, info, trace, warn};
use parking_lot::Mutex;
use slirp::{Config, ConnectionArgs, HostFwdRule, Proto, SlirpRequest, SlirpResponse};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinHandle,
};

struct TcpConnection {
    writer: mpsc::Sender<Bytes>,
    reader_handle: JoinHandle<()>,
    writer_handle: JoinHandle<()>,
}

struct UdpConnection {
    writer: mpsc::Sender<Bytes>,
    reader_handle: JoinHandle<()>,
    writer_handle: JoinHandle<()>,
}

struct IcmpConnection {
    writer: mpsc::Sender<Bytes>,
    reader_handle: JoinHandle<()>,
    writer_handle: JoinHandle<()>,
}

enum Connection {
    Connecting(Option<JoinHandle<()>>),
    Tcp(TcpConnection),
    Udp(UdpConnection),
    Icmp(IcmpConnection),
}

pub struct TokioHost {
    slirp_request_sender: mpsc::Sender<SlirpRequest>,
    slirp_response_receiver: mpsc::Receiver<SlirpResponse>,
    guest_packet_sender: mpsc::Sender<Bytes>,
    timer_task: Option<JoinHandle<()>>,
    test_slirp_response_sender: Option<mpsc::Sender<SlirpResponse>>,
    fast_path_enabled: bool,
    fast_path_connections: Arc<Mutex<HashMap<SocketAddr, mpsc::Sender<Bytes>>>>,
    connections: Arc<Mutex<HashMap<u64, Connection>>>,
    hostfwd_tasks: Vec<JoinHandle<()>>,
    socks5_proxy: Option<SocketAddr>,
    slirp_task: Option<JoinHandle<()>>,
}

impl TokioHost {
    fn setup_hostfwd(
        rules: &[HostFwdRule],
        slirp_request_sender: mpsc::Sender<SlirpRequest>,
        connections: Arc<Mutex<HashMap<u64, Connection>>>,
    ) -> Vec<JoinHandle<()>> {
        let next_host_conn_id = Arc::new(AtomicU64::new(1 << 60));
        let mut hostfwd_tasks = Vec::new();

        for rule in rules {
            match rule.proto {
                Proto::Tcp => {
                    let task = tokio::spawn(start_tcp_hostfwd_listener(
                        rule.clone(),
                        slirp_request_sender.clone(),
                        connections.clone(),
                        next_host_conn_id.clone(),
                    ));
                    hostfwd_tasks.push(task);
                }
                Proto::Udp => {
                    warn!("UDP hostfwd is not supported yet!");
                }
            }
        }

        hostfwd_tasks
    }

    pub fn from_channels(
        slirp_request_sender: mpsc::Sender<SlirpRequest>,
        slirp_response_receiver: mpsc::Receiver<SlirpResponse>,
        guest_packet_sender: mpsc::Sender<Bytes>,
        test_slirp_response_sender: Option<mpsc::Sender<SlirpResponse>>,
        config: Config,
    ) -> Self {
        let socks5_proxy = config.socks5_proxy;
        let connections = Arc::new(Mutex::new(HashMap::new()));
        let hostfwd_tasks =
            Self::setup_hostfwd(&config.hostfwd, slirp_request_sender.clone(), connections.clone());

        Self {
            slirp_request_sender,
            slirp_response_receiver,
            guest_packet_sender,
            timer_task: None,
            test_slirp_response_sender,
            fast_path_enabled: true, // Assume fast path for tests
            fast_path_connections: Arc::new(Mutex::new(HashMap::new())),
            connections,
            hostfwd_tasks,
            socks5_proxy,
            slirp_task: None,
        }
    }

    pub fn handle(&self) -> TokioHostHandle {
        TokioHostHandle { sender: self.slirp_request_sender.clone() }
    }

    pub async fn run(&mut self) {
        while let Some(response) = self.slirp_response_receiver.recv().await {
            trace!("host received response: {response:?}");
            if let Some(sender) = &self.test_slirp_response_sender {
                sender.send(response.clone()).await.ok();
            }
            match response {
                SlirpResponse::Packet(packet) => {
                    self.guest_packet_sender.send(packet).await.ok();
                }
                SlirpResponse::EstablishConnection(conn_id, conn_info) => match conn_info {
                    ConnectionArgs::Tcp(conn_info) => {
                        let slirp_request_sender = self.slirp_request_sender.clone();
                        let connections = self.connections.clone();
                        let socks5_proxy = self.socks5_proxy;
                        self.connections.lock().insert(conn_id, Connection::Connecting(None));
                        let handle = tokio::spawn(async move {
                            let stream =
                                match connect_tcp(conn_info.destination, socks5_proxy).await {
                                    Ok(stream) => stream,
                                    Err(e) => {
                                        warn!(
                                            "Failed to connect TCP to {}: {e}",
                                            conn_info.destination
                                        );
                                        let was_connecting = matches!(
                                            connections.lock().remove(&conn_id),
                                            Some(Connection::Connecting(_))
                                        );
                                        if was_connecting {
                                            slirp_request_sender
                                                .send(SlirpRequest::ConnectionClosed(conn_id))
                                                .await
                                                .ok();
                                        }
                                        return;
                                    }
                                };
                            finish_connecting_tcp(
                                conn_id,
                                stream,
                                slirp_request_sender,
                                &connections,
                            );
                        });
                        attach_connecting_handle(conn_id, handle, &self.connections);
                    }
                    ConnectionArgs::Udp(conn_info) => {
                        if let Err(e) = register_udp_connection(
                            conn_id,
                            conn_info,
                            self.slirp_request_sender.clone(),
                            self.connections.clone(),
                        ) {
                            warn!("Failed to register UDP connection {conn_id}: {e}");
                            self.slirp_request_sender
                                .send(SlirpRequest::ConnectionClosed(conn_id))
                                .await
                                .ok();
                        }
                    }
                    ConnectionArgs::Icmp(conn_info) => {
                        if let Err(e) = register_icmp_connection(
                            conn_id,
                            conn_info,
                            self.slirp_request_sender.clone(),
                            self.connections.clone(),
                        ) {
                            warn!("Failed to register ICMP connection {conn_id}: {e}");
                            self.slirp_request_sender
                                .send(SlirpRequest::ConnectionClosed(conn_id))
                                .await
                                .ok();
                        }
                    }
                },
                SlirpResponse::WriteToConnection(conn_id, data) => {
                    let sender = {
                        let conns = self.connections.lock();
                        match conns.get(&conn_id) {
                            Some(Connection::Tcp(c)) => Some(c.writer.clone()),
                            Some(Connection::Udp(c)) => Some(c.writer.clone()),
                            Some(Connection::Icmp(c)) => Some(c.writer.clone()),
                            Some(Connection::Connecting(_)) | None => None,
                        }
                    };
                    if let Some(sender) = sender {
                        sender.send(data).await.ok();
                    }
                }
                SlirpResponse::ActivateFastPath { conn_id, guest_addr, .. } => {
                    trace!("activating fast path for {guest_addr}");
                    if self.fast_path_enabled {
                        let writer = self.connections.lock().get(&conn_id).and_then(|c| {
                            if let Connection::Tcp(c) = c { Some(c.writer.clone()) } else { None }
                        });
                        if let Some(writer) = writer {
                            self.fast_path_connections.lock().insert(guest_addr, writer);
                        }
                    }
                }
                SlirpResponse::CloseConnection { conn_id, guest_addr } => {
                    info!("Closing connection {conn_id}");
                    let removed = self.connections.lock().remove(&conn_id);
                    if let Some(conn) = removed {
                        match conn {
                            Connection::Connecting(Some(handle)) => {
                                info!("Aborting connecting task for connection {conn_id}");
                                handle.abort();
                            }
                            Connection::Connecting(None) => {
                                info!(
                                    "Connecting connection {conn_id} closed before task handle registered"
                                );
                            }
                            Connection::Tcp(conn) => {
                                info!("Aborting reader task for TCP connection {conn_id}");
                                conn.reader_handle.abort();
                                info!("Aborting writer task for TCP connection {conn_id}");
                                conn.writer_handle.abort();
                                info!("TCP connection {conn_id} tasks aborted");
                            }
                            Connection::Icmp(conn) => {
                                info!("Aborting reader task for ICMP connection {conn_id}");
                                conn.reader_handle.abort();
                                info!("Aborting writer task for ICMP connection {conn_id}");
                                conn.writer_handle.abort();
                                info!("ICMP connection {conn_id} tasks aborted");
                            }
                            Connection::Udp(conn) => {
                                info!("Aborting reader task for UDP connection {conn_id}");
                                conn.reader_handle.abort();
                                info!("Aborting writer task for UDP connection {conn_id}");
                                conn.writer_handle.abort();
                                info!("UDP connection {conn_id} tasks aborted");
                            }
                        }
                    }
                    if self.fast_path_enabled {
                        trace!("deactivating fast path for {guest_addr}");
                        self.fast_path_connections.lock().remove(&guest_addr);
                    }
                    info!("Connection {conn_id} closed");
                }
                SlirpResponse::SetTimer(duration) => {
                    if let Some(timer_task) = self.timer_task.take() {
                        timer_task.abort();
                    }
                    let sender = self.slirp_request_sender.clone();
                    self.timer_task = Some(tokio::spawn(async move {
                        tokio::time::sleep(duration).await;
                        sender.send(SlirpRequest::Timer).await.ok();
                    }));
                }
                SlirpResponse::Reset => {
                    info!(
                        "Received Reset response from Slirp core. Tearing down all active connections."
                    );
                    let mut conns = self.connections.lock();
                    for (conn_id, conn) in conns.drain() {
                        info!("Aborting active connection {conn_id} due to Reset");
                        match conn {
                            Connection::Connecting(Some(handle)) => {
                                handle.abort();
                            }
                            Connection::Connecting(None) => {}
                            Connection::Tcp(tcp_conn) => {
                                tcp_conn.reader_handle.abort();
                                tcp_conn.writer_handle.abort();
                            }
                            Connection::Udp(udp_conn) => {
                                udp_conn.reader_handle.abort();
                                udp_conn.writer_handle.abort();
                            }
                            Connection::Icmp(icmp_conn) => {
                                icmp_conn.reader_handle.abort();
                                icmp_conn.writer_handle.abort();
                            }
                        }
                    }
                    self.fast_path_connections.lock().clear();
                }
                _ => {}
            }
        }
    }

    pub fn connection_count(&self) -> usize {
        self.connections.lock().len()
    }
}

#[derive(Clone)]
pub struct TokioHostHandle {
    sender: mpsc::Sender<SlirpRequest>,
}

impl TokioHostHandle {
    pub async fn save_state(&self) -> Result<Vec<u8>, &'static str> {
        // SlirpRequest::SaveState expects std::sync::mpsc::Sender because the core
        // slirp protocol engine is synchronous and decoupled from Tokio. Therefore,
        // we use std::sync::mpsc coupled with spawn_blocking here.
        let (tx, rx) = std::sync::mpsc::channel();
        self.sender
            .send(SlirpRequest::SaveState(tx))
            .await
            .map_err(|_e| "Failed to send SaveState request to Slirp task")?;

        let state_bytes = tokio::task::spawn_blocking(move || {
            rx.recv().map_err(|_e| "Failed to receive state from Slirp task")
        })
        .await
        .map_err(|_e| "Spawn blocking failed")??;

        Ok(state_bytes)
    }

    pub async fn restore_state(&self, state_bytes: &[u8]) -> Result<(), &'static str> {
        self.sender
            .send(SlirpRequest::RestoreState(Bytes::copy_from_slice(state_bytes)))
            .await
            .map_err(|_e| "Failed to send RestoreState request to Slirp task")?;

        Ok(())
    }
}

impl Drop for TokioHost {
    fn drop(&mut self) {
        info!("Dropping TokioHost. Cleaning up all background tasks.");
        if let Some(task) = self.slirp_task.take() {
            task.abort();
        }

        if let Some(timer_task) = self.timer_task.take() {
            timer_task.abort();
        }
        for task in self.hostfwd_tasks.drain(..) {
            task.abort();
        }
        let mut conns = self.connections.lock();
        for (conn_id, conn) in conns.drain() {
            match conn {
                Connection::Connecting(Some(handle)) => {
                    info!("Aborting connecting connection {conn_id} on drop");
                    handle.abort();
                }
                Connection::Connecting(None) => {}
                Connection::Tcp(tcp_conn) => {
                    info!("Aborting TCP connection {conn_id} on drop");
                    tcp_conn.reader_handle.abort();
                    tcp_conn.writer_handle.abort();
                }
                Connection::Udp(udp_conn) => {
                    info!("Aborting UDP connection {conn_id} on drop");
                    udp_conn.reader_handle.abort();
                    udp_conn.writer_handle.abort();
                }
                Connection::Icmp(icmp_conn) => {
                    info!("Aborting ICMP connection {conn_id} on drop");
                    icmp_conn.reader_handle.abort();
                    icmp_conn.writer_handle.abort();
                }
            }
        }
    }
}

async fn start_tcp_hostfwd_listener(
    rule: HostFwdRule,
    slirp_request_sender: mpsc::Sender<SlirpRequest>,
    connections: Arc<Mutex<HashMap<u64, Connection>>>,
    next_host_conn_id: Arc<AtomicU64>,
) {
    let listener = match TcpListener::bind(rule.host_addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind TCP hostfwd listener on {}: {}", rule.host_addr, e);
            return;
        }
    };
    info!("TCP hostfwd listener listening on {}", rule.host_addr);

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let conn_id = next_host_conn_id.fetch_add(1, Ordering::SeqCst);
                let slirp_request_sender = slirp_request_sender.clone();
                let task_connections = connections.clone();
                let guest_addr = rule.guest_addr;

                connections.lock().insert(conn_id, Connection::Connecting(None));
                let handle = tokio::spawn(async move {
                    // Notify Slirp core to accept the incoming connection and start handshake
                    slirp_request_sender
                        .send(SlirpRequest::AcceptIncoming {
                            conn_id,
                            host_addr: rule.host_addr,
                            guest_addr,
                        })
                        .await
                        .ok();

                    finish_connecting_tcp(conn_id, stream, slirp_request_sender, &task_connections);
                });
                attach_connecting_handle(conn_id, handle, &connections);
            }
            Err(e) => {
                error!("TCP hostfwd accept error on {}: {}", rule.host_addr, e);
            }
        }
    }
}

#[allow(dead_code)]
fn register_tcp_connection(
    stream: TcpStream,
    conn_id: u64,
    slirp_request_sender: mpsc::Sender<SlirpRequest>,
    connections: Arc<Mutex<HashMap<u64, Connection>>>,
) -> mpsc::Sender<Bytes> {
    let mut conns = connections.lock();
    register_tcp_connection_locked(stream, conn_id, slirp_request_sender, &mut conns)
}

fn finish_connecting_tcp(
    conn_id: u64,
    stream: TcpStream,
    slirp_request_sender: mpsc::Sender<SlirpRequest>,
    connections: &Mutex<HashMap<u64, Connection>>,
) {
    let mut conns = connections.lock();
    if let Some(Connection::Connecting(_)) = conns.get(&conn_id) {
        register_tcp_connection_locked(stream, conn_id, slirp_request_sender, &mut conns);
    } else {
        debug!("TCP connection {conn_id} was closed while connecting");
    }
}

fn attach_connecting_handle(
    conn_id: u64,
    handle: JoinHandle<()>,
    connections: &Mutex<HashMap<u64, Connection>>,
) {
    let mut conns = connections.lock();
    if let Some(Connection::Connecting(h)) = conns.get_mut(&conn_id) {
        *h = Some(handle);
    } else {
        handle.abort();
    }
}

fn register_tcp_connection_locked(
    stream: TcpStream,
    conn_id: u64,
    slirp_request_sender: mpsc::Sender<SlirpRequest>,
    conns: &mut HashMap<u64, Connection>,
) -> mpsc::Sender<Bytes> {
    let (mut reader, mut writer) = stream.into_split();
    let (tx, mut rx) = mpsc::channel::<Bytes>(1000);

    // Reader task
    let reader_slirp_request_sender = slirp_request_sender.clone();
    let reader_handle = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => {
                    reader_slirp_request_sender
                        .send(SlirpRequest::RemoteClosed(conn_id))
                        .await
                        .ok();
                    break;
                }
                Ok(n) => {
                    if let Some(slice) = buf.get(..n) {
                        reader_slirp_request_sender
                            .send(SlirpRequest::Data(conn_id, Bytes::copy_from_slice(slice)))
                            .await
                            .ok();
                    }
                }
                Err(_) => {
                    reader_slirp_request_sender
                        .send(SlirpRequest::ConnectionClosed(conn_id))
                        .await
                        .ok();
                    break;
                }
            }
        }
    });

    // Writer task
    let writer_handle = tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            if writer.write_all(&data).await.is_err() {
                break;
            }
        }
    });

    conns.insert(
        conn_id,
        Connection::Tcp(TcpConnection { writer: tx.clone(), reader_handle, writer_handle }),
    );

    tx
}

async fn connect_tcp(
    destination: SocketAddr,
    socks5_proxy: Option<SocketAddr>,
) -> std::io::Result<TcpStream> {
    if let Some(proxy_addr) = socks5_proxy {
        debug!("Connecting to SOCKS5 proxy at {proxy_addr} to reach {destination}");
        let mut stream = TcpStream::connect(proxy_addr).await?;
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            perform_socks5_handshake(&mut stream, destination),
        )
        .await
        .map_err(|_e| std::io::Error::other("SOCKS5 handshake timed out"))??;
        Ok(stream)
    } else {
        debug!("Connecting directly to {destination}");
        TcpStream::connect(destination).await
    }
}

async fn perform_socks5_handshake(
    stream: &mut TcpStream,
    destination: SocketAddr,
) -> std::io::Result<()> {
    // 1. Send greeting: VER=5, NMETHODS=1, METHODS=[0] (No Auth)
    stream.write_all(&[5, 1, 0]).await?;

    // 2. Read greeting response
    let mut greeting_resp = [0u8; 2];
    stream.read_exact(&mut greeting_resp).await?;
    let [ver, method] = greeting_resp;
    if ver != 5 || method != 0 {
        return Err(std::io::Error::other("SOCKS5 authentication negotiation failed"));
    }

    // 3. Send connection request: VER=5, CMD=1 (CONNECT), RSV=0
    let mut req = Vec::new();
    req.extend_from_slice(&[5, 1, 0]);

    match destination.ip() {
        IpAddr::V4(ip) => {
            req.push(1); // ATYP = IPv4
            req.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            req.push(4); // ATYP = IPv6
            req.extend_from_slice(&ip.octets());
        }
    }
    req.extend_from_slice(&destination.port().to_be_bytes());
    stream.write_all(&req).await?;

    // 4. Read response
    let mut resp_header = [0u8; 4];
    stream.read_exact(&mut resp_header).await?;
    let [ver, status, _rsv, atyp] = resp_header;
    if ver != 5 || status != 0 {
        return Err(std::io::Error::other(format!(
            "SOCKS5 connection failed with error code: {status}"
        )));
    }

    // Skip bound address and port in the response
    match atyp {
        1 => {
            let mut buf = [0u8; 6];
            stream.read_exact(&mut buf).await?;
        }
        4 => {
            let mut buf = [0u8; 18];
            stream.read_exact(&mut buf).await?;
        }
        3 => {
            let mut len_buf = [0u8; 1];
            stream.read_exact(&mut len_buf).await?;
            let [len_u8] = len_buf;
            let len = len_u8 as usize;
            let mut buf = vec![0u8; len + 2];
            stream.read_exact(&mut buf).await?;
        }
        _ => {
            return Err(std::io::Error::other("Unknown SOCKS5 address type in response"));
        }
    }

    Ok(())
}

fn register_udp_connection(
    conn_id: u64,
    conn_info: slirp::UdpConnectionArgs,
    slirp_request_sender: mpsc::Sender<SlirpRequest>,
    connections: Arc<Mutex<HashMap<u64, Connection>>>,
) -> std::io::Result<mpsc::Sender<Bytes>> {
    let bind_addr = match conn_info.destination {
        SocketAddr::V4(_) => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        SocketAddr::V6(_) => SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
    };
    let std_socket = std::net::UdpSocket::bind(bind_addr)?;
    std_socket.connect(conn_info.destination)?;
    std_socket.set_nonblocking(true)?;
    let tokio_socket = tokio::net::UdpSocket::from_std(std_socket)?;
    let socket = Arc::new(tokio_socket);

    let (tx, mut rx) = mpsc::channel::<Bytes>(100);

    // Writer task
    let writer_socket = socket.clone();
    let writer_handle = tokio::spawn(async move {
        while let Some(data) = rx.recv().await {
            writer_socket.send(&data).await.ok();
        }
    });

    // Reader task
    let reader_socket = socket.clone();
    let slirp_sender = slirp_request_sender.clone();
    let reader_handle = tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        loop {
            match reader_socket.recv(&mut buf).await {
                Ok(n) => {
                    if let Some(slice) = buf.get(..n) {
                        slirp_sender
                            .send(SlirpRequest::Data(conn_id, Bytes::copy_from_slice(slice)))
                            .await
                            .ok();
                    }
                }
                Err(_) => {
                    slirp_sender.send(SlirpRequest::ConnectionClosed(conn_id)).await.ok();
                    break;
                }
            }
        }
    });

    connections.lock().insert(
        conn_id,
        Connection::Udp(UdpConnection { writer: tx.clone(), reader_handle, writer_handle }),
    );

    Ok(tx)
}

fn register_icmp_connection(
    conn_id: u64,
    conn_info: slirp::IcmpConnectionArgs,
    slirp_request_sender: mpsc::Sender<SlirpRequest>,
    connections: Arc<Mutex<HashMap<u64, Connection>>>,
) -> std::io::Result<mpsc::Sender<Bytes>> {
    use std::net::{IpAddr, SocketAddr};

    use socket2::{Domain, Protocol, Socket, Type};

    // 1. Open ICMP socket
    let (domain, bind_addr) = match conn_info.destination {
        IpAddr::V4(_) => (Domain::IPV4, SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))),
        IpAddr::V6(_) => (Domain::IPV6, SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))),
    };
    let proto = match conn_info.destination {
        IpAddr::V4(_) => Protocol::from(1),  // IPPROTO_ICMP
        IpAddr::V6(_) => Protocol::from(58), // IPPROTO_ICMPV6
    };

    let socket = Socket::new(domain, Type::DGRAM, Some(proto))?;
    socket.set_nonblocking(true)?;
    socket.bind(&bind_addr.into())?;

    let std_socket: std::net::UdpSocket = socket.into();
    let tokio_socket = tokio::net::UdpSocket::from_std(std_socket)?;
    let socket = Arc::new(tokio_socket);

    // Get the kernel-allocated ID (local port)
    let local_addr = socket.local_addr()?;
    let kernel_id = local_addr.port();
    debug!("Opened ICMP socket with kernel_id: {kernel_id} for guest_id: {}", conn_info.guest_id);

    let (tx, mut rx) = mpsc::channel::<Bytes>(100);

    // 2. Spawn writer task
    let writer_socket = socket.clone();
    let dest_ip = conn_info.destination;
    let writer_handle = tokio::spawn(async move {
        let dest_addr = SocketAddr::new(dest_ip, 0); // Port is 0 for raw IP destinations in sendto
        while let Some(data) = rx.recv().await {
            writer_socket.send_to(&data, dest_addr).await.ok();
        }
    });

    // 3. Spawn reader task
    let reader_socket = socket.clone();
    let slirp_sender = slirp_request_sender.clone();
    let guest_id = conn_info.guest_id;
    let guest_ip = conn_info.guest_ip;
    let is_ipv6 = conn_info.destination.is_ipv6();
    let reader_handle = tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        loop {
            match reader_socket.recv_from(&mut buf).await {
                Ok((n, src_addr)) => {
                    let Some(slice) = buf.get(..n) else {
                        continue;
                    };
                    let mut packet_data = slice.to_vec();
                    // Rewrite the ICMP ID back to the guest's original ID only for Echo
                    // Reply/Request!
                    if let [icmp_type, _, c0, c1, id0, id1, _, _, ..] = packet_data.as_mut_slice()
                        && ((!is_ipv6 && matches!(*icmp_type, 0 | 8))
                            || (is_ipv6 && matches!(*icmp_type, 128 | 129)))
                    {
                        [*id0, *id1] = guest_id.to_be_bytes();
                        [*c0, *c1] = [0, 0];
                        let checksum = if !is_ipv6 {
                            Some(netsim_packets::ipv4_checksum(&packet_data))
                        } else if let (IpAddr::V6(src_v6), IpAddr::V6(dst_v6)) =
                            (src_addr.ip(), guest_ip)
                        {
                            Some(netsim_packets::icmpv6_checksum(&packet_data, src_v6, dst_v6))
                        } else {
                            None
                        };
                        if let (Some(csum), [_, _, c0, c1, ..]) =
                            (checksum, packet_data.as_mut_slice())
                        {
                            [*c0, *c1] = csum.to_be_bytes();
                        }
                    }

                    slirp_sender
                        .send(SlirpRequest::Data(conn_id, Bytes::from(packet_data)))
                        .await
                        .ok();
                }
                Err(_) => {
                    slirp_sender.send(SlirpRequest::ConnectionClosed(conn_id)).await.ok();
                    break;
                }
            }
        }
    });

    connections.lock().insert(
        conn_id,
        Connection::Icmp(IcmpConnection { writer: tx.clone(), reader_handle, writer_handle }),
    );

    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_attach_connecting_handle_active() {
        let connections = Arc::new(Mutex::new(HashMap::new()));
        let conn_id = 100;
        connections.lock().insert(conn_id, Connection::Connecting(None));

        let handle = tokio::spawn(async {});
        attach_connecting_handle(conn_id, handle, &connections);

        let conns = connections.lock();
        assert!(matches!(conns.get(&conn_id), Some(Connection::Connecting(Some(_)))));
    }

    #[tokio::test]
    async fn test_attach_connecting_handle_aborts_when_closed() {
        let connections = Arc::new(Mutex::new(HashMap::new()));
        let conn_id = 101;
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });
        let abort_handle = handle.abort_handle();
        attach_connecting_handle(conn_id, handle, &connections);

        tokio::task::yield_now().await;
        assert!(abort_handle.is_finished());
    }

    #[tokio::test]
    async fn test_finish_connecting_tcp_when_closed() {
        let connections = Arc::new(Mutex::new(HashMap::new()));
        let (slirp_tx, _slirp_rx) = mpsc::channel(10);
        let conn_id = 102;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let stream = TcpStream::connect(addr).await.expect("connect tcp");

        finish_connecting_tcp(conn_id, stream, slirp_tx, &connections);

        assert!(!connections.lock().contains_key(&conn_id));
    }

    #[tokio::test]
    async fn test_finish_connecting_tcp_when_connecting() {
        let connections = Arc::new(Mutex::new(HashMap::new()));
        let (slirp_tx, _slirp_rx) = mpsc::channel(10);
        let conn_id = 103;
        connections.lock().insert(conn_id, Connection::Connecting(None));

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let client_task = tokio::spawn(async move {
            let (server_stream, _) = listener.accept().await.expect("accept");
            server_stream
        });
        let client_stream = TcpStream::connect(addr).await.expect("connect");
        let _server_stream = client_task.await.expect("task join");

        finish_connecting_tcp(conn_id, client_stream, slirp_tx, &connections);

        let conns = connections.lock();
        assert!(matches!(conns.get(&conn_id), Some(Connection::Tcp(_))));
    }
}
