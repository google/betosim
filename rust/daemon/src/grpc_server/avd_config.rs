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

use log::{info, warn};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

// This struct matches the top-level configuration.
#[derive(Debug, Default, PartialEq)]
struct NetsimConfig {
    bluetooth_address: Option<String>,
}

// Basic INI parser for header-less format
fn parse_ini(content: &str) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(';') || trimmed.is_empty() {
            continue; // Skip comments and empty lines
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        } else {
            return Err(format!("Invalid INI line: {}", line));
        }
    }
    Ok(map)
}

// Basic INI serializer for header-less format
fn serialize_ini(config: &NetsimConfig) -> String {
    let mut content = String::new();
    if let Some(address) = &config.bluetooth_address {
        content.push_str(&format!("bluetooth.address = {address}\n"));
    }
    content
}

impl NetsimConfig {
    fn from_ini(content: &str) -> Result<Self, String> {
        let map = parse_ini(content)?;
        Ok(NetsimConfig { bluetooth_address: map.get("bluetooth.address").cloned() })
    }
}

// Lists all AVD .ini files in the directory.
fn list_avd_ini_files(avd_root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut ini_files = Vec::new();
    if !avd_root.exists() {
        return Ok(ini_files);
    }
    for entry in fs::read_dir(avd_root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(extension) = path.extension() {
                if extension == "ini" {
                    ini_files.push(path);
                }
            }
        }
    }
    Ok(ini_files)
}

// Helper function to read the AVD .ini file and extract the 'path' value.
fn get_path_from_avd_ini(ini_content: &str) -> Result<PathBuf, String> {
    for line in ini_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("path=") {
            if let Some((_, path_value)) = trimmed.split_once('=') {
                return Ok(PathBuf::from(path_value.trim()));
            }
        }
    }
    Err("Could not find the 'path=' key in the AVD .ini file.".to_string())
}

// Reads the AVD-specific .ini file and returns the AVD's data directory path.
pub fn get_avd_data_path(avd_ini_path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if !avd_ini_path.exists() {
        return Err(format!("AVD pointer file not found: {}", avd_ini_path.display()).into());
    }
    let content = fs::read_to_string(avd_ini_path)?;
    let avd_dir_path = get_path_from_avd_ini(&content)?;
    if !avd_dir_path.is_dir() {
        fs::create_dir_all(&avd_dir_path)?;
        info!("Created missing AVD data directory: {}", avd_dir_path.display());
    }
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
            Err(e) => {
                warn!(
                    "Failed to read netsim.ini for {}: {}, skipping address scan",
                    ini_path.display(),
                    e
                )
            }
        }
    }
    Ok(used_addresses)
}

// Generates the next available MAC address from BB:BB:BB:00:00:01 to BB:BB:BB:FF:FF:FF.
fn generate_next_mac(used_addresses: &HashSet<String>) -> Result<String, Box<dyn Error>> {
    for i in 1..=0xFFFFFF {
        let b1 = (i >> 16) & 0xFF;
        let b2 = (i >> 8) & 0xFF;
        let b3 = i & 0xFF;
        let mac = format!("BB:BB:BB:{:02X}:{:02X}:{:02X}", b1, b2, b3);
        if !used_addresses.contains(&mac) {
            return Ok(mac);
        }
    }
    Err("No available MAC addresses in the range BB:BB:BB:00:00:01 - BB:BB:BB:FF:FF:FF".into())
}

/// Retrieves the Bluetooth MAC address for a given AVD name.
/// If the config or address doesn't exist, it finds the next available sequential MAC,
/// creates a new config, and saves it.
pub fn get_or_create_bluetooth_mac(avd_ini_path: &str) -> Result<String, Box<dyn Error>> {
    // First, check if this AVD already has a configured address.
    let avd_path = PathBuf::from(avd_ini_path);
    match read_netsim_config_for_avd(&avd_path) {
        Ok(config) => {
            if let Some(address) = config.bluetooth_address {
                if !address.is_empty() {
                    return Ok(address);
                }
            }
        }
        Err(e) => {
            // Log error, but proceed to generate a new MAC as the file might be missing or corrupt.
            warn!(
                "Error reading config for {}: {}. Will attempt to create/overwrite.",
                avd_ini_path, e
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

/// Public function that reads the netsim.ini config, updates the 'bluetooth.address' field,
/// and writes the entire config back to the AVD directory.
pub fn set_bluetooth_mac(avd_path: &str, address: &str) -> Result<(), Box<dyn Error>> {
    let avd_ini_path = PathBuf::from(avd_path);
    let mut config = read_netsim_config_for_avd(&avd_ini_path).unwrap_or_default();
    if !address.is_empty() {
        config.bluetooth_address = Some(address.to_string());
    } else {
        config.bluetooth_address = None;
    }
    write_netsim_config_for_avd(&avd_ini_path, &config)?;
    info!("Successfully updated MAC address for AVD: '{}'", avd_path);
    Ok(())
}
