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

//! Integration tests for the `IniFile` manager.

use std::{
    collections::HashMap,
    env, fs, io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use daemon::{IniFile, IniFileAccess, IniFileInitialized};

/// Creates a unique temp directory for a test.
fn create_temp_dir(test_name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("config_test_{}_{}", test_name, rand::random::<u32>()));
    fs::create_dir_all(&dir).expect("Failed to create temp dir");
    dir
}

/// Polls `predicate` every 50 ms for up to 2 s to tolerate brief asynchronous
/// handle release by background scanners on Windows.
fn wait_until(mut predicate: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if predicate() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Returns the sorted file names currently present in `dir`.
fn dir_entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Tests the basic flow of acquiring the lock as a Writer, writing to the INI
/// file, and a second instance becoming a Reader and reading the configuration.
#[test]
fn test_ini_writer_reader_flow() {
    let temp_dir = create_temp_dir("writer_reader");

    let ini_file1 = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    match ini_file1.try_acquire() {
        Ok(IniFileAccess::Writer(guard)) => {
            let mut data = HashMap::new();
            data.insert("pid".to_string(), "12345".to_string());
            data.insert("grpc.port".to_string(), "8554".to_string());
            data.insert("uds.path".to_string(), "/tmp/netsim.sock".to_string());
            let initialized_guard = guard.write(&data).unwrap();

            let ini_file2 = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
            match ini_file2.try_acquire() {
                Ok(IniFileAccess::Reader(config)) => {
                    assert_eq!(config.pid, Some(12345));
                    assert_eq!(config.grpc_port, 8554);
                    assert_eq!(config.uds_path, Some("/tmp/netsim.sock".to_string()));
                }
                Ok(IniFileAccess::Initializing) => {
                    panic!("Expected Reader access, got Initializing");
                }
                Err(e) => panic!("Expected Reader access, got Err: {}", e),
                Ok(IniFileAccess::Writer(_)) => panic!("Expected Reader access, got Writer"),
            }

            let ini_path = initialized_guard.path().to_path_buf();
            drop(initialized_guard);
            assert!(wait_until(|| !ini_path.exists()));
        }
        Ok(IniFileAccess::Reader(_)) => panic!("Expected Writer access, got Reader"),
        Ok(IniFileAccess::Initializing) => panic!("Expected Writer access, got Initializing"),
        Err(e) => panic!("Expected Writer access, got Err: {}", e),
    }
    let _ = fs::remove_dir_all(&temp_dir);
}

/// Tests that `try_acquire` correctly returns an error when the INI file
/// is malformed when read by a Reader instance.
#[test]
fn test_ini_reader_malformed_file() {
    let temp_dir = create_temp_dir("reader_malformed");

    // Instance 1 becomes owner and writes a malformed file
    let ini_file1 = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    match ini_file1.try_acquire() {
        Ok(IniFileAccess::Writer(guard)) => {
            let mut data = HashMap::new();
            data.insert("grpc.port".to_string(), "8554".to_string());
            let _initialized_guard = guard.write(&data).unwrap();

            // Mess up the file manually after it has been initialized
            fs::write(temp_dir.join("netsim.ini"), "malformed_line").unwrap();

            let ini_file2 = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
            let result = ini_file2.try_acquire();

            match result {
                Err(e) => {
                    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
                }
                Ok(r) => panic!("Should have failed to read malformed INI, got {:?}", r),
            }
        }
        Ok(IniFileAccess::Reader(_)) => panic!("Instance 1 should be Writer, got Reader"),
        Ok(IniFileAccess::Initializing) => panic!("Instance 1 should be Writer, got Initializing"),
        Err(e) => panic!("Instance 1 should be Writer, got Err: {}", e),
    }
    let _ = fs::remove_dir_all(&temp_dir);
}

/// Tests that `try_acquire` returns an error if grpc.port is missing.
#[test]
fn test_ini_reader_missing_grpc_port() {
    let temp_dir = create_temp_dir("reader_missing_port");
    let ini_file1 = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    match ini_file1.try_acquire() {
        Ok(IniFileAccess::Writer(guard)) => {
            let mut data = HashMap::new();
            data.insert("pid".to_string(), "12345".to_string());
            let _initialized_guard = guard.write(&data).unwrap(); // grpc.port is missing

            let ini_file2 = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
            match ini_file2.try_acquire() {
                Err(e) => {
                    assert_eq!(e.kind(), io::ErrorKind::InvalidData);
                    assert!(e.to_string().contains("Missing grpc.port"));
                }
                Ok(r) => panic!("Expected error for missing grpc.port, got {:?}", r),
            }
        }
        Ok(IniFileAccess::Reader(_)) => panic!("Instance 1 should be Writer, got Reader"),
        Ok(IniFileAccess::Initializing) => panic!("Instance 1 should be Writer, got Initializing"),
        Err(e) => panic!("Instance 1 should be Writer, got Err: {}", e),
    }
    let _ = fs::remove_dir_all(&temp_dir);
}

/// Tests that `try_acquire` correctly becomes the Writer when no INI file
/// or lock file exists initially.
#[test]
fn test_ini_writer_no_file() {
    let temp_dir = create_temp_dir("writer_no_file");
    let ini_file = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();

    // Should become Writer as no file exists in this dir
    match ini_file.try_acquire() {
        Ok(IniFileAccess::Writer(_)) => {
            // Success
        }
        Ok(IniFileAccess::Reader(_)) => panic!("Expected Writer access, got Reader"),
        Ok(IniFileAccess::Initializing) => panic!("Expected Writer access, got Initializing"),
        Err(e) => panic!("Expected Writer access as file does not exist, got Err: {}", e),
    }
    let _ = fs::remove_dir_all(&temp_dir);
}

/// Tests that evicting a stale lock lets a new instance become the Writer
/// while the evicted owner still holds its lock on the retired lock file, that
/// a repeated eviction for the old port is a no-op, and that the evicted owner
/// releasing its lock neither deletes the new owner's INI file nor lets a
/// handle opened before the eviction become a split-brain Writer.
#[test]
fn test_ini_evict_stale_lock_allows_new_writer_and_blocks_resurrected_owner() {
    let temp_dir = create_temp_dir("evict_stale_lock");

    let ini_file1 = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let Ok(IniFileAccess::Writer(guard1)) = ini_file1.try_acquire() else {
        panic!("Instance 1 should be Writer");
    };
    let mut data1 = HashMap::new();
    data1.insert("grpc.port".to_string(), "59299".to_string());
    let initialized_guard1 = guard1.write(&data1).unwrap();
    let guard1_ini_path = initialized_guard1.path().to_path_buf();
    let guard1_lock_inode = initialized_guard1.lock_inode();
    assert!(initialized_guard1.still_owns_lock());

    let reader_check = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let observed = match reader_check.try_acquire() {
        Ok(IniFileAccess::Reader(config)) => config,
        other => panic!("Expected Reader(59299), got {other:?}"),
    };
    assert_eq!(observed.grpc_port, 59299);

    // Open a handle before the eviction to cover the retired-lock race.
    let orphan_ini = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();

    assert!(
        IniFile::evict_stale_lock(&temp_dir, 1, &observed).unwrap(),
        "first eviction should retire the stale lock"
    );
    assert!(!initialized_guard1.still_owns_lock());

    // A new instance becomes Writer even though `initialized_guard1` still
    // holds its lock on the retired lock file.
    let ini_file2 = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let Ok(IniFileAccess::Writer(guard2)) = ini_file2.try_acquire() else {
        panic!("Instance 2 should be Writer after the stale lock eviction");
    };
    let mut data2 = HashMap::new();
    data2.insert("grpc.port".to_string(), "60123".to_string());
    let initialized_guard2 = guard2.write(&data2).unwrap();
    assert!(initialized_guard2.still_owns_lock());

    // A second eviction for the old port must be a no-op.
    assert!(!IniFile::evict_stale_lock(&temp_dir, 1, &observed).unwrap());
    assert!(initialized_guard2.still_owns_lock());

    // Simulate `NetsimDaemon::run_daemon` teardown of the evicted owner using
    // its startup-cached `(ini_path, ini_lock_inode)` before dropping its
    // guard: neither that call nor `drop(initialized_guard1)` may unlink the
    // new owner's INI file, and the pre-eviction handle must observe
    // `Initializing` rather than becoming a second Writer.
    IniFileInitialized::unlink_discovery_file_if_owner(&guard1_ini_path, guard1_lock_inode);
    drop(initialized_guard1);
    match orphan_ini.try_acquire() {
        Ok(IniFileAccess::Initializing) => {}
        other => panic!("Expected Initializing, got {other:?}"),
    }
    let expected = ["netsim.ini", "netsim.ini.init.lock", "netsim.ini.lock"];
    assert!(
        wait_until(|| dir_entries(&temp_dir) == expected),
        "only the new owner's files should remain once the retired lock closes: {:?}",
        dir_entries(&temp_dir),
    );

    let reader_after = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    match reader_after.try_acquire() {
        Ok(IniFileAccess::Reader(config)) => assert_eq!(config.grpc_port, 60123),
        other => panic!("Expected Reader(60123), got {other:?}"),
    }

    // Legitimate owner teardown with its startup-cached identity removes `netsim.ini`.
    let guard2_ini_path = initialized_guard2.path().to_path_buf();
    let guard2_lock_inode = initialized_guard2.lock_inode();
    IniFileInitialized::unlink_discovery_file_if_owner(&guard2_ini_path, guard2_lock_inode);
    assert!(wait_until(|| !guard2_ini_path.exists()));
    drop(initialized_guard2);
    let _ = fs::remove_dir_all(&temp_dir);
}

/// Tests that `evict_stale_lock` does not retire a new owner's `.lock` when the
/// successor binds the same pinned gRPC port with a different PID.
#[test]
fn test_ini_evict_stale_lock_spares_same_port_successor() {
    let temp_dir = create_temp_dir("evict_same_port_successor");

    // Owner W advertises a pinned port.
    let ini_w = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let Ok(IniFileAccess::Writer(guard_w)) = ini_w.try_acquire() else {
        panic!("W should be Writer");
    };
    let mut data_w = HashMap::new();
    data_w.insert("grpc.port".to_string(), "59299".to_string());
    data_w.insert("pid".to_string(), "1111".to_string());
    let initialized_w = guard_w.write(&data_w).unwrap();

    // Starter S observes W and (in production) starts probing it.
    let reader_s = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let observed = match reader_s.try_acquire() {
        Ok(IniFileAccess::Reader(config)) => config,
        other => panic!("Expected Reader(59299), got {other:?}"),
    };
    assert_eq!(observed.grpc_port, 59299);

    // W tears down gracefully; successor N binds the same pinned port.
    drop(initialized_w);
    let ini_n = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let Ok(IniFileAccess::Writer(guard_n)) = ini_n.try_acquire() else {
        panic!("N should be Writer");
    };
    let mut data_n = HashMap::new();
    data_n.insert("grpc.port".to_string(), "59299".to_string());
    data_n.insert("pid".to_string(), "2222".to_string());
    let initialized_n = guard_n.write(&data_n).unwrap();

    // S's probe of W failed; its eviction must not retire N's `.lock`.
    assert!(!IniFile::evict_stale_lock(&temp_dir, 1, &observed).unwrap());
    assert!(initialized_n.still_owns_lock(), "eviction retired the successor's .lock");
    drop(initialized_n);
    let _ = fs::remove_dir_all(&temp_dir);
}

/// Tests that `evict_stale_lock` is a no-op when `netsim.ini` matches `observed`
/// but `netsim.ini.lock` is not currently held.
#[test]
fn test_ini_evict_is_noop_when_lock_not_held() {
    let temp_dir = create_temp_dir("evict_noop_unlocked");

    let ini_file1 = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let Ok(IniFileAccess::Writer(guard1)) = ini_file1.try_acquire() else {
        panic!("Instance 1 should be Writer");
    };
    let mut data1 = HashMap::new();
    data1.insert("grpc.port".to_string(), "59299".to_string());
    let initialized_guard1 = guard1.write(&data1).unwrap();

    let reader_check = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let observed = match reader_check.try_acquire() {
        Ok(IniFileAccess::Reader(config)) => config,
        other => panic!("Expected Reader(59299), got {other:?}"),
    };
    drop(initialized_guard1);

    // Recreate `netsim.ini` while `netsim.ini.lock` on disk is unlocked.
    let ini_path = temp_dir.join("netsim.ini");
    fs::write(&ini_path, "grpc.port=59299\n").unwrap();

    assert!(!IniFile::evict_stale_lock(&temp_dir, 1, &observed).unwrap());
    assert!(ini_path.exists());
    assert!(temp_dir.join("netsim.ini.lock").exists());
    let _ = fs::remove_dir_all(&temp_dir);
}

/// Tests that `evict_stale_lock` reports `false` and leaves the discovery
/// files untouched while another process holds `netsim.ini.init.lock`, and
/// retires the stale lock once that holder releases it.
#[test]
fn test_ini_evict_skips_while_init_lock_held() {
    let temp_dir = create_temp_dir("evict_skips_init_lock_held");

    let ini_file1 = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let Ok(IniFileAccess::Writer(guard1)) = ini_file1.try_acquire() else {
        panic!("Instance 1 should be Writer");
    };
    let mut data1 = HashMap::new();
    data1.insert("grpc.port".to_string(), "59299".to_string());
    let initialized_guard1 = guard1.write(&data1).unwrap();

    let reader_check = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let observed = match reader_check.try_acquire() {
        Ok(IniFileAccess::Reader(config)) => config,
        other => panic!("Expected Reader(59299), got {other:?}"),
    };

    // Another starter holds `netsim.ini.init.lock` (inside `try_acquire` or
    // its own eviction) when this evictor runs. A second handle in the same
    // process contends like a separate process on flock and LockFileEx.
    let init_lock_holder = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(temp_dir.join("netsim.ini.init.lock"))
        .unwrap();
    init_lock_holder.lock().unwrap();

    assert!(
        !IniFile::evict_stale_lock(&temp_dir, 1, &observed).unwrap(),
        "eviction should be skipped while netsim.ini.init.lock is held"
    );
    assert!(
        initialized_guard1.still_owns_lock(),
        "stale lock was retired despite the busy init lock"
    );
    assert_eq!(dir_entries(&temp_dir), ["netsim.ini", "netsim.ini.init.lock", "netsim.ini.lock"]);

    // Unlock explicitly before closing the handle: Windows may release
    // LockFileEx locks lazily on CloseHandle, which would make the retry
    // below observe a still-busy init lock.
    init_lock_holder.unlock().unwrap();
    drop(init_lock_holder);
    assert!(IniFile::evict_stale_lock(&temp_dir, 1, &observed).unwrap());
    assert!(!initialized_guard1.still_owns_lock());
    drop(initialized_guard1);
    let _ = fs::remove_dir_all(&temp_dir);
}

/// Tests that a failed retire-rename leaves `netsim.ini` intact so the next
/// starter invocation still observes `Reader` and can retry eviction.
#[cfg(windows)]
#[test]
fn test_ini_evict_failure_keeps_ini_for_retry() {
    use std::os::windows::fs::OpenOptionsExt;

    // `FILE_SHARE_READ | FILE_SHARE_WRITE` from `winnt.h`, deliberately
    // without `FILE_SHARE_DELETE` (0x4).
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;

    let temp_dir = create_temp_dir("evict_failure_keeps_ini");
    let ini_file1 = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let Ok(IniFileAccess::Writer(guard1)) = ini_file1.try_acquire() else {
        panic!("Instance 1 should be Writer");
    };
    let mut data1 = HashMap::new();
    data1.insert("grpc.port".to_string(), "59299".to_string());
    let initialized_guard1 = guard1.write(&data1).unwrap();
    let reader_check = IniFile::new_for_dir(temp_dir.clone(), 1).unwrap();
    let observed = match reader_check.try_acquire() {
        Ok(IniFileAccess::Reader(config)) => config,
        other => panic!("Expected Reader(59299), got {other:?}"),
    };
    // Open without FILE_SHARE_DELETE so the evictor's rename fails.
    let blocker = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(temp_dir.join("netsim.ini.lock"))
        .unwrap();
    let err = IniFile::evict_stale_lock(&temp_dir, 1, &observed).unwrap_err();
    assert!(err.to_string().contains("Failed to retire"), "unexpected eviction error: {err}");
    match IniFile::new_for_dir(temp_dir.clone(), 1).unwrap().try_acquire() {
        Ok(IniFileAccess::Reader(config)) => assert_eq!(config.grpc_port, 59299),
        other => panic!("Expected Reader(59299), got {other:?}"),
    }
    drop(blocker);
    assert!(IniFile::evict_stale_lock(&temp_dir, 1, &observed).unwrap());
    assert!(!initialized_guard1.still_owns_lock());
    drop(initialized_guard1);
    let _ = fs::remove_dir_all(&temp_dir);
}
