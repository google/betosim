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
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::SystemTime,
};

use actor_framework::FrameworkError;
use bytes::Bytes;
use capture_actor::{CaptureActor, CaptureClient, CaptureError, DataLinkType};
use capture_api::{CaptureCreate, CaptureInfo, CaptureSender, Direction};
use netsim_model::{ChipId, ChipKind, ClientError};
use netsim_packets::PcapHeader;

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// The BDD World for Capture Actor tests.
pub struct World {
    pub client: CaptureClient,
    pub temp_dir: PathBuf,
    pub keep_temp_dir: bool,
    _actor_task: tokio::task::JoinHandle<()>,
}

impl Drop for World {
    fn drop(&mut self) {
        self._actor_task.abort();
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

        World { client, temp_dir, keep_temp_dir: false, _actor_task: actor_task }
    }

    pub async fn when_create_capture(
        &self,
        chip_id: u32,
        chip_kind: ChipKind,
        device_name: &str,
        enabled: bool,
    ) -> Result<(), ClientError> {
        let enabled_flag = Arc::new(AtomicBool::new(enabled));
        let create_params =
            CaptureCreate { chip_kind, device_name: device_name.to_string(), enabled_flag };
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
        let tx =
            self.client.packet_sender(ChipId(chip_id)).await.expect("Failed to get packet sender");
        tx.send((SystemTime::now(), Direction::Sent, Bytes::copy_from_slice(packet)))
            .expect("Failed to send packet");
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
}
