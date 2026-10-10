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

use bytes::Bytes;
use libslirp_rs::libslirp::LibSlirp;
use libslirp_rs::libslirp_config::SlirpConfig;

use std::io;
use std::sync::mpsc;
use std::time::Duration;

/// Test whether shutdown closes the rx channel
#[test]
fn it_shutdown() {
    let config = SlirpConfig { ..Default::default() };

    let before_fd_count = count_open_fds().unwrap();

    let (tx, rx) = mpsc::channel::<Bytes>();
    let slirp = LibSlirp::new(config, tx, None, None);
    slirp.shutdown();
    assert_eq!(
        rx.recv_timeout(Duration::from_millis(5)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );

    let after_fd_count = count_open_fds().unwrap();
    assert_eq!(before_fd_count, after_fd_count);
}

#[cfg(target_os = "linux")]
fn count_open_fds() -> io::Result<usize> {
    use std::fs;
    let entries = fs::read_dir("/proc/self/fd")?;
    Ok(entries.count())
}

#[cfg(not(target_os = "linux"))]
fn count_open_fds() -> io::Result<usize> {
    Ok(0)
}
