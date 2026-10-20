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
        let async_fd = AsyncFd::new(fd).map_err(PacketStreamError::Io)?;
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
    config: DualFdConfig,
    /// Streams established from pre-existing FDs (e.g., inherited pipes) that
    /// do not need to accept connections dynamically.
    pending_static_streams:
        VecDeque<(DeviceInfo, String, Option<i32>, Option<String>, (OwnedFd, OwnedFd))>,
    listening_sockets: Vec<(DeviceInfo, String, Option<i32>, Option<String>, AsyncFd<OwnedFd>)>,
}

impl DualFdListener {
    pub async fn new(config: DualFdConfig) -> Result<Self> {
        let mut listener =
            Self { config, pending_static_streams: VecDeque::new(), listening_sockets: Vec::new() };
        listener.prepare_streams()?;
        Ok(listener)
    }

    fn prepare_streams(&mut self) -> Result<()> {
        self.pending_static_streams.clear();
        self.listening_sockets.clear();
        for device in &self.config.devices {
            let mut device_info = device.device_info.clone().unwrap_or_default();
            if device_info.name.is_empty() {
                if let Some(name) = &device.name {
                    device_info.name.clone_from(name);
                }
            }
            if device_info.id.is_empty() {
                device_info.id.clone_from(&device_info.name);
            }

            for chip in &device.chips {
                let in_fd = claim_fd(chip.fd_in)?;

                if is_listening_socket(&in_fd) {
                    set_nonblocking(&in_fd)?;
                    let async_fd = AsyncFd::new(in_fd).map_err(|e| {
                        PacketStreamError::InvalidConfig(format!(
                            "Failed to create AsyncFd for listening socket: {e}"
                        ))
                    })?;
                    self.listening_sockets.push((
                        device_info.clone(),
                        chip.kind.clone(),
                        chip.sim_type,
                        chip.sim_profile.clone(),
                        async_fd,
                    ));
                } else {
                    let out_fd = match chip.fd_out {
                        Some(fd) if fd != chip.fd_in => claim_fd(fd)?,
                        _ => in_fd.try_clone().map_err(PacketStreamError::Io)?,
                    };

                    self.pending_static_streams.push_back((
                        device_info.clone(),
                        chip.kind.clone(),
                        chip.sim_type,
                        chip.sim_profile.clone(),
                        (in_fd, out_fd),
                    ));
                }
            }
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
        let total_chips: usize = self.config.devices.iter().map(|d| d.chips.len()).sum();
        Ok(StreamAddress::Fd {
            in_fd: i32::try_from(total_chips).unwrap_or(i32::MAX),
            out_fd: Some(i32::try_from(self.config.devices.len()).unwrap_or(i32::MAX)),
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
    use super::*;

    #[tokio::test]
    async fn test_dual_fd_listener_listening_socket() {
        use std::os::unix::{io::IntoRawFd, net::UnixListener};

        use futures::{SinkExt, StreamExt};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let socket_path =
            std::env::temp_dir().join(format!("test_vsock_comm_{}.sock", std::process::id()));
        if socket_path.exists() {
            let _ = std::fs::remove_file(&socket_path);
        }
        let std_listener = UnixListener::bind(&socket_path).unwrap();
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
        let mut listener = DualFdListener::new(config).await.unwrap();

        let client_path = socket_path.clone();
        let client_task = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let mut stream = tokio::net::UnixStream::connect(client_path).await.unwrap();
            stream.write_all(b"hello from client\n").await.unwrap();

            let mut buf = [0; 18];
            stream.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"hello from server\n");
        });

        let (mut stream, mut sink, chip_info, _guid) = listener.accept().await.unwrap();
        let dev_info = chip_info.device_info.unwrap();
        assert_eq!(dev_info.name, "cvd-1");
        assert_eq!(dev_info.kind, "CUTTLEFISH");
        assert_eq!(chip_info.chip.unwrap().name, "CELLULAR");

        let bytes = stream.next().await.unwrap().unwrap();
        assert_eq!(bytes.as_ref(), b"hello from client");

        sink.send(bytes::Bytes::from("hello from server\n")).await.unwrap();

        client_task.await.unwrap();
        let _ = std::fs::remove_file(&socket_path);
    }

    #[tokio::test]
    async fn test_dual_fd_listener_device_info_resolution() {
        use std::os::unix::io::IntoRawFd;

        use nix::unistd::pipe;

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
                r_fd.into_raw_fd(),
                w_fd.into_raw_fd(),
                tc.extra_json
            );

            let config: DualFdConfig = serde_json::from_str(&json_config)
                .unwrap_or_else(|e| panic!("Failed to parse config for test '{}': {e}", tc.name));
            let mut listener = DualFdListener::new(config).await.unwrap();

            let (_stream, _sink, chip_info, _guid) = listener.accept().await.unwrap();
            let dev_info = chip_info.device_info.unwrap_or_default();
            assert_eq!(
                dev_info, tc.expected_device_info,
                "Failed assertion for case '{}'",
                tc.name
            );
        }
    }
}
