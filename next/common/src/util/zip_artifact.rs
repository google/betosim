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

//! # Zip Artifact Utilities
//!
//! Synchronous file I/O (`std::fs`) and compression (`zip::ZipWriter`) are used
//! intentionally throughout this module instead of async equivalents:
//! - **Startup cleanup** (`remove_old_artifacts`): Runs once after acquiring
//!   the daemon INI lock and before spawning async actors or accepting network
//!   connections.
//! - **Shutdown archiving** (`zip_artifacts`): Runs once during daemon shutdown
//!   after the main event loop has exited and all actors and background tasks
//!   have terminated.
//!
//! Because no concurrent async tasks or packet streams are active at these
//! lifecycle boundaries, blocking the thread is safe and avoids unnecessary
//! async overhead.

use std::{
    fs::{File, read_dir, remove_file},
    io::{self, Result},
    path::{Path, PathBuf},
};

use tracing::{error, info, warn};
use zip::{ZipWriter, result::ZipResult, write::FileOptions};

use super::time_display::file_current_time;
use crate::system::netsimd_temp_dir;

/// Collect all files in root recursively using iterative DFS.
fn recurse_files(root: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut result = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if dir != root && e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            // Use file_type() rather than metadata() so symlinks are not followed.
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            if file_type.is_symlink() {
                continue;
            } else if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                if let Some(filename) = entry
                    .path()
                    .file_name()
                    .and_then(|os_name| os_name.to_str())
                    .map(|str_name| str_name.to_string())
                {
                    result.push((entry.path(), filename));
                } else {
                    warn!("Unable to fetch filename for file: {}", entry.path().display());
                }
            }
        }
    }
    Ok(result)
}

/// Fetch all zip files in root and put it in sorted Vec<PathBuf>
fn fetch_zip_files(root: &Path) -> Result<Vec<PathBuf>> {
    // Read all entries in the given root directory
    // Push path to result if name matches "netsim_artifacts_*.zip"
    let mut result: Vec<PathBuf> = read_dir(root)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|ft| ft.is_file()))
        .map(|e| e.path())
        .filter(|path| {
            path.file_name().and_then(|os_name| os_name.to_str()).is_some_and(|filename| {
                filename.starts_with("netsim_artifacts_") && filename.ends_with(".zip")
            })
        })
        .collect();
    // Sort the zip files by timestamp from oldest to newest
    result.sort();
    Ok(result)
}

/// Remove set number of zip files in a specific directory
fn remove_zip_files_in_dir(root: &Path) -> Result<usize> {
    // TODO(b/305012017): Add parameter for keeping some number of zip files
    let zip_files = fetch_zip_files(root)?;
    let mut removed = 0;
    for file in zip_files {
        match std::fs::remove_file(&file) {
            Ok(()) => removed += 1,
            Err(err) => warn!("Failed to remove file {}: {err}", file.display()),
        }
    }
    Ok(removed)
}

/// Method for clearing pcap files in a specific directory
fn clear_pcap_files_in_dir(root: &Path) -> bool {
    let path = root.join("pcaps");
    match std::fs::remove_dir_all(&path) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            warn!("Failed to remove directory {}: {err}", path.display());
            false
        }
    }
}

/// Remove old artifacts (zip files and pcaps) in a specific directory
pub fn remove_old_artifacts_in_dir(root: &Path) {
    // Clear all zip files
    match remove_zip_files_in_dir(root) {
        Ok(count) if count > 0 => {
            info!("Removed {count} netsim generated zip files in temp directory.");
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => error!("Failed to remove zip files in {}: {err}", root.display()),
    }

    // Clear all pcap files
    if clear_pcap_files_in_dir(root) {
        info!("netsim generated pcap files in temp directory have been removed.");
    }
}

/// Remove old artifacts (zip files and pcaps)
pub fn remove_old_artifacts() {
    remove_old_artifacts_in_dir(&netsimd_temp_dir());
}

/// Zip the whole specified directory and store the archive in the same
/// directory.
pub fn zip_artifacts_in_dir(root: &Path) -> ZipResult<()> {
    let files = match recurse_files(root) {
        Ok(files) => files,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    // Define PathBuf for zip file
    let zip_file = root.join(format!("netsim_artifacts_{}.zip", file_current_time()));

    // Create a new ZipWriter
    let mut zip_writer = ZipWriter::new(File::create(zip_file)?);

    // Put each artifact files into zip file
    for (file, filename) in files {
        // Avoid zip files
        if filename.starts_with("netsim_artifacts") {
            continue;
        }

        {
            let mut f = match File::open(&file) {
                Ok(f) => f,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    warn!("Failed to open artifact file {}: {err}", file.display());
                    continue;
                }
            };

            // Write to zip file
            zip_writer.start_file(&filename, FileOptions::default())?;
            io::copy(&mut f, &mut zip_writer)?;
        }

        // Remove the file once written except for netsim log and json files
        if filename.starts_with("netsim_")
            && (filename.ends_with(".log") || filename.ends_with(".json"))
        {
            continue;
        }
        match remove_file(&file) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => warn!("Failed to remove artifact file {}: {err}", file.display()),
        }
    }

    // Finish writing zip file
    zip_writer.finish()?;
    Ok(())
}

/// Zip the whole netsimd temp directory and store it in temp directory.
pub fn zip_artifacts() -> ZipResult<()> {
    zip_artifacts_in_dir(&netsimd_temp_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recurse_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // Create a file
        let file = path.join("hello.txt");
        File::create(&file).unwrap();

        // Create a folder and a file inside
        let folder = path.join("folder");
        std::fs::create_dir_all(&folder).unwrap();
        let nested_file = folder.join("world.txt");
        File::create(&nested_file).unwrap();

        // Recurse Files and check the contents
        let files_result = recurse_files(path);
        assert!(files_result.is_ok());
        let files = files_result.unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.contains(&(file, "hello.txt".to_string())));
        assert!(files.contains(&(nested_file, "world.txt".to_string())));
    }

    #[test]
    fn test_fetch_zip_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // Create multiple zip files in random order of timestamps
        let zip_file_1 = path.join("netsim_artifacts_2024-01-01.zip");
        let zip_file_2 = path.join("netsim_artifacts_2022-12-31.zip");
        let zip_file_3 = path.join("netsim_artifacts_2023-06-01.zip");
        let zip_file_faulty = path.join("netsim_arts_2000-01-01.zip");
        File::create(&zip_file_1).unwrap();
        File::create(&zip_file_2).unwrap();
        File::create(&zip_file_3).unwrap();
        File::create(zip_file_faulty).unwrap();

        // Fetch all zip files and check the contents if it is in order
        let files_result = fetch_zip_files(path);
        assert!(files_result.is_ok());
        let files = files_result.unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files.first().unwrap(), &zip_file_2);
        assert_eq!(files.get(1).unwrap(), &zip_file_3);
        assert_eq!(files.get(2).unwrap(), &zip_file_1);
    }

    #[test]
    fn test_zip_and_remove_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // 1. Setup sample artifacts: pcap directory, transient file, and persistent
        //    log/json files
        let pcap_dir = path.join("pcaps");
        std::fs::create_dir_all(&pcap_dir).unwrap();
        let pcap_file = pcap_dir.join("capture.pcap");
        std::fs::write(&pcap_file, b"pcap data").unwrap();

        let transient_file = path.join("session_temp.txt");
        std::fs::write(&transient_file, b"transient data").unwrap();

        let log_file = path.join("netsim_daemon.log");
        std::fs::write(&log_file, b"log content").unwrap();

        let json_file = path.join("netsim_session_stats.json");
        std::fs::write(&json_file, b"{}").unwrap();

        // 2. Zip artifacts
        assert!(zip_artifacts_in_dir(path).is_ok());

        // Verify zip file was created
        let zips = fetch_zip_files(path).unwrap();
        assert_eq!(zips.len(), 1);

        // Verify transient and pcap files were removed after being zipped, while log
        // and json persist
        assert!(!transient_file.exists(), "Transient file should be removed after zip");
        assert!(!pcap_file.exists(), "Pcap file should be removed after zip");
        assert!(log_file.exists(), "netsim_*.log should be preserved after zip");
        assert!(json_file.exists(), "netsim_*.json should be preserved after zip");

        // 3. Re-create pcaps directory and test remove_old_artifacts_in_dir (startup
        //    cleanup)
        std::fs::create_dir_all(&pcap_dir).unwrap();
        std::fs::write(&pcap_file, b"new pcap").unwrap();
        assert!(pcap_dir.exists());

        remove_old_artifacts_in_dir(path);

        // Verify both zip archives and pcaps directory were cleared
        assert!(fetch_zip_files(path).unwrap().is_empty(), "Zip archives should be cleared");
        assert!(!pcap_dir.exists(), "pcaps directory should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn test_symlinks_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let target_dir = tempfile::tempdir().unwrap();
        let target_file = target_dir.path().join("sensitive.txt");
        std::fs::write(&target_file, b"secret").unwrap();

        let symlink_path = path.join("symlink_file.txt");
        std::os::unix::fs::symlink(&target_file, &symlink_path).unwrap();

        assert!(zip_artifacts_in_dir(path).is_ok());

        // Target file and symlink must remain untouched (not archived or deleted)
        assert!(target_file.exists(), "Symlink target must not be deleted");
        assert!(symlink_path.symlink_metadata().is_ok(), "Symlink itself should be ignored");
    }
}
