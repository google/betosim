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

use std::{collections::HashMap, env, fs, io, path::PathBuf};

use daemon::{IniFile, IniFileAccess};

/// Creates a unique temp directory for a test.
fn create_temp_dir(test_name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("config_test_{}_{}", test_name, rand::random::<u32>()));
    fs::create_dir_all(&dir).expect("Failed to create temp dir");
    dir
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
            let _initialized_guard = guard.write(&data).unwrap();

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
