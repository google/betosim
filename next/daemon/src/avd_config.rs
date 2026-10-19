// Copyright 2025 The Android Open Source Project
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
    collections::{HashMap, HashSet},
    error::Error,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use common::util::ini_file::{IniParserOptions, parse_ini};
use tokio::sync::Mutex;
use tracing::{info, warn};

// This struct matches the top-level configuration.
#[derive(Debug, Default, PartialEq)]
struct NetsimConfig {
    bluetooth_address: Option<String>,
}

fn parse_avd_ini(content: &str) -> Result<HashMap<String, String>, String> {
    parse_ini(content, &IniParserOptions { strict: true }).map_err(|e| e.to_string())
}

// Basic INI serializer for header-less format
fn serialize_ini(config: &NetsimConfig) -> String {
    let mut content = String::new();
    if let Some(address) = &config.bluetooth_address {
        let _ = writeln!(content, "bluetooth.address = {address}");
    }
    content
}

impl NetsimConfig {
    fn from_ini(content: &str) -> Result<Self, String> {
        let map = parse_avd_ini(content)?;
        Ok(NetsimConfig { bluetooth_address: map.get("bluetooth.address").cloned() })
    }
}

// Lists all AVD .ini files or .avd directories in the directory.
fn list_avd_ini_files(avd_root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut ini_files = Vec::new();
    if !avd_root.exists() {
        return Ok(ini_files);
    }
    for entry in fs::read_dir(avd_root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && let Some(extension) = path.extension()
            && extension == "ini"
        {
            ini_files.push(path);
        } else if path.is_dir()
            && (path.extension().and_then(|ext| ext.to_str()) == Some("avd")
                || path.join("netsim.ini").exists())
        {
            ini_files.push(path);
        }
    }
    Ok(ini_files)
}

/// Validates that a Bluetooth MAC address consists of 6 two-digit hex octets
/// separated by colons or hyphens (e.g. "00:11:22:33:44:55" or "00-11-22-33-44-55").
pub fn is_valid_mac_address(addr: &str) -> bool {
    let separator = if addr.contains(':') {
        ':'
    } else if addr.contains('-') {
        '-'
    } else {
        return false;
    };
    let parts: Vec<&str> = addr.split(separator).collect();
    parts.len() == 6
        && parts.iter().all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
}

// Helper function to read the AVD .ini file and extract the 'path' value.
fn get_path_from_avd_ini(ini_content: &str) -> Result<PathBuf, String> {
    for line in ini_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("path=")
            && let Some((_, path_value)) = trimmed.split_once('=')
        {
            return Ok(PathBuf::from(path_value.trim()));
        }
    }
    Err("Could not find the 'path=' key in the AVD .ini file.".to_string())
}

// Reads the AVD-specific .ini file or directory and returns the AVD's data directory path.
fn get_avd_data_path(avd_path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if !avd_path.exists() {
        return Err(format!("AVD path not found: {}", avd_path.display()).into());
    }
    if avd_path.is_dir() {
        return Ok(avd_path.to_path_buf());
    }
    let content = fs::read_to_string(avd_path)?;
    let avd_dir_path = get_path_from_avd_ini(&content)?;
    Ok(avd_dir_path)
}

// Finds the AVD directory and read the netsim.ini file within it.
fn read_netsim_config_for_avd(avd_ini_path: &Path) -> Result<NetsimConfig, Box<dyn Error>> {
    let avd_dir_path = get_avd_data_path(avd_ini_path)?;
    let config_path = avd_dir_path.join("netsim.ini");
    if !config_path.exists() {
        return Ok(NetsimConfig::default());
    }
    let contents = fs::read_to_string(&config_path)?;
    NetsimConfig::from_ini(&contents).map_err(|e| e.into())
}

// Serializes a NetSimConfig and write it to the netsim.ini file
// within the specified AVD's directory.
fn write_netsim_config_for_avd(
    avd_ini_path: &Path,
    config: &NetsimConfig,
) -> Result<(), Box<dyn Error>> {
    let avd_dir_path = get_avd_data_path(avd_ini_path)?;
    // Ensure dir exists
    if !avd_dir_path.exists() {
        fs::create_dir_all(&avd_dir_path)?;
    }
    let config_path = avd_dir_path.join("netsim.ini");
    let contents = serialize_ini(config);
    fs::write(config_path, contents)?;
    Ok(())
}

// Reads all netsim.ini files and returns a set of used Bluetooth addresses.
fn read_all_bluetooth_addresses(avd_root: &Path) -> Result<HashSet<String>, Box<dyn Error>> {
    let mut used_addresses = HashSet::new();
    let ini_files = list_avd_ini_files(avd_root)?;
    for ini_path in ini_files {
        match read_netsim_config_for_avd(&ini_path) {
            Ok(config) => {
                if let Some(address) = config.bluetooth_address {
                    used_addresses.insert(address);
                }
            }
            Err(error) => {
                warn!(
                    %error,
                    path = %ini_path.display(),
                    "Failed to read netsim.ini, skipping address scan"
                )
            }
        }
    }
    Ok(used_addresses)
}

// Generates the next available MAC address from BB:BB:BB:00:00:01 to
// BB:BB:BB:FF:FF:FF.
fn generate_next_mac(used_addresses: &HashSet<String>) -> Result<String, Box<dyn Error>> {
    for i in 1..=0xFFFFFF {
        let b1 = (i >> 16) & 0xFF;
        let b2 = (i >> 8) & 0xFF;
        let b3 = i & 0xFF;
        let mac = format!("BB:BB:BB:{:02X}:{:02X}:{:02X}", b1, b2, b3);
        debug_assert!(is_valid_mac_address(&mac), "Generated MAC must be valid: {mac}");
        if !used_addresses.contains(&mac) {
            return Ok(mac);
        }
    }
    Err("No available MAC addresses in the range BB:BB:BB:00:00:01 - BB:BB:BB:FF:FF:FF".into())
}

/// Retrieves the Bluetooth MAC address for a given AVD name.
/// If the config or address doesn't exist, it finds the next available
/// sequential MAC, creates a new config, and saves it.
fn get_or_create_bluetooth_mac(avd_ini_path: &str) -> Result<String, Box<dyn Error>> {
    // First, check if this AVD already has a configured address.
    let avd_path = PathBuf::from(avd_ini_path);
    match read_netsim_config_for_avd(&avd_path) {
        Ok(config) => {
            if let Some(address) = config.bluetooth_address
                && !address.is_empty()
                && is_valid_mac_address(&address)
            {
                return Ok(address);
            }
        }
        Err(error) => {
            // Log error, but proceed to generate a new MAC as the file might be missing or
            // corrupt.
            warn!(
                %error,
                "Error reading config for {}. Will attempt to create/overwrite.",
                avd_ini_path
            );
        }
    }

    // No address found for this AVD, need to generate a new one.
    let avd_root = avd_path.parent().ok_or("Invalid AVD path")?;
    let used_addresses = read_all_bluetooth_addresses(avd_root)?;
    let new_mac = generate_next_mac(&used_addresses)?;

    let new_config = NetsimConfig { bluetooth_address: Some(new_mac.clone()) };
    write_netsim_config_for_avd(&avd_path, &new_config)?;
    info!("Generated and assigned new MAC {} to AVD {}", new_mac, avd_ini_path);
    Ok(new_mac)
}

/// Public function that reads the netsim.ini config, updates the
/// 'bluetooth.address' field, and writes the entire config back to the AVD
/// directory.
fn set_bluetooth_mac(avd_path: &str, address: &str) -> Result<(), Box<dyn Error>> {
    let avd_ini_path = PathBuf::from(avd_path);
    let mut config = read_netsim_config_for_avd(&avd_ini_path).unwrap_or_default();
    if !address.is_empty() {
        if !is_valid_mac_address(address) {
            return Err(format!("Invalid Bluetooth MAC address: '{address}'").into());
        }
        config.bluetooth_address = Some(address.to_string());
    } else {
        config.bluetooth_address = None;
    }
    write_netsim_config_for_avd(&avd_ini_path, &config)?;
    info!("Successfully updated MAC address for AVD: '{}'", avd_path);
    Ok(())
}

static AVD_CONFIG_LOCK: Mutex<()> = Mutex::const_new(());

/// Resolves or persists the Bluetooth MAC address asynchronously.
///
/// Serializes concurrent calls across async connection streams using an async
/// mutex, and offloads blocking filesystem directory scans and writes
/// to Tokio's blocking thread pool (`spawn_blocking`).
pub async fn resolve_bluetooth_mac(avd_path: &str, provided_address: &str) -> String {
    let guard = AVD_CONFIG_LOCK.lock().await;
    let avd_path = avd_path.to_string();
    let address = provided_address.to_string();
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        if address.is_empty() {
            match get_or_create_bluetooth_mac(&avd_path) {
                Ok(addr) => {
                    info!("Assigned persistent MAC {addr} to AVD {avd_path}");
                    addr
                }
                Err(error) => {
                    warn!(%error, "Failed to get/create MAC for {avd_path}");
                    String::new()
                }
            }
        } else {
            if !is_valid_mac_address(&address) {
                warn!("Refusing to persist invalid MAC address '{address}' for AVD {avd_path}");
                return String::new();
            }
            if let Err(error) = set_bluetooth_mac(&avd_path, &address) {
                warn!(%error, "Failed to persist MAC {address} for AVD {avd_path}");
                return String::new();
            }
            address
        }
    })
    .await
    .unwrap_or_else(|error| {
        warn!(%error, "Failed to join spawn_blocking for resolve_bluetooth_mac");
        String::new()
    })
}
