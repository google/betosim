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
    collections::VecDeque,
    io,
    os::unix::io::{AsFd, AsRawFd, FromRawFd, OwnedFd},
    pin::Pin,
    str::FromStr,
    task::{Context, Poll},
};

use async_trait::async_trait;
#[cfg(all(not(test), feature = "cuttlefish"))]
use command_fds::inherited::take_fd_ownership;
use futures::{SinkExt, ready, stream::StreamExt};
use netsim_types::{Chip, ChipInfo, ChipKind, DeviceInfo};
use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    sys::socket::{SockFlag, accept4, getsockopt, sockopt},
    unistd::{read, write},
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf, unix::AsyncFd};
use tokio_util::codec::FramedRead;

use crate::{
    error::{PacketStreamError, Result},
    transport::{
        H4Codec, ModemCodec, NciCodec, UciCodec,
        traits::{PacketSink, PacketStream, TransportListener},
    },
    types::StreamAddress,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    #[serde(default)]
    pub name: Option<String>,
    pub chips: Vec<ChipConfig>,
    #[serde(default)]
    pub device_info: Option<DeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChipConfig {
    pub kind: String,
    /// Input to the device (netsim output). Supports aliases "vsockFd" and
    /// "virtioFd".
    #[serde(rename = "fdIn", alias = "vsockFd", alias = "virtioFd")]
    pub fd_in: i32,
    #[serde(rename = "fdOut")]
    pub fd_out: Option<i32>,
    pub model: Option<String>,
    #[serde(rename = "simType")]
    pub sim_type: Option<i32>,
    #[serde(rename = "simProfile")]
    pub sim_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualFdConfig {
    pub devices: Vec<DeviceConfig>,
}

#[derive(Debug)]
pub struct DualFdChipEndpoints {
    pub kind: String,
    /// Input to the device (netsim writes to it).
    pub in_fd: OwnedFd,
    /// Output from the device (netsim reads from it). When `None`, `in_fd` is
    /// bidirectional; must be `None` when `in_fd` is a listening socket.
    pub out_fd: Option<OwnedFd>,
    pub sim_type: Option<i32>,
    pub sim_profile: Option<String>,
}

#[derive(Debug)]
pub struct DualFdDeviceEndpoints {
    pub name: Option<String>,
    pub chips: Vec<DualFdChipEndpoints>,
    pub device_info: Option<DeviceInfo>,
}

#[cfg(all(not(test), feature = "cuttlefish"))]
fn claim_fd(fd: i32) -> Result<OwnedFd> {
    take_fd_ownership(fd)
        .map_err(|e| PacketStreamError::InvalidConfig(format!("Failed to claim FD {fd}: {e}")))
}

#[cfg(all(not(test), not(feature = "cuttlefish")))]
fn claim_fd(fd: i32) -> Result<OwnedFd> {
    Err(PacketStreamError::InvalidConfig(format!(
        "Claiming FD {fd} is not supported without cuttlefish feature"
    )))
}

#[cfg(test)]
fn claim_fd(fd: i32) -> Result<OwnedFd> {
    // SAFETY: In test configurations, we assume the provided raw file descriptor is
    // valid and that ownership can be safely transferred to OwnedFd.
    // WARNING: This transfers ownership, so the FD will be closed
    // when dropped. Do not pass process-wide FDs (like 0, 1, 2)
    // unless you intend to close them.
    unsafe { Ok(OwnedFd::from_raw_fd(fd)) }
}

fn is_listening_socket<T: AsFd>(fd: &T) -> bool {
    getsockopt(fd, sockopt::AcceptConn).unwrap_or(false)
}

fn set_nonblocking<T: AsFd>(fd: &T) -> io::Result<()> {
    let flags_int = fcntl(fd, FcntlArg::F_GETFL).map_err(io::Error::from)?;
    let flags = OFlag::from_bits_truncate(flags_int);
    fcntl(fd, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK)).map_err(io::Error::from)?;
    Ok(())
}

struct AsyncGenericFd(AsyncFd<OwnedFd>);

impl AsyncGenericFd {
    fn new(fd: OwnedFd) -> Result<Self> {
        // SAFETY: `fd` is an exclusively owned `OwnedFd` moved into `AsyncFd`; `AsyncFd::drop`
        // deregisters it before `OwnedFd::drop` closes it, and `AsyncGenericFd` never calls
        // `get_mut()` or `into_inner()`.
        let async_fd =
            unsafe { AsyncFd::register(fd) }.map_err(|e| PacketStreamError::Io(e.into()))?;
        Ok(Self(async_fd))
    }
}

impl AsyncRead for AsyncGenericFd {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        loop {
            let mut guard = ready!(this.0.poll_read_ready(cx))?;
            let unfilled = buf.initialize_unfilled();

            match guard.try_io(|inner| read(inner, unfilled).map_err(io::Error::from)) {
                Ok(Ok(bytes_read)) => {
                    buf.advance(bytes_read);
                    return Poll::Ready(Ok(()));
                }
                Ok(Err(err)) => return Poll::Ready(Err(err)),
                Err(_would_block) => continue,
            }
        }
    }
}

impl AsyncWrite for AsyncGenericFd {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.as_mut().get_mut();
        loop {
            let mut guard = ready!(this.0.poll_write_ready(cx))?;

            match guard.try_io(|inner| write(inner, buf).map_err(io::Error::from)) {
                Ok(Ok(bytes_written)) => return Poll::Ready(Ok(bytes_written)),
                Ok(Err(err)) => return Poll::Ready(Err(err)),
                Err(_would_block) => continue,
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

pub struct DualFdListener {
    num_devices: usize,
    num_chips: usize,
    /// Streams established from pre-existing FDs (e.g., inherited pipes) that
    /// do not need to accept connections dynamically.
    pending_static_streams: VecDeque<StaticStreamEntry>,
    listening_sockets: Vec<ListeningSocketEntry>,
}

type StaticStreamEntry = (DeviceInfo, String, Option<i32>, Option<String>, (OwnedFd, OwnedFd));
type ListeningSocketEntry = (DeviceInfo, String, Option<i32>, Option<String>, AsyncFd<OwnedFd>);

fn resolve_device_info(name: Option<&str>, device_info: Option<DeviceInfo>) -> DeviceInfo {
    let mut info = device_info.unwrap_or_default();
    if let Some(name) = name.filter(|_| info.name.is_empty()) {
        info.name = name.to_string();
    }
    if info.id.is_empty() {
        info.id.clone_from(&info.name);
    }
    info
}

impl DualFdListener {
    pub fn new(config: DualFdConfig) -> Result<Self> {
        let num_devices = config.devices.len();
        let num_chips = config.devices.iter().map(|d| d.chips.len()).sum();
        let mut listener = Self {
            num_devices,
            num_chips,
            pending_static_streams: VecDeque::new(),
            listening_sockets: Vec::new(),
        };
        listener.prepare_streams(&config)?;
        Ok(listener)
    }

    /// Creates a [`DualFdListener`] from pre-claimed device and chip endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - [`PacketStreamError::InvalidConfig`]: a listening socket specifies
    ///   `out_fd: Some(_)`, or `AsyncFd::register` fails to register the socket
    ///   with the Tokio reactor.
    /// - [`PacketStreamError::Io`]: setting `O_NONBLOCK` on a listening socket
    ///   fails, or `OwnedFd::try_clone` fails for a bidirectional non-listening
    ///   `in_fd` when `out_fd` is `None`.
    ///
    /// # Panics
    ///
    /// Panics if any endpoint is a listening socket and this function is called
    /// outside an active Tokio runtime.
    pub fn try_new_with_owned_fds(devices: Vec<DualFdDeviceEndpoints>) -> Result<Self> {
        let num_devices = devices.len();
        let num_chips = devices.iter().map(|d| d.chips.len()).sum();
        let mut listener = Self {
            num_devices,
            num_chips,
            pending_static_streams: VecDeque::new(),
            listening_sockets: Vec::new(),
        };
        for device in devices {
            let device_info = resolve_device_info(device.name.as_deref(), device.device_info);
            for chip in device.chips {
                listener.add_stream_endpoints(
                    device_info.clone(),
                    chip.kind,
                    chip.sim_type,
                    chip.sim_profile,
                    chip.in_fd,
                    chip.out_fd,
                )?;
            }
        }
        Ok(listener)
    }

    fn prepare_streams(&mut self, config: &DualFdConfig) -> Result<()> {
        self.pending_static_streams.clear();
        self.listening_sockets.clear();
        for device in &config.devices {
            let device_info =
                resolve_device_info(device.name.as_deref(), device.device_info.clone());
            for chip in &device.chips {
                let in_fd = claim_fd(chip.fd_in)?;
                let out_fd = if is_listening_socket(&in_fd) {
                    None
                } else {
                    chip.fd_out.filter(|&fd| fd != chip.fd_in).map(claim_fd).transpose()?
                };
                self.add_stream_endpoints(
                    device_info.clone(),
                    chip.kind.clone(),
                    chip.sim_type,
                    chip.sim_profile.clone(),
                    in_fd,
                    out_fd,
                )?;
            }
        }
        Ok(())
    }

    fn add_stream_endpoints(
        &mut self,
        device_info: DeviceInfo,
        kind: String,
        sim_type: Option<i32>,
        sim_profile: Option<String>,
        in_fd: OwnedFd,
        out_fd: Option<OwnedFd>,
    ) -> Result<()> {
        if is_listening_socket(&in_fd) {
            if out_fd.is_some() {
                return Err(PacketStreamError::InvalidConfig(format!(
                    "Listening socket endpoint for chip {kind} cannot specify out_fd"
                )));
            }
            set_nonblocking(&in_fd)?;
            // SAFETY: `in_fd` is an exclusively owned `OwnedFd` moved into `AsyncFd`;
            // `AsyncFd::drop` deregisters it before `OwnedFd::drop` closes it, and
            // `DualFdListener` never calls `get_mut()` or `into_inner()`.
            let async_fd = unsafe { AsyncFd::register(in_fd) }.map_err(|e| {
                PacketStreamError::InvalidConfig(format!(
                    "Failed to create AsyncFd for listening socket: {e}"
                ))
            })?;
            self.listening_sockets.push((device_info, kind, sim_type, sim_profile, async_fd));
        } else {
            let out_fd = match out_fd {
                Some(fd) => fd,
                None => in_fd.try_clone().map_err(PacketStreamError::Io)?,
            };

            self.pending_static_streams.push_back((
                device_info,
                kind,
                sim_type,
                sim_profile,
                (in_fd, out_fd),
            ));
        }
        Ok(())
    }
}

async fn accept_one_connection(async_fd: &AsyncFd<OwnedFd>) -> Result<(OwnedFd, OwnedFd)> {
    loop {
        let mut guard = async_fd.readable().await.map_err(PacketStreamError::Io)?;

        match guard.try_io(|inner| {
            accept4(inner.as_raw_fd(), SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK)
                .map_err(io::Error::from)
        }) {
            Ok(Ok(connected_raw_fd)) => {
                // SAFETY: accept4 successfully returned a new file descriptor. We take
                // ownership of this descriptor to ensure it is closed when dropped.
                let connected_fd = unsafe { OwnedFd::from_raw_fd(connected_raw_fd) };
                let cloned_fd = connected_fd.try_clone().map_err(PacketStreamError::Io)?;
                return Ok((connected_fd, cloned_fd));
            }
            Ok(Err(err)) => return Err(PacketStreamError::Io(err)),
            Err(_would_block) => continue,
        }
    }
}

fn create_stream_and_sink(
    device_info: DeviceInfo,
    chip_kind: String,
    sim_type: Option<i32>,
    sim_profile: Option<String>,
    in_fd: OwnedFd,
    out_fd: OwnedFd,
) -> Result<(PacketStream, PacketSink, ChipInfo, String)> {
    let guid = format!("dualfd-{}", device_info.name);
    let kind = ChipKind::from_str(&chip_kind).unwrap_or(ChipKind::UNSPECIFIED);

    let chip_info = ChipInfo {
        device_info: Some(device_info),
        chip: Some(Chip {
            name: chip_kind.clone(),
            kind,
            id: chip_kind.clone(),
            manufacturer: "".to_string(),
            product_name: "".to_string(),
            address: "".to_string(),
            sim_type,
            sim_profile,
            loopback: false,
            bt_properties: None,
        }),
        name: String::new(),
    };

    set_nonblocking(&in_fd)?;
    set_nonblocking(&out_fd)?;

    let reader = AsyncGenericFd::new(out_fd)?;
    let writer = AsyncGenericFd::new(in_fd)?;

    let stream: PacketStream = match kind {
        ChipKind::BLUETOOTH => {
            let framed = FramedRead::new(reader, H4Codec);
            Box::pin(framed.map(|item| item.map_err(PacketStreamError::Io)))
        }
        ChipKind::UWB => {
            let framed = FramedRead::new(reader, UciCodec);
            Box::pin(framed.map(|item| item.map_err(PacketStreamError::Io)))
        }
        ChipKind::NFC => {
            let framed = FramedRead::new(reader, NciCodec);
            Box::pin(framed.map(|item| item.map_err(PacketStreamError::Io)))
        }
        ChipKind::CELLULAR => {
            let framed = FramedRead::new(reader, ModemCodec);
            Box::pin(framed.map(|item| item.map_err(PacketStreamError::Io)))
        }
        _ => {
            return Err(PacketStreamError::InvalidConfig(format!(
                "Unsupported chip kind for FD transport: {chip_kind}"
            )));
        }
    };

    let sink = futures::sink::unfold(writer, |mut writer, item: bytes::Bytes| async move {
        writer.write_all(&item).await?;
        Ok(writer)
    });
    let sink: PacketSink = Box::pin(sink.sink_map_err(PacketStreamError::Io));

    Ok((stream, sink, chip_info, guid))
}

#[async_trait]
impl TransportListener for DualFdListener {
    async fn accept(&mut self) -> Result<(PacketStream, PacketSink, ChipInfo, String)> {
        while let Some((device, chip_kind, sim_type, sim_profile, (in_fd, out_fd))) =
            self.pending_static_streams.pop_front()
        {
            match create_stream_and_sink(device, chip_kind, sim_type, sim_profile, in_fd, out_fd) {
                Ok(res) => return Ok(res),
                Err(error) => {
                    tracing::warn!(%error, "Failed to create transport for static FD, skipping");
                }
            }
        }

        if self.listening_sockets.is_empty() {
            std::future::pending().await
        }

        // Recreating and boxing futures on every loop iteration is fine because we only
        // loop on transient accept errors, which are rare. This is not a hot path.
        loop {
            let mut accept_futures = Vec::new();
            for (device, chip_kind, sim_type, sim_profile, async_fd) in &self.listening_sockets {
                accept_futures.push(Box::pin(async move {
                    let res = accept_one_connection(async_fd).await;
                    (device, chip_kind, *sim_type, sim_profile, res)
                }));
            }

            let (result, _, _remaining) = futures::future::select_all(accept_futures).await;
            let (device, chip_kind, sim_type, sim_profile, res) = result;

            match res {
                Ok((connected_fd, cloned_fd)) => {
                    match create_stream_and_sink(
                        device.clone(),
                        chip_kind.clone(),
                        sim_type,
                        sim_profile.clone(),
                        connected_fd,
                        cloned_fd,
                    ) {
                        Ok(res) => return Ok(res),
                        Err(error) => {
                            tracing::warn!(
                                %error,
                                "Failed to create transport for accepted connection, continuing"
                            );
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "accept4 failed, continuing");
                }
            }
        }
    }

    fn local_addr(&self) -> Result<StreamAddress> {
        Ok(StreamAddress::Fd {
            in_fd: i32::try_from(self.num_chips).unwrap_or(i32::MAX),
            out_fd: Some(i32::try_from(self.num_devices).unwrap_or(i32::MAX)),
        })
    }

    async fn shutdown(&mut self) -> Result<()> {
        self.pending_static_streams.clear();
        self.listening_sockets.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        os::unix::{
            io::{IntoRawFd, OwnedFd},
            net::{UnixListener, UnixStream},
        },
        path::Path,
    };

    use futures::FutureExt;
    use nix::unistd::pipe;

    use super::*;

    const EXPECTED_CHIP_COUNT: i32 = 2;
    const EXPECTED_DEVICE_COUNT: i32 = 1;
    const HCI_RESET_CMD: &[u8] = &[0x01, 0x03, 0x0c, 0x00];

    fn static_fd_listener(configs: &[(&str, i32, Option<i32>)]) -> DualFdListener {
        let chips = configs
            .iter()
            .map(|&(kind, fd_in, fd_out)| ChipConfig {
                kind: kind.to_string(),
                fd_in,
                fd_out,
                model: None,
                sim_type: None,
                sim_profile: None,
            })
            .collect();
        let config = DualFdConfig {
            devices: vec![DeviceConfig {
                name: Some("test-dev".to_string()),
                chips,
                device_info: None,
            }],
        };
        DualFdListener::new(config).unwrap()
    }

    struct SocketGuard<'a>(&'a Path);

    impl Drop for SocketGuard<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }

    fn prepare_test_socket(socket_path: &Path) -> UnixListener {
        match std::fs::remove_file(socket_path) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => panic!("failed to remove stale socket {socket_path:?}: {e}"),
        }
        UnixListener::bind(socket_path).unwrap()
    }

    async fn assert_listening_socket_roundtrip(
        socket_path: std::path::PathBuf,
        mut listener: DualFdListener,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        const SERVER_MSG: &[u8] = b"hello from server\n";
        let client_path = socket_path.clone();
        let client_task = tokio::spawn(async move {
            let mut stream = tokio::net::UnixStream::connect(client_path).await.unwrap();
            stream.write_all(b"hello from client\n").await.unwrap();

            let mut buf = [0u8; SERVER_MSG.len()];
            stream.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, SERVER_MSG);
        });

        let (mut stream, mut sink, chip_info, _guid) = listener.accept().await.unwrap();
        let dev_info = chip_info.device_info.unwrap();
        assert_eq!(dev_info.name, "cvd-1");
        assert_eq!(dev_info.kind, "CUTTLEFISH");
        assert_eq!(chip_info.chip.unwrap().name, "CELLULAR");

        let bytes = stream.next().await.unwrap().unwrap();
        assert_eq!(bytes.as_ref(), b"hello from client");

        sink.send(bytes::Bytes::from_static(SERVER_MSG)).await.unwrap();

        client_task.await.unwrap();
        std::fs::remove_file(&socket_path).unwrap();
    }

    async fn assert_multi_chip_static_streams_roundtrip(
        mut listener: DualFdListener,
        expected_device_name: &str,
        mut guest_sock: UnixStream,
        guest_to_host_w: OwnedFd,
        host_to_guest_r: OwnedFd,
        expected_sim_type: i32,
        expected_sim_profile: &str,
    ) {
        assert!(matches!(
            listener.local_addr().unwrap(),
            StreamAddress::Fd { in_fd, out_fd: Some(out) }
                if in_fd == EXPECTED_CHIP_COUNT && out == EXPECTED_DEVICE_COUNT
        ));

        let (mut stream0, mut sink0, chip_info0, _guid) = listener.accept().await.unwrap();
        assert_eq!(chip_info0.device_info.unwrap().name, expected_device_name);
        assert_eq!(chip_info0.chip.unwrap().name, "CELLULAR");

        guest_sock.write_all(b"hello from guest 0\n").unwrap();

        let frame0 = stream0.next().await.unwrap().unwrap();
        assert_eq!(frame0.as_ref(), b"hello from guest 0");

        drop(stream0);

        const SERVER_MSG_0: &[u8] = b"hello from server 0\n";
        sink0.send(bytes::Bytes::from_static(SERVER_MSG_0)).await.unwrap();

        let mut guest_buf0 = [0u8; SERVER_MSG_0.len()];
        guest_sock.read_exact(&mut guest_buf0).unwrap();
        assert_eq!(&guest_buf0, SERVER_MSG_0);

        let (mut stream1, mut sink1, chip_info1, _guid) = listener.accept().await.unwrap();
        let chip1 = chip_info1.chip.unwrap();
        assert_eq!(chip1.name, "BLUETOOTH");
        assert_eq!(chip1.sim_type, Some(expected_sim_type));
        assert_eq!(chip1.sim_profile.as_deref(), Some(expected_sim_profile));

        nix::unistd::write(&guest_to_host_w, HCI_RESET_CMD).unwrap();

        let frame1 = stream1.next().await.unwrap().unwrap();
        assert_eq!(frame1.as_ref(), HCI_RESET_CMD);

        const HCI_EVENT: &[u8] = &[4, 0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00];
        sink1.send(bytes::Bytes::from_static(HCI_EVENT)).await.unwrap();

        let mut guest_buf1 = [0u8; HCI_EVENT.len()];
        let mut offset = 0;
        while offset < guest_buf1.len() {
            let n = nix::unistd::read(&host_to_guest_r, &mut guest_buf1[offset..]).unwrap();
            assert!(n > 0, "unexpected EOF on host_to_guest_r");
            offset += n;
        }
        assert_eq!(&guest_buf1, HCI_EVENT);
    }

    #[tokio::test]
    async fn test_dual_fd_listener_listening_socket() {
        let socket_path =
            std::env::temp_dir().join(format!("test_vsock_comm_{}.sock", std::process::id()));
        let std_listener = prepare_test_socket(&socket_path);
        let listener_fd = std_listener.into_raw_fd();

        let json_config = format!(
            r#"
            {{
                "devices": [
                    {{
                        "device_info": {{
                            "name": "cvd-1",
                            "kind": "CUTTLEFISH"
                        }},
                        "chips": [
                            {{
                                "kind": "CELLULAR",
                                "vsockFd": {}
                            }}
                        ]
                    }}
                ]
            }}
            "#,
            listener_fd
        );

        let config: DualFdConfig = serde_json::from_str(&json_config).unwrap();
        let listener = DualFdListener::new(config).unwrap();
        assert_listening_socket_roundtrip(socket_path, listener).await;
    }

    #[tokio::test]
    async fn test_dual_fd_listener_device_info_resolution() {
        struct TestCase {
            name: &'static str,
            extra_json: &'static str,
            expected_device_info: DeviceInfo,
        }

        let test_cases = [
            TestCase {
                name: "explicit device_info with kind",
                extra_json: r#""device_info": {"name": "cvd-2", "kind": "CUSTOM_KIND"}"#,
                expected_device_info: DeviceInfo {
                    name: "cvd-2".into(),
                    id: "cvd-2".into(),
                    kind: "CUSTOM_KIND".into(),
                    ..Default::default()
                },
            },
            TestCase {
                name: "missing device_info defaults to outer legacy name",
                extra_json: r#""name": "generic-dev""#,
                expected_device_info: DeviceInfo {
                    name: "generic-dev".into(),
                    id: "generic-dev".into(),
                    ..Default::default()
                },
            },
            TestCase {
                name: "device_info without name falls back to outer legacy name",
                extra_json: r#""name": "legacy-dev", "device_info": {"kind": "CUTTLEFISH", "variant": "userdebug"}"#,
                expected_device_info: DeviceInfo {
                    name: "legacy-dev".into(),
                    id: "legacy-dev".into(),
                    kind: "CUTTLEFISH".into(),
                    variant: "userdebug".into(),
                    ..Default::default()
                },
            },
            TestCase {
                name: "device_info name takes precedence over legacy outer name",
                extra_json: r#""name": "legacy-dev", "device_info": {"name": "primary-dev", "kind": "CUTTLEFISH"}"#,
                expected_device_info: DeviceInfo {
                    name: "primary-dev".into(),
                    id: "primary-dev".into(),
                    kind: "CUTTLEFISH".into(),
                    ..Default::default()
                },
            },
            TestCase {
                name: "full metadata fields preserved",
                extra_json: r#""device_info": {
                    "name": "cvd-3",
                    "kind": "CUTTLEFISH",
                    "version": "1.0",
                    "sdk_version": "34",
                    "build_id": "AP1A.240305.019",
                    "variant": "userdebug",
                    "arch": "x86_64"
                }"#,
                expected_device_info: DeviceInfo {
                    name: "cvd-3".into(),
                    id: "cvd-3".into(),
                    kind: "CUTTLEFISH".into(),
                    version: "1.0".into(),
                    sdk_version: "34".into(),
                    build_id: "AP1A.240305.019".into(),
                    variant: "userdebug".into(),
                    arch: "x86_64".into(),
                    ..Default::default()
                },
            },
        ];

        for tc in test_cases {
            let (r_fd, w_fd) = pipe().unwrap();
            let json_config = format!(
                r#"{{"devices": [{{"chips": [{{"kind": "CELLULAR", "fdIn": {}, "fdOut": {}}}], {} }}]}}"#,
                w_fd.into_raw_fd(),
                r_fd.into_raw_fd(),
                tc.extra_json
            );

            let config: DualFdConfig = serde_json::from_str(&json_config)
                .unwrap_or_else(|e| panic!("Failed to parse config for test '{}': {e}", tc.name));
            let mut listener = DualFdListener::new(config).unwrap();

            let (_stream, _sink, chip_info, _guid) = listener.accept().await.unwrap();
            let dev_info = chip_info.device_info.unwrap_or_default();
            assert_eq!(
                dev_info, tc.expected_device_info,
                "Failed assertion for case '{}'",
                tc.name
            );
        }
    }

    #[tokio::test]
    async fn test_dual_fd_listener_multi_chip_static_streams() {
        const CHIP1_SIM_TYPE: i32 = 1;
        const CHIP1_SIM_PROFILE: &str = "test-profile-1";

        let (guest_sock, host_sock) = UnixStream::pair().unwrap();
        let sock_fd = host_sock.into_raw_fd();
        let (host_to_guest_r, host_to_guest_w) = pipe().unwrap();
        let (guest_to_host_r, guest_to_host_w) = pipe().unwrap();
        let json_config = format!(
            r#"{{
                "devices": [
                    {{
                        "name": "multi-chip-dev",
                        "chips": [
                            {{
                                "kind": "CELLULAR",
                                "fdIn": {sock_fd},
                                "fdOut": {sock_fd}
                            }},
                            {{
                                "kind": "BLUETOOTH",
                                "fdIn": {},
                                "fdOut": {},
                                "simType": {CHIP1_SIM_TYPE},
                                "simProfile": "{CHIP1_SIM_PROFILE}"
                            }}
                        ]
                    }}
                ]
            }}"#,
            host_to_guest_w.into_raw_fd(),
            guest_to_host_r.into_raw_fd()
        );
        let config: DualFdConfig = serde_json::from_str(&json_config).unwrap();
        let listener = DualFdListener::new(config).unwrap();
        assert_multi_chip_static_streams_roundtrip(
            listener,
            "multi-chip-dev",
            guest_sock,
            guest_to_host_w,
            host_to_guest_r,
            CHIP1_SIM_TYPE,
            CHIP1_SIM_PROFILE,
        )
        .await;
    }

    #[tokio::test]
    async fn test_try_new_with_owned_fds_listening_socket() {
        let socket_path =
            std::env::temp_dir().join(format!("test_vsock_owned_{}.sock", std::process::id()));
        let std_listener = prepare_test_socket(&socket_path);
        let _socket_guard = SocketGuard(&socket_path);
        let in_fd = OwnedFd::from(std_listener);

        let endpoints = vec![DualFdDeviceEndpoints {
            name: None,
            device_info: Some(DeviceInfo {
                name: "cvd-1".to_string(),
                kind: "CUTTLEFISH".to_string(),
                ..Default::default()
            }),
            chips: vec![DualFdChipEndpoints {
                kind: "CELLULAR".to_string(),
                in_fd,
                out_fd: None,
                sim_type: None,
                sim_profile: None,
            }],
        }];

        let listener = DualFdListener::try_new_with_owned_fds(endpoints).unwrap();
        assert_listening_socket_roundtrip(socket_path.clone(), listener).await;
    }

    #[tokio::test]
    async fn test_try_new_with_owned_fds_rejects_out_fd_for_listening_socket() {
        let socket_path = std::env::temp_dir()
            .join(format!("test_vsock_owned_reject_out_fd_{}.sock", std::process::id()));
        let std_listener = prepare_test_socket(&socket_path);
        let _socket_guard = SocketGuard(&socket_path);
        let in_fd = OwnedFd::from(std_listener);
        let (out_fd, _unused_w) = pipe().unwrap();

        let endpoints = vec![DualFdDeviceEndpoints {
            name: Some("cvd-reject".to_string()),
            device_info: None,
            chips: vec![DualFdChipEndpoints {
                kind: "CELLULAR".to_string(),
                in_fd,
                out_fd: Some(out_fd),
                sim_type: None,
                sim_profile: None,
            }],
        }];

        let Err(err) = DualFdListener::try_new_with_owned_fds(endpoints) else {
            panic!("expected InvalidConfig when out_fd is set on listening socket");
        };
        assert!(
            matches!(
                &err,
                PacketStreamError::InvalidConfig(msg)
                    if msg == "Listening socket endpoint for chip CELLULAR cannot specify out_fd"
            ),
            "unexpected error: {err:?}"
        );
    }

    async fn accept_owned_device_info(
        device_name: Option<&str>,
        device_info: Option<DeviceInfo>,
    ) -> DeviceInfo {
        let (r_fd, w_fd) = pipe().unwrap();
        let endpoints = vec![DualFdDeviceEndpoints {
            name: device_name.map(str::to_string),
            device_info,
            chips: vec![DualFdChipEndpoints {
                kind: "CELLULAR".to_string(),
                in_fd: w_fd,
                out_fd: Some(r_fd),
                sim_type: None,
                sim_profile: None,
            }],
        }];
        let mut listener = DualFdListener::try_new_with_owned_fds(endpoints).unwrap();
        let (_stream, _sink, chip_info, _guid) = listener.accept().await.unwrap();
        chip_info.device_info.unwrap_or_default()
    }

    #[tokio::test]
    async fn test_try_new_with_owned_fds_device_info_fallback_to_device_name() {
        let info = accept_owned_device_info(Some("generic-dev"), None).await;
        assert_eq!(info.name, "generic-dev");
        assert_eq!(info.id, "generic-dev");
    }

    #[tokio::test]
    async fn test_try_new_with_owned_fds_device_info_precedence() {
        let info = accept_owned_device_info(
            Some("legacy-dev"),
            Some(DeviceInfo {
                name: "primary-dev".into(),
                kind: "CUTTLEFISH".into(),
                ..Default::default()
            }),
        )
        .await;
        assert_eq!(info.name, "primary-dev");
        assert_eq!(info.id, "primary-dev");
        assert_eq!(info.kind, "CUTTLEFISH");
    }

    #[tokio::test]
    async fn test_try_new_with_owned_fds_device_info_empty_name_fallback() {
        let info = accept_owned_device_info(
            Some("legacy-dev"),
            Some(DeviceInfo { kind: "CUTTLEFISH".into(), ..Default::default() }),
        )
        .await;
        assert_eq!(info.name, "legacy-dev");
        assert_eq!(info.id, "legacy-dev");
        assert_eq!(info.kind, "CUTTLEFISH");
    }

    #[tokio::test]
    async fn test_try_new_with_owned_fds_multi_device_isolation() {
        let (r_fd1, w_fd1) = pipe().unwrap();
        let (r_fd2, w_fd2) = pipe().unwrap();
        let endpoints = vec![
            DualFdDeviceEndpoints {
                name: None,
                device_info: Some(DeviceInfo {
                    name: "cvd-1".to_string(),
                    kind: "CUTTLEFISH".to_string(),
                    ..Default::default()
                }),
                chips: vec![DualFdChipEndpoints {
                    kind: "CELLULAR".to_string(),
                    in_fd: w_fd1,
                    out_fd: Some(r_fd1),
                    sim_type: None,
                    sim_profile: None,
                }],
            },
            DualFdDeviceEndpoints {
                name: None,
                device_info: Some(DeviceInfo {
                    name: "cvd-2".to_string(),
                    kind: "CUTTLEFISH".to_string(),
                    ..Default::default()
                }),
                chips: vec![DualFdChipEndpoints {
                    kind: "BLUETOOTH".to_string(),
                    in_fd: w_fd2,
                    out_fd: Some(r_fd2),
                    sim_type: None,
                    sim_profile: None,
                }],
            },
        ];
        let mut listener = DualFdListener::try_new_with_owned_fds(endpoints).unwrap();
        let addr = listener.local_addr().unwrap();
        let StreamAddress::Fd { in_fd, out_fd: Some(num_devices) } = addr else {
            panic!("unexpected local_addr: {addr:?}");
        };
        assert_eq!(in_fd, 2);
        assert_eq!(num_devices, 2);

        let (_stream1, _sink1, chip_info1, guid1) = listener.accept().await.unwrap();
        let Some(dev1) = chip_info1.device_info else {
            panic!("expected device_info for first accepted connection");
        };
        assert_eq!(dev1.name, "cvd-1");
        assert_eq!(dev1.id, "cvd-1");
        assert_eq!(dev1.kind, "CUTTLEFISH");

        let (_stream2, _sink2, chip_info2, guid2) = listener.accept().await.unwrap();
        let Some(dev2) = chip_info2.device_info else {
            panic!("expected device_info for second accepted connection");
        };
        assert_eq!(dev2.name, "cvd-2");
        assert_eq!(dev2.id, "cvd-2");
        assert_eq!(dev2.kind, "CUTTLEFISH");
        assert_ne!(guid1, guid2);
    }

    #[tokio::test]
    async fn test_try_new_with_owned_fds_multi_chip_static_streams() {
        const CHIP1_SIM_TYPE: i32 = 2;
        const CHIP1_SIM_PROFILE: &str = "test-profile-owned";

        let (guest_sock, host_sock) = UnixStream::pair().unwrap();
        let (host_to_guest_r, host_to_guest_w) = pipe().unwrap();
        let (guest_to_host_r, guest_to_host_w) = pipe().unwrap();
        let endpoints = vec![DualFdDeviceEndpoints {
            name: Some("multi-chip-owned".to_string()),
            device_info: None,
            chips: vec![
                DualFdChipEndpoints {
                    kind: "CELLULAR".to_string(),
                    in_fd: OwnedFd::from(host_sock),
                    out_fd: None,
                    sim_type: None,
                    sim_profile: None,
                },
                DualFdChipEndpoints {
                    kind: "BLUETOOTH".to_string(),
                    in_fd: host_to_guest_w,
                    out_fd: Some(guest_to_host_r),
                    sim_type: Some(CHIP1_SIM_TYPE),
                    sim_profile: Some(CHIP1_SIM_PROFILE.to_string()),
                },
            ],
        }];
        let listener = DualFdListener::try_new_with_owned_fds(endpoints).unwrap();
        assert_multi_chip_static_streams_roundtrip(
            listener,
            "multi-chip-owned",
            guest_sock,
            guest_to_host_w,
            host_to_guest_r,
            CHIP1_SIM_TYPE,
            CHIP1_SIM_PROFILE,
        )
        .await;
    }

    #[tokio::test]
    async fn test_dual_fd_listener_uwb_and_nfc_static_streams() {
        const UCI_CTRL_PKT: &[u8] = &[0x20, 0x00, 0x00, 0x02, 0xaa, 0xbb];
        const NCI_CTRL_PKT: &[u8] = &[0x20, 0x01, 0x02, 0xcc, 0xdd];

        let (mut uwb_guest, uwb_host) = UnixStream::pair().unwrap();
        let (mut nfc_guest, nfc_host) = UnixStream::pair().unwrap();
        let mut listener = static_fd_listener(&[
            ("UWB", uwb_host.into_raw_fd(), None),
            ("NFC", nfc_host.into_raw_fd(), None),
        ]);

        uwb_guest.write_all(UCI_CTRL_PKT).unwrap();
        let (mut uwb_stream, _uwb_sink, uwb_info, _guid) = listener.accept().await.unwrap();
        assert_eq!(uwb_info.chip.unwrap().kind, ChipKind::UWB);
        let uwb_frame = uwb_stream.next().await.unwrap().unwrap();
        assert_eq!(uwb_frame.as_ref(), UCI_CTRL_PKT);

        nfc_guest.write_all(NCI_CTRL_PKT).unwrap();
        let (mut nfc_stream, _nfc_sink, nfc_info, _guid) = listener.accept().await.unwrap();
        assert_eq!(nfc_info.chip.unwrap().kind, ChipKind::NFC);
        let nci_frame = nfc_stream.next().await.unwrap().unwrap();
        assert_eq!(nci_frame.as_ref(), NCI_CTRL_PKT);
    }

    #[tokio::test]
    async fn test_dual_fd_listener_skips_unsupported_chip() {
        let (_bad_guest, bad_host) = UnixStream::pair().unwrap();
        let (mut bt_guest, bt_host) = UnixStream::pair().unwrap();

        let mut listener = static_fd_listener(&[
            ("UNSPECIFIED", bad_host.into_raw_fd(), None),
            ("BLUETOOTH", bt_host.into_raw_fd(), None),
        ]);

        let (mut bt_stream, _bt_sink, bt_info, _guid) = listener.accept().await.unwrap();
        assert_eq!(bt_info.chip.unwrap().kind, ChipKind::BLUETOOTH);

        bt_guest.write_all(HCI_RESET_CMD).unwrap();
        let hci_frame = bt_stream.next().await.unwrap().unwrap();
        assert_eq!(hci_frame.as_ref(), HCI_RESET_CMD);
    }

    #[tokio::test]
    async fn test_dual_fd_listener_shutdown_clears_pending_streams() {
        let (mut bt_guest, bt_host) = UnixStream::pair().unwrap();
        bt_guest.set_read_timeout(Some(std::time::Duration::from_secs(1))).unwrap();

        let mut listener = static_fd_listener(&[("BLUETOOTH", bt_host.into_raw_fd(), None)]);

        listener.shutdown().await.unwrap();

        let mut eof_buf = [0u8; 1];
        assert_eq!(bt_guest.read(&mut eof_buf).unwrap(), 0);

        // With pending_static_streams and listening_sockets cleared by DualFdListener::shutdown,
        // DualFdListener::accept must park on std::future::pending() rather than reach
        // futures::future::select_all, which panics on an empty set.
        assert!(
            listener.accept().now_or_never().is_none(),
            "DualFdListener::accept should remain pending after shutdown"
        );
    }
}
