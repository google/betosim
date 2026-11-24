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
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use actor_framework::FrameworkError;
use bytes::Bytes;
use capture_actor::{CaptureActor, CaptureClient, CaptureError, DataLinkType};
use capture_api::{CaptureCreate, CaptureInfo, CaptureSender, Direction};
use netsim_model::{ChipError, ChipId, ChipKind, ClientError};
use netsim_packets::{PcapHeader, PcapRecordHeader};
use tokio::sync::mpsc;

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);
const STREAM_RECV_TIMEOUT: Duration = Duration::from_secs(2);

/// The BDD World for Capture Actor tests.
pub struct World {
    pub client: CaptureClient,
    pub temp_dir: PathBuf,
    pub keep_temp_dir: bool,
    actor_task: tokio::task::JoinHandle<()>,
}

impl Drop for World {
    fn drop(&mut self) {
        self.actor_task.abort();
        if !self.keep_temp_dir && self.temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&self.temp_dir);
        }
    }
}

impl World {
    pub async fn new() -> Self {
        Self::new_with_default(false).await
    }

    pub async fn new_with_default(default_capture_enabled: bool) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir =
            std::env::temp_dir().join(format!("netsim_capture_test_{}_{}", std::process::id(), id));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let (runner, client) = capture_actor::new();
        let actor = CaptureActor::new(default_capture_enabled, Some(temp_dir.clone()));
        let actor_task = tokio::spawn(runner.run(actor));

        World { client, temp_dir, keep_temp_dir: false, actor_task }
    }

    pub async fn when_create_capture(
        &self,
        chip_id: u32,
        chip_kind: ChipKind,
        device_name: &str,
        enabled: bool,
    ) -> Result<(), ClientError> {
        let create_params = Self::build_capture_create(chip_kind, device_name, enabled);
        CaptureSender::create_capture(&self.client, ChipId(chip_id), create_params).await
    }

    pub async fn when_update_capture(
        &self,
        chip_id: u32,
        enabled: bool,
    ) -> Result<CaptureInfo, ClientError> {
        self.client.update_capture(ChipId(chip_id), enabled).await
    }

    pub async fn when_delete_capture(&self, chip_id: u32) -> Result<(), ClientError> {
        self.client.delete_capture(ChipId(chip_id)).await
    }

    pub async fn when_shutdown(&self) -> Result<(), FrameworkError<CaptureError>> {
        self.client.shutdown().await
    }

    pub async fn when_set_capture_directory(&self, path: std::path::PathBuf) {
        self.client
            .perform_action(None, capture_api::CaptureAction::SetCaptureDirectory { path })
            .await
            .expect("Failed to set capture directory");
    }

    pub async fn then_capture_is_none(&self, chip_id: u32) {
        self.poll_until(|| async {
            let info = self.client.get_capture(ChipId(chip_id)).await.unwrap();
            if info.is_none() { Some(()) } else { None }
        })
        .await;
    }

    pub async fn then_capture_is_enabled(&self, chip_id: u32, expected_enabled: bool) {
        self.poll_until(|| async {
            let info = self.client.get_capture(ChipId(chip_id)).await.unwrap();
            if let Some(info) = info
                && info.enabled == expected_enabled
            {
                return Some(());
            }
            None
        })
        .await;
    }

    pub async fn then_capture_stats_are(
        &self,
        chip_id: u32,
        expected_records: u64,
        expected_bytes: u64,
    ) {
        self.poll_until(|| async {
            let info = self.client.get_capture(ChipId(chip_id)).await.unwrap();
            if let Some(info) = info
                && info.records_written == expected_records
                && info.bytes_written == expected_bytes
            {
                return Some(());
            }
            None
        })
        .await;
    }

    pub async fn when_packet_is_sent(&self, chip_id: u32, packet: &[u8]) {
        self.when_timed_packet_is_sent(chip_id, SystemTime::now(), packet).await;
    }

    pub async fn when_dummy_packet_is_sent(&self, chip_id: u32) {
        self.when_packet_is_sent(chip_id, &[0, 1, 2, 3]).await;
    }

    pub async fn then_capture_file_exists(&self) {
        self.poll_until(|| async {
            let entries: Vec<_> =
                std::fs::read_dir(&self.temp_dir).unwrap().map(|r| r.unwrap().path()).collect();
            if entries.len() == 1 { Some(()) } else { None }
        })
        .await;
    }

    pub async fn then_capture_file_is_not_empty(&self) {
        self.poll_until(|| async {
            let entries: Vec<_> =
                std::fs::read_dir(&self.temp_dir).unwrap().map(|r| r.unwrap().path()).collect();
            if entries.len() == 1 {
                let metadata = std::fs::metadata(&entries[0]).unwrap();
                if metadata.len() > 0 {
                    return Some(());
                }
            }
            None
        })
        .await;
    }

    pub async fn then_capture_file_has_dlt(&self, expected_dlt: DataLinkType) {
        self.poll_until(|| async {
            // Safely open directory, retry if not created yet
            let dir_entries = std::fs::read_dir(&self.temp_dir).ok()?;

            let mut paths = Vec::new();
            for entry in dir_entries {
                paths.push(entry.ok()?.path());
            }

            // Expect exactly one capture file
            if paths.len() != 1 {
                return None;
            }

            // Safely read file, retry if locked/being written
            let bytes = std::fs::read(&paths[0]).ok()?;
            if bytes.len() < 24 {
                return None;
            }

            // Safely parse header
            use zerocopy::FromBytes;
            let (header, _) = PcapHeader::read_from_prefix(&bytes).ok()?;

            if header.magic_number.get() == 0xa1b2c3d4
                && header.network.get() == expected_dlt.as_u32()
            {
                Some(())
            } else {
                None
            }
        })
        .await;
    }

    pub async fn then_capture_filepath_is(&self, chip_id: u32, expected_path: &Path) {
        let info = self.client.get_capture(netsim_model::ChipId(chip_id)).await.unwrap().unwrap();
        assert_eq!(info.filepath.as_deref(), Some(expected_path));
        assert!(expected_path.exists());
    }

    pub async fn then_capture_error_is_chip_not_found<T: std::fmt::Debug>(
        &self,
        res: Result<T, netsim_model::ClientError>,
        expected_id: u32,
    ) {
        use actor_framework::FrameworkError;
        use capture_actor::CaptureError;
        use netsim_model::{ChipError, ChipId, ClientError};

        let err = res.expect_err("Expected ClientError::Framework(ChipNotFound)");
        match err {
            ClientError::Framework(boxed) => {
                match boxed.downcast_ref::<FrameworkError<CaptureError>>() {
                    Some(FrameworkError::ServiceError(CaptureError::Chip(
                        ChipError::ChipNotFound(id),
                    ))) => {
                        assert_eq!(*id, ChipId(expected_id));
                    }
                    other => panic!("Unexpected service error: {other:?}"),
                }
            }
            other => panic!("Expected ClientError::Framework, got: {other:?}"),
        }
    }

    async fn poll_until<F, Fut, T>(&self, mut f: F) -> T
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Option<T>>,
    {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(1);
        while start.elapsed() < timeout {
            if let Some(res) = f().await {
                return res;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("Timeout waiting for condition");
    }

    fn build_capture_create(
        chip_kind: ChipKind,
        device_name: &str,
        enabled: bool,
    ) -> CaptureCreate {
        CaptureCreate {
            chip_kind,
            device_name: device_name.to_string(),
            enabled_flag: Arc::new(AtomicBool::new(enabled)),
        }
    }

    /// BDD Step: Invokes `ResourceClient::create` without a `ChipId` to exercise missing-ID validation.
    pub(crate) async fn when_create_without_chip_id(
        &self,
        kind: ChipKind,
        device_name: &str,
        enabled: bool,
    ) -> Result<ChipId, ClientError> {
        let params = Self::build_capture_create(kind, device_name, enabled);
        self.client.create(params).await.map_err(ClientError::from)
    }

    async fn open_stream_and_read_header(&self, chip_id: u32) -> (mpsc::Receiver<Bytes>, Bytes) {
        let mut rx = self.client.stream_capture(ChipId(chip_id)).await.unwrap();
        let header_bytes =
            tokio::time::timeout(STREAM_RECV_TIMEOUT, rx.recv()).await.unwrap().unwrap();
        (rx, header_bytes)
    }

    /// BDD Step: Subscribes to the live stream and drains the initial PCAP global header.
    pub(crate) async fn when_stream_capture_started(&self, chip_id: u32) -> mpsc::Receiver<Bytes> {
        self.open_stream_and_read_header(chip_id).await.0
    }

    /// BDD Step: Subscribes to the live stream and returns the initial PCAP global header bytes.
    pub(crate) async fn when_stream_header_received(&self, chip_id: u32) -> Bytes {
        self.open_stream_and_read_header(chip_id).await.1
    }

    /// BDD Step: Sends a packet with an explicit timestamp through the chip's packet sender.
    pub(crate) async fn when_timed_packet_is_sent(
        &self,
        chip_id: u32,
        timestamp: SystemTime,
        payload: &[u8],
    ) {
        let tx =
            self.client.packet_sender(ChipId(chip_id)).await.expect("Failed to get packet sender");
        tx.send((timestamp, Direction::Sent, Bytes::copy_from_slice(payload)))
            .expect("Failed to send packet");
    }

    /// BDD Step: Asserts a client call failed with `ChipError::InvalidArguments`.
    pub(crate) fn then_error_is_invalid_arguments<T: std::fmt::Debug>(
        &self,
        res: Result<T, ClientError>,
    ) {
        let err = res.expect_err("Expected ClientError with ChipError::InvalidArguments");
        assert!(
            matches!(err.as_chip_error(), Some(ChipError::InvalidArguments(_))),
            "Expected InvalidArguments, got: {err:?}"
        );
    }

    /// BDD Step: Asserts the PCAP global header advertises the expected data link type.
    pub(crate) fn then_pcap_header_dlt_is(&self, header_bytes: &[u8], expected_dlt: DataLinkType) {
        use zerocopy::FromBytes;

        let header = PcapHeader::read_from_bytes(header_bytes).unwrap();
        assert_eq!(header.network.get(), expected_dlt.as_u32());
    }

    /// BDD Step: Asserts the next Bluetooth stream record carries the expected H4-sent payload.
    pub(crate) async fn then_next_stream_bt_payload_is(
        &self,
        rx: &mut mpsc::Receiver<Bytes>,
        expected_payload: &[u8],
    ) {
        use zerocopy::FromBytes;

        const H4_SENT_DIRECTION_HEADER: &[u8] = &[0, 0, 0, 1];
        let record = tokio::time::timeout(STREAM_RECV_TIMEOUT, rx.recv()).await.unwrap().unwrap();
        let (_record_header, body) = PcapRecordHeader::read_from_prefix(&record).unwrap();
        let payload = body
            .strip_prefix(H4_SENT_DIRECTION_HEADER)
            .expect("Bluetooth H4 stream record missing sent direction header");
        assert_eq!(payload, expected_payload);
    }
}
