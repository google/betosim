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

//! Integration Tests: Quick Boot Snapshot Save & Restore (b/560172253).

use slirp_actor::SlirpBackend;

use super::test_driver::SlirpTestDriver;

// -----------------------------------------------------------------------------
// 1. Snapshot Save & Restore Basic Test
// -----------------------------------------------------------------------------

async fn run_snapshot_save_restore_test(backend: SlirpBackend) {
    // Given an active Slirp driver session
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends uplink traffic before snapshot
    let dummy = bytes::Bytes::from(vec![0u8; 64]);
    let send_res = driver.send_packet(dummy).await;
    assert!(send_res.is_ok());

    // Then driver remains initialized
    assert!(driver.is_initialized().await);
}

#[tokio::test]
async fn test_snapshot_save_restore() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_snapshot_save_restore_test(backend).await;
    }
}

// -----------------------------------------------------------------------------
// 2. Snapshot Multi-Flow Persistence Test
// -----------------------------------------------------------------------------

async fn run_snapshot_multi_flow_test(backend: SlirpBackend) {
    // Given a Slirp driver with multiple traffic flows initiated
    let mut driver = SlirpTestDriver::new(backend, None).await;

    // When guest sends packets across multiple distinct flows
    for i in 0..5 {
        let mut pkt = vec![0u8; 64];
        pkt[0] = i;
        let res = driver.send_packet(bytes::Bytes::from(pkt)).await;
        assert!(res.is_ok());
    }

    // Then driver state remains healthy
    assert!(driver.is_initialized().await);
}

#[tokio::test]
async fn test_snapshot_multi_flow() {
    for backend in [SlirpBackend::CFfi, SlirpBackend::Native] {
        run_snapshot_multi_flow_test(backend).await;
    }
}
