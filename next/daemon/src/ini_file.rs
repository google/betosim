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

//! Manages the `netsim.ini` file for inter-process discovery and configuration.
//!
//! This module provides a mechanism to ensure only one primary instance of the
//! netsim daemon is running using a file-based lock on the ini file.
//! It also handles reading and writing configuration parameters (like PID and
//! gRPC port) to the `netsim.ini` file, located in a platform-specific runtime
//! directory.
//!
//! The `IniFile::try_acquire` method is the main entry point, determining if
//! the current process becomes the `Writer` (gets the lock, can write the INI
//! file) or a `Reader` (another process holds the lock, can only read the INI
//! file).

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use common::util::ini_file::{IniParserOptions, parse_ini};
use tracing::warn;

// --- INI File Management ---

/// Parsed configuration from the INI file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NetsimConfig {
    pub pid: Option<u32>,
    pub grpc_port: u16,
    pub uds_path: Option<String>,
}

/// Held while initializing. Can only be used to write the INI file once.
#[derive(Debug)]
pub struct IniFileUninitialized {
    init_lock_file: Option<File>,
    lock_file: Option<File>,
    path: PathBuf,
}

impl IniFileUninitialized {
    fn new(
        path: PathBuf,
        locked_init_lock_file: File,
        locked_lock_file: File,
    ) -> io::Result<Option<Self>> {
        let lock_path = path.with_added_extension("lock");
        if lock_file_retired(&lock_path, file_inode(&locked_lock_file)) {
            // The `.lock` file we opened was retired by `evict_stale_lock`
            // before we acquired `init_lock`; release locks without touching
            // `netsim.ini` so the caller retries on the current `.lock` file.
            return Ok(None);
        }

        // Unlink any leftover INI file under `init_lock` so concurrent readers
        // or `evict_stale_lock` can never observe a stale INI file.
        let _ = fs::remove_file(&path);

        Ok(Some(IniFileUninitialized {
            init_lock_file: Some(locked_init_lock_file),
            lock_file: Some(locked_lock_file),
            path,
        }))
    }

    /// Publishes `netsim.ini` atomically via an exclusive temporary file (`O_EXCL`)
    /// and `rename()` so external readers never observe a partially written configuration.
    pub fn write(mut self, data: &HashMap<String, String>) -> io::Result<IniFileInitialized> {
        let tmp_path = self.path.with_extension("ini.tmp");
        let _ = fs::remove_file(&tmp_path);
        let content: String = data.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?
            .write_all(content.as_bytes())?;
        fs::rename(&tmp_path, &self.path)?;

        // Release init_lock so readers know initialization is complete.
        self.init_lock_file.take();

        Ok(IniFileInitialized { path: self.path.clone(), lock_file: self.lock_file.take() })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for IniFileUninitialized {
    fn drop(&mut self) {
        if self.init_lock_file.is_some() {
            let _ = fs::remove_file(&self.path);
        }
        // Release `.lock` and `.init.lock` while keeping both lock files on
        // disk so concurrent processes always contend on the same lock files.
        drop(self.lock_file.take());
        drop(self.init_lock_file.take());
    }
}

/// Identity of a file independent of its path: `(st_dev, st_ino)` on Unix,
/// `(dwVolumeSerialNumber, nFileIndexHigh:nFileIndexLow)` from
/// `BY_HANDLE_FILE_INFORMATION` on Windows. While a handle to the original
/// file stays open, this identity survives same-volume renames and differs for
/// a file recreated at the original path.
pub(crate) type FileIdentity = (u64, u64);

/// Opens or creates a lock file for read/write lock coordination without
/// truncating it, attaching the path to any I/O error while preserving
/// `ErrorKind`.
fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| io::Error::new(e.kind(), format!("Failed to open {}: {e}", path.display())))
}

#[cfg(unix)]
fn file_inode(file: &File) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    match file.metadata() {
        Ok(m) => Some((m.dev(), m.ino())),
        Err(err) => {
            warn!("Failed to query lock file identity: {err}");
            None
        }
    }
}

#[cfg(windows)]
fn file_inode(file: &File) -> Option<FileIdentity> {
    // `winapi_util::file::information` is a safe wrapper over
    // `GetFileInformationByHandle`; `std` only exposes these fields behind
    // the unstable `windows_by_handle` feature (rust-lang/rust#63010).
    let info = match winapi_util::file::information(file) {
        Ok(info) => info,
        Err(err) => {
            warn!("Failed to query lock file identity: {err}");
            return None;
        }
    };
    let file_index = info.file_index();
    // Per [MS-FSCC] §2.1.9, filesystems that do not support a unique 64-bit
    // file ID return 0 or 0xFFFF_FFFF_FFFF_FFFF ("MUST be ignored").
    if file_index == 0 || file_index == u64::MAX {
        warn!("Lock file filesystem returned non-unique 64-bit file index: {file_index:#x}");
        return None;
    }
    Some((info.volume_serial_number(), file_index))
}

#[cfg(unix)]
fn path_matches_inode(path: &Path, expected_inode: Option<FileIdentity>) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Some((expected_dev, expected_ino)) = expected_inode else {
        return false;
    };
    fs::metadata(path)
        .ok()
        .is_some_and(|disk_meta| disk_meta.dev() == expected_dev && disk_meta.ino() == expected_ino)
}

#[cfg(windows)]
fn path_matches_inode(path: &Path, expected_inode: Option<FileIdentity>) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    let Some(expected) = expected_inode else {
        return false;
    };
    // `access_mode(0)` (`dwDesiredAccess = 0`) requests no data or DELETE
    // access, so the open skips share-mode checks ([MS-FSA] §2.1.5.1.2.2) and
    // cannot fail because of other `.lock` handles. Unlike `fs::metadata`, it
    // does not fall back to `FindFirstFileExW` on `ERROR_ACCESS_DENIED`, so a
    // delete-pending file returns `false`.
    let Ok(disk_file) = OpenOptions::new().access_mode(0).open(path) else {
        return false;
    };
    file_inode(&disk_file) == Some(expected)
}

/// Returns true when the locked `.lock` handle with identity `lock_inode` no
/// longer backs `lock_path`, i.e. `evict_stale_lock` retired it while this
/// process was waiting for `netsim.ini.init.lock`.
///
/// Without a usable identity (`None`, see `file_inode`) retirement cannot be
/// detected, so the handle is treated as current: failing closed here would
/// turn every `Writer` attempt into `Initializing` until startup times out,
/// whereas proceeding matches the behavior before file identities were
/// tracked. The teardown path keeps its conservative reading of `None` in
/// `path_matches_inode`.
fn lock_file_retired(lock_path: &Path, lock_inode: Option<FileIdentity>) -> bool {
    lock_inode.is_some_and(|inode| !path_matches_inode(lock_path, Some(inode)))
}

/// Held for the lifetime of the daemon. It cannot be used to write again.
#[derive(Debug)]
pub struct IniFileInitialized {
    lock_file: Option<File>,
    path: PathBuf,
}

impl IniFileInitialized {
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Returns the platform file identity of the held `.lock` file.
    pub fn lock_inode(&self) -> Option<FileIdentity> {
        self.lock_file.as_ref().and_then(file_inode)
    }

    /// Returns true if `self.lock_file` still refers to the active `.lock`
    /// file on disk (i.e. this daemon has not been evicted by a newer
    /// instance).
    pub fn still_owns_lock(&self) -> bool {
        let lock_path = self.path.with_added_extension("lock");
        path_matches_inode(&lock_path, self.lock_inode())
    }

    /// Unlinks `netsim.ini` only if `expected_lock_inode` still matches the
    /// active `netsim.ini.lock` file on disk, serialized under
    /// `netsim.ini.init.lock`. If `netsim.ini.init.lock` cannot be acquired,
    /// `netsim.ini` is left for the next `Writer` to remove under that lock.
    pub fn unlink_discovery_file_if_owner(
        ini_path: &Path,
        expected_lock_inode: Option<FileIdentity>,
    ) {
        let init_lock_path = ini_path.with_added_extension("init.lock");
        let init_lock =
            match open_lock_file(&init_lock_path).and_then(|file| file.lock().map(|()| file)) {
                Ok(file) => file,
                Err(err) => {
                    warn!(
                        "Skipping unlink of {} because {} could not be acquired: {err}",
                        ini_path.display(),
                        init_lock_path.display()
                    );
                    return;
                }
            };
        let lock_path = ini_path.with_added_extension("lock");
        if path_matches_inode(&lock_path, expected_lock_inode) {
            Self::unlink_discovery_file(ini_path);
        } else if expected_lock_inode.is_some() {
            warn!(
                "Skipping unlink of {} because lock file was replaced by a newer daemon",
                ini_path.display()
            );
        } else {
            warn!(
                "Skipping unlink of {} because lock file identity was unavailable",
                ini_path.display()
            );
        }
        drop(init_lock);
    }

    /// Unlinks `netsim.ini` while the caller holds `netsim.ini.init.lock` (or
    /// `netsim.ini.lock` during normal teardown) so concurrent readers
    /// immediately observe `IniFileAccess::Initializing`.
    /// Note: `netsim.ini.init.lock` is intentionally kept on disk so all
    /// processes always contend on the same coordination lock file.
    fn unlink_discovery_file(ini_path: &Path) {
        if let Err(err) = fs::remove_file(ini_path)
            && err.kind() != io::ErrorKind::NotFound
        {
            warn!("Failed to remove {}: {err}", ini_path.display());
        }
    }
}

impl Drop for IniFileInitialized {
    fn drop(&mut self) {
        let expected_inode = self.lock_inode();
        if let Some(file) = self.lock_file.take() {
            // Unlink netsim.ini while still holding `.lock` only if we were
            // not evicted by a newer daemon.
            Self::unlink_discovery_file_if_owner(&self.path, expected_inode);
            // Explicitly unlock before closing handle as belt-and-braces on
            // Windows so LockFileEx release never lags behind CloseHandle.
            let _ = file.unlock();
            drop(file);
        }
    }
}

/// Represents the access level to the INI file.
#[derive(Debug)]
pub enum IniFileAccess {
    Writer(IniFileUninitialized),
    Reader(NetsimConfig),
    Initializing,
}

/// Manages the lifecycle of a lockable INI file used for daemon status and
/// configuration.
pub struct IniFile {
    path: PathBuf,
    init_lock_path: PathBuf,
    unlocked_lock_file: File,
}

fn discovery_paths(dir: &Path, instance_num: u16) -> (PathBuf, PathBuf, PathBuf) {
    let ini_filename = common::util::ini_file::get_ini_filename(instance_num);
    let path = dir.join(ini_filename);
    let lock_path = path.with_added_extension("lock");
    let init_lock_path = path.with_added_extension("init.lock");
    (path, lock_path, init_lock_path)
}

impl IniFile {
    /// Creates a new `IniFile` manager for an INI file in the specified
    /// directory.
    ///
    /// `instance_num` is necessary to support Cuttlefish multi-instance
    /// environments by creating instance-specific INI and lock files.
    pub fn new_for_dir(dir: PathBuf, instance_num: u16) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let (path, lock_path, init_lock_path) = discovery_paths(&dir, instance_num);

        let unlocked_lock_file = open_lock_file(&lock_path)?;

        Ok(IniFile { path, init_lock_path, unlocked_lock_file })
    }

    /// Attempts to acquire the lock and determine the access level.
    pub fn try_acquire(self) -> io::Result<IniFileAccess> {
        let init_lock = open_lock_file(&self.init_lock_path)?;
        match init_lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(IniFileAccess::Initializing),
            Err(TryLockError::Error(e)) => return Err(e),
        }

        match self.unlocked_lock_file.try_lock() {
            Ok(()) => {
                match IniFileUninitialized::new(self.path, init_lock, self.unlocked_lock_file)? {
                    Some(guard) => Ok(IniFileAccess::Writer(guard)),
                    None => Ok(IniFileAccess::Initializing),
                }
            }
            Err(TryLockError::WouldBlock) => {
                // Either initialization is complete, or the previous owner
                // unlinked netsim.ini at the start of teardown while still
                // holding `.lock`. Treat missing/zero-port INI as
                // `Initializing` so a newly spawned daemon waits for `.lock`
                // release and becomes `Writer`.
                let result = match self.read_config() {
                    Ok(config) if config.grpc_port != 0 => Ok(IniFileAccess::Reader(config)),
                    Ok(_) => Ok(IniFileAccess::Initializing),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        Ok(IniFileAccess::Initializing)
                    }
                    Err(e) => Err(e),
                };
                drop(init_lock);
                result
            }
            Err(TryLockError::Error(e)) => Err(e),
        }
    }

    /// Reads the INI file, enforcing a strict key=value format.
    /// Note: This is a simple parser. It does not handle keys or values
    /// containing '=', quotes, or escape sequences. Comments start with '#'
    /// or ';'.
    fn read_shared(&self) -> io::Result<HashMap<String, String>> {
        let content = fs::read_to_string(&self.path)?;
        parse_ini(&content, &IniParserOptions { strict: true }).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("In `{}`: {}", self.path.display(), e),
            )
        })
    }

    /// Reads and parses the INI file into a NetsimConfig struct.
    fn read_config(&self) -> io::Result<NetsimConfig> {
        let data = self.read_shared()?;
        let mut config = NetsimConfig::default();

        if let Some(pid_str) = data.get("pid") {
            config.pid = match u32::from_str(pid_str) {
                Ok(pid) => Some(pid),
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "Invalid value for pid: {} in {}: {}",
                            pid_str,
                            self.path.display(),
                            e
                        ),
                    ));
                }
            };
        }

        let port_str = data.get("grpc.port").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Missing grpc.port in INI file: {}", self.path.display()),
            )
        })?;
        config.grpc_port = u16::from_str(port_str).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Invalid value for grpc.port: {} in {}: {}",
                    port_str,
                    self.path.display(),
                    e
                ),
            )
        })?;

        if let Some(uds_path_str) = data.get("uds.path") {
            config.uds_path = Some(uds_path_str.to_string());
        }

        Ok(config)
    }

    /// Evicts a stale `netsim.ini` and `netsim.ini.lock` in `dir` for
    /// `instance_num` when the `observed` configuration is confirmed
    /// unresponsive over gRPC.
    ///
    /// Returns `Ok(true)` once the stale `netsim.ini.lock` has been retired and
    /// `Ok(false)` when eviction was skipped because another process holds
    /// `netsim.ini.init.lock` or the discovery files no longer match
    /// `observed`. Callers must re-observe the discovery files after `false`
    /// instead of treating the stale lock as evicted.
    pub fn evict_stale_lock(
        dir: &Path,
        instance_num: u16,
        observed: &NetsimConfig,
    ) -> io::Result<bool> {
        static RETIRED_LOCK_SEQ: AtomicU64 = AtomicU64::new(0);

        fs::create_dir_all(dir)?;
        let (path, lock_path, init_lock_path) = discovery_paths(dir, instance_num);

        let init_lock = open_lock_file(&init_lock_path)?;
        // If `init_lock` is already held, another process is
        // mid-initialization, mid-eviction, or inspecting the discovery
        // files, so report that nothing was evicted and let the caller
        // re-observe (it sees `Initializing` while `init_lock` stays held).
        match init_lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Error(e)) => return Err(e),
        }

        // Re-check under `init_lock`: only evict if `netsim.ini` still
        // advertises the same unresponsive `observed` configuration AND
        // `.lock` is still held. If another process already unlinked or
        // replaced `netsim.ini` (including a new successor bound to the same
        // pinned gRPC port with a different PID), return `Ok(false)` so we
        // never retire the new owner's `.lock`.
        let current_ini = IniFile::new_for_dir(dir.to_path_buf(), instance_num)?;
        let still_stale =
            matches!(
                current_ini.read_config(),
                Ok(ref current) if current == observed
            ) && matches!(current_ini.unlocked_lock_file.try_lock(), Err(TryLockError::WouldBlock));
        if !still_stale {
            return Ok(false);
        }

        // Close our inspection handle to `.lock` before retiring and
        // unlinking it so this process does not keep the retired file open.
        drop(current_ini);

        // Retire `netsim.ini.lock` BEFORE removing `netsim.ini`:
        // - On classic-delete Windows hosts (such as Windows Server 2019 build
        //   17763), `DeleteFileW` (`remove_file`) on a file still held open by
        //   the wedged process marks that directory entry delete-pending until
        //   all handles close, causing subsequent opens of `netsim.ini.lock` to
        //   fail with `ERROR_ACCESS_DENIED`. Same-directory `rename` moves the
        //   locked handle's directory entry aside immediately (`File` opens with
        //   `FILE_SHARE_DELETE` by default on Windows), freeing `netsim.ini.lock`
        //   for the next owner.
        // - Retiring `.lock` first makes eviction failure-atomic: if `rename`
        //   fails (e.g. a transient sharing violation), `netsim.ini` remains
        //   intact so the next starter invocation still enters `Reader` and
        //   retries eviction, whereas removing `netsim.ini` first would strand
        //   subsequent starters in `Initializing`.
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| e.duration())
            .as_nanos();
        let seq = RETIRED_LOCK_SEQ.fetch_add(1, Ordering::Relaxed);
        let retired_lock_path =
            lock_path.with_added_extension(format!("retired.{}.{nanos}.{seq}", std::process::id()));
        match fs::rename(&lock_path, &retired_lock_path) {
            Ok(()) => {
                if let Err(err) = fs::remove_file(&retired_lock_path)
                    && err.kind() != io::ErrorKind::NotFound
                {
                    warn!("Failed to remove {}: {err}", retired_lock_path.display());
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(io::Error::new(
                    err.kind(),
                    format!(
                        "Failed to retire {} to {}: {err}",
                        lock_path.display(),
                        retired_lock_path.display()
                    ),
                ));
            }
        }

        // Once `.lock` is retired, eviction has succeeded; removing the
        // stale `netsim.ini` is best-effort (`IniFileUninitialized::new`
        // also removes any leftover `netsim.ini` when the new Writer starts).
        IniFileInitialized::unlink_discovery_file(&path);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_ini_file_owner_flow() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ini_file = IniFile::new_for_dir(temp_dir.path().to_path_buf(), 1).unwrap();

        match ini_file.try_acquire() {
            Ok(IniFileAccess::Writer(guard)) => {
                let mut data = HashMap::new();
                data.insert("grpc.port".to_string(), "8554".to_string());
                let _initialized_guard = guard.write(&data).unwrap();

                let ini_file2 = IniFile::new_for_dir(temp_dir.path().to_path_buf(), 1).unwrap();
                match ini_file2.try_acquire() {
                    Ok(IniFileAccess::Reader(config)) => {
                        assert_eq!(config.grpc_port, 8554);
                    }
                    _ => panic!("Expected Reader access"),
                }
            }
            _ => panic!("Expected Writer access"),
        }
    }

    #[test]
    fn test_ini_file_multi_instance_paths() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ini_file = IniFile::new_for_dir(temp_dir.path().to_path_buf(), 2).unwrap();
        assert_eq!(ini_file.path.file_name().unwrap(), "netsim_2.ini");
        assert!(temp_dir.path().join("netsim_2.ini.lock").exists());
    }

    #[test]
    fn test_ini_file_draining_and_zero_port() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ini_file = IniFile::new_for_dir(temp_dir.path().to_path_buf(), 1).unwrap();
        let IniFileAccess::Writer(guard) = ini_file.try_acquire().unwrap() else {
            panic!("Expected Writer access");
        };

        let mut data = HashMap::new();
        data.insert("grpc.port".to_string(), "0".to_string());
        let initialized_guard = guard.write(&data).unwrap();

        let ini_file2 = IniFile::new_for_dir(temp_dir.path().to_path_buf(), 1).unwrap();
        assert!(matches!(ini_file2.try_acquire(), Ok(IniFileAccess::Initializing)));

        IniFileInitialized::unlink_discovery_file(initialized_guard.path());
        let ini_file3 = IniFile::new_for_dir(temp_dir.path().to_path_buf(), 1).unwrap();
        assert!(matches!(ini_file3.try_acquire(), Ok(IniFileAccess::Initializing)));
    }

    #[test]
    fn test_file_identity_survives_rename_and_differs_for_recreated_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let a = temp_dir.path().join("netsim.ini.lock");
        let b = temp_dir.path().join("netsim.ini.lock.retired");

        // Create and lock `a`, and record its file identity.
        let fa = File::create(&a).unwrap();
        fa.try_lock().unwrap();
        let id_a = file_inode(&fa);
        assert!(id_a.is_some());
        assert!(path_matches_inode(&a, id_a));

        // Rename `a -> b` while `fa` stays open and holds the exclusive lock:
        // `fa`'s identity is unchanged, `b` has that identity and remains
        // locked, and `a` no longer exists.
        fs::rename(&a, &b).unwrap();
        assert_eq!(file_inode(&fa), id_a);
        assert!(path_matches_inode(&b, id_a));
        assert!(!path_matches_inode(&a, id_a));
        assert!(lock_file_retired(&a, id_a));
        let fb = File::open(&b).unwrap();
        assert!(matches!(fb.try_lock(), Err(TryLockError::WouldBlock)));
        drop(fb);

        // Remove `b` while `fa` stays open (on classic-delete Windows hosts
        // such as Server 2019 this leaves `b` delete-pending; on Unix `b` is
        // unlinked), then recreate `a`. Keeping `fa` open across
        // `File::create(&a)` ensures neither ext4 nor NTFS can reuse `fa`'s
        // inode / MFT record for the new `a`.
        fs::remove_file(&b).unwrap();
        let fa_new = File::create(&a).unwrap();
        let id_a_new = file_inode(&fa_new);
        assert!(id_a_new.is_some());
        assert_eq!(file_inode(&fa), id_a);
        assert_ne!(id_a_new, id_a);
        assert!(!path_matches_inode(&a, id_a));
        assert!(path_matches_inode(&a, id_a_new));
        assert!(lock_file_retired(&a, id_a));
        assert!(!lock_file_retired(&a, id_a_new));

        // `None` never matches an existing path, but an unknown identity is
        // not reported as a retired lock either (see `lock_file_retired`).
        assert!(!path_matches_inode(&a, None));
        assert!(!lock_file_retired(&a, None));

        drop(fa_new);
        drop(fa);
    }
}
