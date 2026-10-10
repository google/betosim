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
    path::PathBuf,
    str::FromStr,
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
    init_lock_path: PathBuf,
}

impl IniFileUninitialized {
    pub fn new(path: PathBuf, init_lock_path: PathBuf, locked_lock_file: File) -> io::Result<Self> {
        // Unlink any leftover INI file BEFORE creating/locking init_lock so
        // concurrent readers can never observe a stale INI file from a crashed run.
        let _ = fs::remove_file(&path);
        let init_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&init_lock_path)?;
        init_lock.lock()?;

        Ok(IniFileUninitialized {
            init_lock_file: Some(init_lock),
            lock_file: Some(locked_lock_file),
            path,
            init_lock_path,
        })
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
        if let Some(file) = self.init_lock_file.take() {
            drop(file);
            if let Err(err) = fs::remove_file(&self.init_lock_path)
                && err.kind() != io::ErrorKind::NotFound
            {
                warn!("Failed to remove {}: {err}", self.init_lock_path.display());
            }
        }
        // Release `.lock` last while keeping the `.lock` file on disk so
        // concurrent processes always contend on the same inode.
        drop(self.lock_file.take());
    }
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

    /// Unlinks `netsim.ini` and `netsim.ini.init.lock` while the caller holds
    /// `netsim.ini.lock` so concurrent readers immediately observe
    /// `IniFileAccess::Initializing`.
    pub fn unlink_discovery_files(ini_path: &std::path::Path) {
        for path in [ini_path, &ini_path.with_extension("ini.init.lock")] {
            if let Err(err) = fs::remove_file(path)
                && err.kind() != io::ErrorKind::NotFound
            {
                warn!("Failed to remove {}: {err}", path.display());
            }
        }
    }
}

impl Drop for IniFileInitialized {
    fn drop(&mut self) {
        if let Some(file) = self.lock_file.take() {
            // Unlink netsim.ini and init.lock while still holding `.lock` so no
            // concurrent reader can observe a stale port after lock release.
            Self::unlink_discovery_files(&self.path);
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

impl IniFile {
    /// Creates a new `IniFile` manager for an INI file in the specified
    /// directory.
    ///
    /// `instance_num` is necessary to support Cuttlefish multi-instance
    /// environments by creating instance-specific INI and lock files.
    pub fn new_for_dir(dir: PathBuf, instance_num: u16) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let ini_filename = common::util::ini_file::get_ini_filename(instance_num);
        let lock_path = dir.join(format!("{ini_filename}.lock"));
        let init_lock_path = dir.join(format!("{ini_filename}.init.lock"));
        let path = dir.join(ini_filename);

        let unlocked_lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;

        Ok(IniFile { path, init_lock_path, unlocked_lock_file })
    }

    /// Attempts to acquire the lock and determine the access level.
    pub fn try_acquire(self) -> io::Result<IniFileAccess> {
        match self.unlocked_lock_file.try_lock() {
            Ok(()) => Ok(IniFileAccess::Writer(IniFileUninitialized::new(
                self.path,
                self.init_lock_path,
                self.unlocked_lock_file,
            )?)),
            Err(TryLockError::WouldBlock) => {
                let init_lock_result = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .truncate(false)
                    .open(&self.init_lock_path);

                match init_lock_result {
                    Ok(init_lock) => match init_lock.try_lock() {
                        Ok(()) => {
                            // Either initialization is complete, or the previous
                            // owner unlinked netsim.ini at the start of teardown
                            // while still holding `.lock`. Treat missing/zero-port
                            // INI as `Initializing` so a newly spawned daemon
                            // waits for `.lock` release and becomes `Writer`.
                            drop(init_lock);
                            match self.read_config() {
                                Ok(config) if config.grpc_port != 0 => {
                                    Ok(IniFileAccess::Reader(config))
                                }
                                Ok(_) => Ok(IniFileAccess::Initializing),
                                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                                    Ok(IniFileAccess::Initializing)
                                }
                                Err(e) => Err(e),
                            }
                        }
                        Err(TryLockError::WouldBlock) => Ok(IniFileAccess::Initializing),
                        Err(TryLockError::Error(e)) => Err(e),
                    },
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        // Owner hasn't created the init lock file yet
                        Ok(IniFileAccess::Initializing)
                    }
                    Err(e) => Err(e),
                }
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

        IniFileInitialized::unlink_discovery_files(initialized_guard.path());
        let ini_file3 = IniFile::new_for_dir(temp_dir.path().to_path_buf(), 1).unwrap();
        assert!(matches!(ini_file3.try_acquire(), Ok(IniFileAccess::Initializing)));
    }
}
