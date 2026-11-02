// Copyright 2024 The Android Open Source Project
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
    net::SocketAddr,
    sync::{Arc, mpsc},
    thread,
};

use bytes::Bytes;
use parking_lot::Mutex;
use slirp::{ProxyConnect, ProxyManager};
use tokio::{
    runtime::Runtime,
    task::{AbortHandle, JoinSet},
};
use tracing::{debug, warn};

use crate::{
    Connector, DnsManager, Result,
    util::{ProxyConfig, into_raw_descriptor},
};

#[derive(Default)]
struct ConnectTasks {
    set: JoinSet<()>,
    next_generation: u64,
    abort_handles: HashMap<usize, (u64, AbortHandle)>,
}

impl ConnectTasks {
    fn reap_completed(&mut self) {
        while self.set.try_join_next().is_some() {}
        self.abort_handles.retain(|_, (_, handle)| !handle.is_finished());
    }

    fn claim(&mut self, connect_id: usize, generation: u64) -> bool {
        if matches!(self.abort_handles.get(&connect_id), Some(&(g, _)) if g == generation) {
            self.abort_handles.remove(&connect_id);
            true
        } else {
            false
        }
    }
}

/// # Manager
///
/// The `Manager` struct implements the `ProxyManager` trait from
/// `slirp`.  It is responsible for managing TCP connections
/// through an HTTP proxy using the `Connector` struct.
///
/// The `Manager` uses a `tokio::runtime::Runtime` to spawn tasks for
/// establishing proxy connections.  It takes a proxy configuration
/// string as input, which is parsed into a `ProxyConfig` to create a
/// `Connector` instance.
///
/// The `try_connect` method attempts to establish a connection to the
/// given `SocketAddr` through the proxy.  If successful, it calls the
/// `proxy_connect_stream` method of `ProxyConnect` with the owned
/// `std::net::TcpStream` when `accepts_stream()` is `true`, or the
/// `proxy_connect` method with the raw file descriptor otherwise.
///
/// # Example
///
/// ```
/// use std::net::SocketAddr;
///
/// use slirp::ProxyConnect;
///
/// struct MyProxyConnect;
///
/// impl ProxyConnect for MyProxyConnect {
///     fn proxy_connect(&self, fd: i32, sockaddr: SocketAddr) {
///         // Handle the connected socket
///     }
/// }
///
/// #[tokio::main]
/// async fn main() {}
/// ```
pub struct Manager {
    runtime: Option<Runtime>,
    connector: Connector,
    tasks: Arc<Mutex<ConnectTasks>>,
}

impl Manager {
    /// Creates a new `Manager` instance to handle HTTP proxy connections.
    ///
    /// This function initializes the proxy configuration, creates a
    /// `DnsManager` for IP-to-FQDN reverse lookup caching, and spawns a
    /// background thread to capture and process ethernet traffic for DNS
    /// caching.
    pub fn new(proxy: &str, rx_proxy_bytes: mpsc::Receiver<Bytes>) -> Result<Self> {
        let config = ProxyConfig::from_string(proxy)?;
        let dns_manager = Arc::new(DnsManager::new());

        let _ = thread::Builder::new().name("Dns Manager".to_string()).spawn(move || {
            while let Ok(bytes) = rx_proxy_bytes.recv() {
                dns_manager.add_from_ethernet_slice(&bytes);
            }
        });

        // We initialize a private, isolated tokio runtime for HTTP proxy tasks.
        // Sharing the main daemon runtime with high-throughput actors (like
        // Slirp or Bluetooth) can cause scheduling delays for HTTP
        // reachability checks, leading to PARTIAL_CONNECTIVITY status.
        // Isolation ensures consistent performance.
        let runtime = Some(Runtime::new()?);

        Ok(Self {
            runtime,
            connector: config.into_connector(),
            tasks: Arc::new(Mutex::new(ConnectTasks::default())),
        })
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        let tasks = std::mem::take(&mut *self.tasks.lock());
        drop(tasks);
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl ProxyManager for Manager {
    /// Attempts to establish a TCP connection to the given `sockaddr` through
    /// the proxy.
    ///
    /// This function spawns a new task in the `tokio` runtime to handle the
    /// connection process. If the connection is successful, it calls
    /// `proxy_connect_stream` with the connected `std::net::TcpStream` when
    /// `accepts_stream()` is `true`, or `proxy_connect` with the raw file
    /// descriptor of the connected socket otherwise.
    ///
    /// # Arguments
    ///
    /// * `sockaddr` - The target socket address to connect to.
    /// * `connect_id` - An identifier for the connection.
    /// * `connect_func` - A `ProxyConnect` object that will be called with the
    ///   connected socket.
    ///
    /// # Returns
    ///
    /// `true` if the connection attempt was initiated, `false` otherwise.
    fn try_connect(
        &self,
        sockaddr: SocketAddr,
        connect_id: usize,
        connect_func: Box<dyn ProxyConnect + Send>,
    ) -> bool {
        let Some(runtime) = &self.runtime else {
            return false;
        };
        debug!("Connecting to {sockaddr:?} with connect ID {connect_id}");
        let connector = self.connector.clone();
        let tasks_ref = Arc::clone(&self.tasks);

        let replaced_handle = {
            let mut tasks = self.tasks.lock();
            tasks.reap_completed();
            let task_gen = tasks.next_generation;
            tasks.next_generation = tasks.next_generation.wrapping_add(1);
            let abort_handle = tasks.set.spawn_on(
                async move {
                    let connected = connector.connect(sockaddr).await;
                    let claimed = {
                        let mut tasks = tasks_ref.lock();
                        tasks.claim(connect_id, task_gen)
                    };
                    if !claimed {
                        return;
                    }
                    if connect_func.accepts_stream() {
                        let result = connected
                            .and_then(|stream| {
                                // Hand off a blocking stream, matching `into_raw_descriptor`.
                                let std_stream =
                                    stream.into_std().map_err(crate::Error::IoError)?;
                                std_stream.set_nonblocking(false).map_err(crate::Error::IoError)?;
                                Ok(std_stream)
                            })
                            .map_err(|error| {
                                warn!(%error, "Failed to connect to proxy {sockaddr}");
                                match error {
                                    crate::Error::IoError(io_error) => io_error,
                                    other => std::io::Error::other(other),
                                }
                            });
                        connect_func.proxy_connect_stream(result, sockaddr);
                    } else {
                        let fd = match connected.and_then(into_raw_descriptor) {
                            Ok(fd) => fd,
                            Err(error) => {
                                warn!(%error, "Failed to connect to proxy {sockaddr}");
                                -1
                            }
                        };
                        connect_func.proxy_connect(fd, sockaddr);
                    }
                },
                runtime.handle(),
            );
            tasks.abort_handles.insert(connect_id, (task_gen, abort_handle)).map(|(_, h)| h)
        };
        if let Some(prev) = replaced_handle {
            prev.abort();
        }

        true
    }

    /// Removes a connection with the given `connect_id`.
    ///
    /// # Arguments
    ///
    /// * `connect_id` - The identifier of the connection to remove.
    fn remove(&self, connect_id: usize) {
        debug!("Remove connect ID {}", connect_id);
        let removed_handle = {
            let mut tasks = self.tasks.lock();
            tasks.reap_completed();
            tasks.abort_handles.remove(&connect_id).map(|(_, h)| h)
        };
        if let Some(handle) = removed_handle {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Read},
        net::TcpStream,
        time::{Duration, Instant},
    };

    use tokio::{
        io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
        sync::oneshot,
        task::JoinHandle,
        time::timeout,
    };

    use super::*;

    #[tokio::test]
    async fn test_manager_drop_in_async_context() {
        let (_tx, rx) = mpsc::channel();
        let manager = Manager::new("http://127.0.0.1:8080", rx).expect("valid proxy manager");
        drop(manager);
    }

    type StreamOutcome = (io::Result<TcpStream>, SocketAddr);
    type RawFdOutcome = (i32, SocketAddr);

    struct StreamProxyConnect {
        sender: Mutex<Option<oneshot::Sender<StreamOutcome>>>,
    }

    impl StreamProxyConnect {
        fn new(sender: oneshot::Sender<StreamOutcome>) -> Self {
            Self { sender: Mutex::new(Some(sender)) }
        }
    }

    impl ProxyConnect for StreamProxyConnect {
        fn proxy_connect(&self, _fd: i32, _addr: SocketAddr) {
            panic!("proxy_connect should not be called when accepts_stream is true");
        }

        fn accepts_stream(&self) -> bool {
            true
        }

        fn proxy_connect_stream(&self, result: io::Result<TcpStream>, addr: SocketAddr) {
            let sender = self.sender.lock().take();
            if let Some(sender) = sender {
                let _ = sender.send((result, addr));
            }
        }
    }

    struct RawFdProxyConnect {
        sender: Mutex<Option<oneshot::Sender<RawFdOutcome>>>,
    }

    impl RawFdProxyConnect {
        fn new(sender: oneshot::Sender<RawFdOutcome>) -> Self {
            Self { sender: Mutex::new(Some(sender)) }
        }
    }

    impl ProxyConnect for RawFdProxyConnect {
        fn proxy_connect(&self, fd: i32, addr: SocketAddr) {
            let sender = self.sender.lock().take();
            if let Some(sender) = sender {
                let _ = sender.send((fd, addr));
            }
        }
    }

    async fn setup_mock_proxy(response: &'static [u8]) -> (Manager, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let proxy_addr = listener.local_addr().expect("local addr");
        let proxy_task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 1024];
            let _n = socket.read(&mut buf).await.expect("read connect request");
            socket.write_all(response).await.expect("write response");
        });
        let (_tx, rx) = mpsc::channel();
        let manager =
            Manager::new(&format!("http://{proxy_addr}"), rx).expect("valid proxy manager");
        (manager, proxy_task)
    }

    #[tokio::test]
    async fn test_try_connect_stream_success() {
        let (manager, proxy_task) =
            setup_mock_proxy(b"HTTP/1.1 200 Connection established\r\n\r\n").await;
        let (tx, rx) = oneshot::channel();
        let stream_connect = StreamProxyConnect::new(tx);
        let target_addr: SocketAddr = "192.0.2.1:443".parse().expect("target addr");

        assert!(manager.try_connect(target_addr, 1, Box::new(stream_connect)));

        let (result, addr) = rx.await.expect("oneshot receive");
        assert_eq!(addr, target_addr);
        assert!(result.is_ok(), "Expected Ok(TcpStream), got {result:?}");

        proxy_task.await.expect("proxy task");
    }

    #[tokio::test]
    async fn test_try_connect_stream_delivered_stream_is_blocking() {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let proxy_addr = listener.local_addr().expect("local addr");
        let proxy_task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 1024];
            let _n = socket.read(&mut buf).await.expect("read connect request");
            socket
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .expect("write response");
            // Hold the connection open so the client can verify blocking read
            // without encountering immediate EOF.
            let _ = shutdown_rx.await;
        });

        let (_tx, rx) = mpsc::channel();
        let manager =
            Manager::new(&format!("http://{proxy_addr}"), rx).expect("valid proxy manager");
        let (tx, rx_stream) = oneshot::channel();
        let stream_connect = StreamProxyConnect::new(tx);
        let target_addr: SocketAddr = "192.0.2.1:443".parse().expect("target addr");

        assert!(manager.try_connect(target_addr, 1, Box::new(stream_connect)));

        let (result, addr) = rx_stream.await.expect("oneshot receive");
        assert_eq!(addr, target_addr);
        let mut std_stream = result.expect("connected stream");

        // The handed-off std::net::TcpStream must be in blocking mode per the
        // contract with ProxyConnect. We assert this by configuring a read
        // timeout and verifying that reading an empty stream blocks until the
        // timeout expires rather than returning EAGAIN/WouldBlock immediately.
        // Checking only a lower bound on elapsed time ensures the test is
        // robust against scheduling jitter on loaded CI machines.
        let (read_result, elapsed) = timeout(
            Duration::from_secs(2),
            tokio::task::spawn_blocking(move || {
                std_stream
                    .set_read_timeout(Some(Duration::from_millis(200)))
                    .expect("set read timeout");
                let mut buf = [0u8; 16];
                let start = Instant::now();
                let res = std_stream.read(&mut buf);
                (res, start.elapsed())
            }),
        )
        .await
        .expect("read should complete within outer timeout")
        .expect("spawn_blocking");

        let err = read_result.expect_err("read on empty blocking stream with timeout must fail");
        assert!(
            err.kind() == io::ErrorKind::WouldBlock || err.kind() == io::ErrorKind::TimedOut,
            "Expected WouldBlock or TimedOut, got {err:?}"
        );
        assert!(
            elapsed >= Duration::from_millis(150),
            "Expected elapsed >= 150ms for blocking read timeout, but took {elapsed:?}"
        );

        let _ = shutdown_tx.send(());
        proxy_task.await.expect("proxy task");
    }

    #[tokio::test]
    async fn test_try_connect_stream_failure() {
        let (manager, proxy_task) = setup_mock_proxy(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
        let (tx, rx) = oneshot::channel();
        let stream_connect = StreamProxyConnect::new(tx);
        let target_addr: SocketAddr = "192.0.2.1:443".parse().expect("target addr");

        assert!(manager.try_connect(target_addr, 1, Box::new(stream_connect)));

        let (result, addr) = rx.await.expect("oneshot receive");
        assert_eq!(addr, target_addr);
        assert_eq!(result.expect_err("expected 502 error").kind(), io::ErrorKind::Other);

        proxy_task.await.expect("proxy task");
    }

    #[tokio::test]
    async fn test_try_connect_stream_io_error_preserves_kind() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let proxy_addr = listener.local_addr().expect("local addr");
        drop(listener);

        let (_tx, rx) = mpsc::channel();
        let manager =
            Manager::new(&format!("http://{proxy_addr}"), rx).expect("valid proxy manager");
        let (tx, rx_stream) = oneshot::channel();
        let stream_connect = StreamProxyConnect::new(tx);
        let target_addr: SocketAddr = "192.0.2.1:443".parse().expect("target addr");

        assert!(manager.try_connect(target_addr, 1, Box::new(stream_connect)));

        let (result, addr) = rx_stream.await.expect("oneshot receive");
        assert_eq!(addr, target_addr);
        assert_eq!(
            result.expect_err("expected connect error").kind(),
            io::ErrorKind::ConnectionRefused
        );
    }

    #[tokio::test]
    async fn test_try_connect_raw_fd_success() {
        let (manager, proxy_task) =
            setup_mock_proxy(b"HTTP/1.1 200 Connection established\r\n\r\n").await;
        let (tx, rx) = oneshot::channel();
        let raw_connect = RawFdProxyConnect::new(tx);
        let target_addr: SocketAddr = "192.0.2.1:443".parse().expect("target addr");

        assert!(manager.try_connect(target_addr, 1, Box::new(raw_connect)));

        let (fd, addr) = rx.await.expect("oneshot receive");
        assert_eq!(addr, target_addr);
        assert!(fd >= 0, "Expected valid fd, got {fd}");

        proxy_task.await.expect("proxy task");
    }

    #[tokio::test]
    async fn test_try_connect_raw_fd_failure() {
        let (manager, proxy_task) = setup_mock_proxy(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
        let (tx, rx) = oneshot::channel();
        let raw_connect = RawFdProxyConnect::new(tx);
        let target_addr: SocketAddr = "192.0.2.1:443".parse().expect("target addr");

        assert!(manager.try_connect(target_addr, 1, Box::new(raw_connect)));

        let (fd, addr) = rx.await.expect("oneshot receive");
        assert_eq!(addr, target_addr);
        assert_eq!(fd, -1, "Expected -1 for failure");

        proxy_task.await.expect("proxy task");
    }

    #[tokio::test]
    async fn test_remove_aborts_in_flight_connect_task() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let proxy_addr = listener.local_addr().expect("local addr");
        let (conn_established_tx, conn_established_rx) = oneshot::channel();

        let proxy_task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept");
            let mut reader = BufReader::new(socket);
            let mut line = String::new();
            while reader.read_line(&mut line).await.expect("read header line") > 0 {
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            conn_established_tx.send(()).expect("signal request received");
            let mut remainder = Vec::new();
            assert_eq!(reader.read_to_end(&mut remainder).await.expect("read to end"), 0);
        });

        let (_tx, rx_bytes) = mpsc::channel();
        let manager =
            Manager::new(&format!("http://{proxy_addr}"), rx_bytes).expect("valid proxy manager");
        let (tx, rx) = oneshot::channel();
        let stream_connect = StreamProxyConnect::new(tx);
        let target_addr: SocketAddr = "192.0.2.1:443".parse().expect("target addr");

        assert!(manager.try_connect(target_addr, 7, Box::new(stream_connect)));
        conn_established_rx.await.expect("proxy received request headers");

        manager.remove(7);

        timeout(Duration::from_secs(1), async {
            assert!(rx.await.is_err());
            proxy_task.await.expect("proxy task completes on EOF");
        })
        .await
        .expect("connection task should be aborted within 1s");
    }

    #[tokio::test]
    async fn test_drop_aborts_in_flight_connect_tasks() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let proxy_addr = listener.local_addr().expect("local addr");
        let (conn_established_tx, conn_established_rx) = oneshot::channel();

        let proxy_task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept");
            let mut reader = BufReader::new(socket);
            let mut line = String::new();
            while reader.read_line(&mut line).await.expect("read header line") > 0 {
                if line == "\r\n" {
                    break;
                }
                line.clear();
            }
            conn_established_tx.send(()).expect("signal request received");
            let mut remainder = Vec::new();
            assert_eq!(reader.read_to_end(&mut remainder).await.expect("read to end"), 0);
        });

        let (_tx, rx_bytes) = mpsc::channel();
        let manager =
            Manager::new(&format!("http://{proxy_addr}"), rx_bytes).expect("valid proxy manager");
        let (tx, rx) = oneshot::channel();
        let stream_connect = StreamProxyConnect::new(tx);
        let target_addr: SocketAddr = "192.0.2.1:443".parse().expect("target addr");

        assert!(manager.try_connect(target_addr, 7, Box::new(stream_connect)));
        conn_established_rx.await.expect("proxy received request headers");

        drop(manager);

        timeout(Duration::from_secs(1), async {
            assert!(rx.await.is_err());
            proxy_task.await.expect("proxy task completes on EOF");
        })
        .await
        .expect("connection task should be aborted on manager drop within 1s");
    }

    #[tokio::test]
    async fn test_reused_connect_id_aborts_previous_in_flight_task() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let proxy_addr = listener.local_addr().expect("local addr");
        let (conn1_ready_tx, conn1_ready_rx) = oneshot::channel();

        let proxy_task = tokio::spawn(async move {
            let (socket1, _) = listener.accept().await.expect("accept conn 1");
            let mut reader1 = BufReader::new(socket1);
            let mut line1 = String::new();
            while reader1.read_line(&mut line1).await.expect("read line 1") > 0 {
                if line1 == "\r\n" {
                    break;
                }
                line1.clear();
            }
            conn1_ready_tx.send(()).expect("signal conn 1 ready");

            let (socket2, _) = listener.accept().await.expect("accept conn 2");
            let mut reader2 = BufReader::new(socket2);
            let mut line2 = String::new();
            while reader2.read_line(&mut line2).await.expect("read line 2") > 0 {
                if line2 == "\r\n" {
                    break;
                }
                line2.clear();
            }
            let mut socket2 = reader2.into_inner();
            socket2
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .expect("write response 2");

            let mut remainder1 = Vec::new();
            assert_eq!(reader1.read_to_end(&mut remainder1).await.expect("read to end 1"), 0);
        });

        let (_tx, rx_bytes) = mpsc::channel();
        let manager =
            Manager::new(&format!("http://{proxy_addr}"), rx_bytes).expect("valid proxy manager");
        let (tx1, rx1) = oneshot::channel();
        let (tx2, rx2) = oneshot::channel();
        let target_addr: SocketAddr = "192.0.2.1:443".parse().expect("target addr");

        assert!(manager.try_connect(target_addr, 42, Box::new(StreamProxyConnect::new(tx1))));
        conn1_ready_rx.await.expect("conn 1 ready");

        assert!(manager.try_connect(target_addr, 42, Box::new(StreamProxyConnect::new(tx2))));

        timeout(Duration::from_secs(1), async {
            assert!(rx1.await.is_err(), "displaced task 1 should be aborted");
            let (res2, addr2) = rx2.await.expect("replacement connect result");
            assert_eq!(addr2, target_addr);
            assert!(res2.is_ok(), "Expected Ok(TcpStream) for conn 2, got {res2:?}");
            proxy_task.await.expect("proxy task completes");
        })
        .await
        .expect("connect id replacement should complete within 1s");
    }

    #[tokio::test]
    async fn test_completed_tasks_are_reaped_on_subsequent_calls() {
        let (manager, proxy_task) =
            setup_mock_proxy(b"HTTP/1.1 200 Connection established\r\n\r\n").await;
        let (tx, rx) = oneshot::channel();
        let stream_connect = StreamProxyConnect::new(tx);
        let target_addr: SocketAddr = "192.0.2.1:443".parse().expect("target addr");

        assert!(manager.try_connect(target_addr, 1, Box::new(stream_connect)));
        let (res, addr) = rx.await.expect("oneshot receive");
        assert_eq!(addr, target_addr);
        assert!(res.is_ok(), "Expected Ok(TcpStream), got {res:?}");
        proxy_task.await.expect("proxy task");

        let mut reaped = false;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            manager.remove(999);
            let is_empty = {
                let tasks = manager.tasks.lock();
                tasks.set.is_empty() && tasks.abort_handles.is_empty()
            };
            if is_empty {
                reaped = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(reaped, "Completed tasks should be reaped from JoinSet and abort_handles");
    }
}
