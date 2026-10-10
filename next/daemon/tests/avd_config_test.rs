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

use std::{collections::HashSet, fs, sync::Arc};

use daemon::{is_valid_mac_address, resolve_bluetooth_mac};
use tokio::sync::Barrier;

#[tokio::test]
async fn test_concurrent_resolve_bluetooth_mac_no_collision() {
    // Create 8 wiped AVDs (no netsim.ini yet) sharing the same avd_root.
    let temp_dir = tempfile::tempdir().unwrap();
    let avd_inis: Vec<String> = (0..8)
        .map(|i| {
            let avd_dir = temp_dir.path().join(format!("avd_{i}.avd"));
            fs::create_dir_all(&avd_dir).unwrap();
            let ini = temp_dir.path().join(format!("avd_{i}.ini"));
            fs::write(&ini, format!("path={}\n", avd_dir.display())).unwrap();
            ini.to_string_lossy().into_owned()
        })
        .collect();

    // Synchronize all tasks at a barrier to simulate concurrent AVD boot
    // streams racing to scan avd_root and persist their assigned MACs.
    let count = avd_inis.len();
    let barrier = Arc::new(Barrier::new(count));
    let mut handles = Vec::new();

    for ini in &avd_inis {
        let b = barrier.clone();
        let ini_path = ini.clone();
        handles.push(tokio::spawn(async move {
            b.wait().await;
            resolve_bluetooth_mac(&ini_path, "").await
        }));
    }

    let mut assigned_macs = HashSet::new();
    for handle in handles {
        let mac = handle.await.unwrap();
        assert!(!mac.is_empty(), "Assigned MAC should not be empty");
        assigned_macs.insert(mac);
    }

    // Every AVD must receive a distinct sequential MAC address.
    assert_eq!(
        assigned_macs.len(),
        count,
        "Expected {count} distinct MAC addresses, but got {assigned_macs:?}"
    );

    // Explicit overrides must persist to netsim.ini for subsequent lookups.
    let override_mac = "BB:BB:BB:00:00:99";
    assert_eq!(resolve_bluetooth_mac(&avd_inis[0], override_mac).await, override_mac);
    assert_eq!(resolve_bluetooth_mac(&avd_inis[0], "").await, override_mac);

    // Verify netsim.ini is persisted with the override MAC.
    let avd_0_dir = temp_dir.path().join("avd_0.avd");
    let content = fs::read_to_string(avd_0_dir.join("netsim.ini")).unwrap();
    assert!(content.contains(&format!("bluetooth.address = {override_mac}")));
}

#[tokio::test]
async fn test_existing_valid_netsim_ini_preserves_mac() {
    let temp_dir = tempfile::tempdir().unwrap();
    let avd_dir = temp_dir.path().join("existing.avd");
    fs::create_dir_all(&avd_dir).unwrap();

    // Pre-populate netsim.ini with a specific MAC.
    let existing_mac = "BB:BB:BB:00:00:42";
    fs::write(avd_dir.join("netsim.ini"), format!("bluetooth.address = {existing_mac}\n")).unwrap();

    let ini = temp_dir.path().join("existing.ini");
    fs::write(&ini, format!("path={}\n", avd_dir.display())).unwrap();
    let ini_path = ini.to_string_lossy().into_owned();

    // Resolving without provided MAC must return the existing configured address.
    let resolved = resolve_bluetooth_mac(&ini_path, "").await;
    assert_eq!(resolved, existing_mac);
}

#[tokio::test]
async fn test_corrupt_netsim_ini_recovers_with_new_mac() {
    let temp_dir = tempfile::tempdir().unwrap();
    let avd_dir = temp_dir.path().join("corrupt.avd");
    fs::create_dir_all(&avd_dir).unwrap();

    // Create an invalid/corrupt netsim.ini (0 bytes or malformed).
    fs::write(avd_dir.join("netsim.ini"), "malformed_entry_no_equals\n").unwrap();

    let ini = temp_dir.path().join("corrupt.ini");
    fs::write(&ini, format!("path={}\n", avd_dir.display())).unwrap();
    let ini_path = ini.to_string_lossy().into_owned();

    // Resolving should recover gracefully and assign a valid sequential MAC.
    let resolved = resolve_bluetooth_mac(&ini_path, "").await;
    assert!(resolved.starts_with("BB:BB:BB:"));
    assert_ne!(resolved, "");

    // Subsequent resolution returns the newly recovered MAC.
    let resolved_again = resolve_bluetooth_mac(&ini_path, "").await;
    assert_eq!(resolved_again, resolved);
}

#[tokio::test]
async fn test_resolve_bluetooth_mac_cancellation_safety() {
    let temp_dir = tempfile::tempdir().unwrap();
    let avd_1_dir = temp_dir.path().join("avd_1.avd");
    fs::create_dir_all(&avd_1_dir).unwrap();
    let ini_1 = temp_dir.path().join("avd_1.ini");
    fs::write(&ini_1, format!("path={}\n", avd_1_dir.display())).unwrap();
    let ini_path_1 = ini_1.to_string_lossy().into_owned();

    let avd_2_dir = temp_dir.path().join("avd_2.avd");
    fs::create_dir_all(&avd_2_dir).unwrap();
    let ini_2 = temp_dir.path().join("avd_2.ini");
    fs::write(&ini_2, format!("path={}\n", avd_2_dir.display())).unwrap();
    let ini_path_2 = ini_2.to_string_lossy().into_owned();

    // Spawn a task for AVD 1 and abort/cancel its async future.
    let ini_path_1_clone = ini_path_1.clone();
    let task_1 = tokio::spawn(async move { resolve_bluetooth_mac(&ini_path_1_clone, "").await });
    // Abort the task (cancelling the outer future while spawn_blocking may be in flight).
    task_1.abort();

    // Concurrently resolve MAC for AVD 2.
    // The lock guard moved into spawn_blocking ensures AVD 1's blocking work
    // finishes and releases the lock cleanly before AVD 2 proceeds.
    let mac_2 = resolve_bluetooth_mac(&ini_path_2, "").await;
    assert!(!mac_2.is_empty());

    // Resolve AVD 1 to verify its configuration is valid and has a distinct MAC.
    let mac_1 = resolve_bluetooth_mac(&ini_path_1, "").await;
    assert!(!mac_1.is_empty());
    assert_ne!(mac_1, mac_2, "Aborted task must not cause MAC collision with subsequent tasks");
}

#[test]
fn test_is_valid_mac_address() {
    assert!(is_valid_mac_address("00:11:22:33:44:55"));
    assert!(is_valid_mac_address("AA:BB:CC:DD:EE:FF"));
    assert!(is_valid_mac_address("aa:bb:cc:dd:ee:ff"));
    assert!(is_valid_mac_address("00-11-22-33-44-55"));

    assert!(!is_valid_mac_address(""));
    assert!(!is_valid_mac_address("00:11:22:33:44"));
    assert!(!is_valid_mac_address("00:11:22:33:44:55:66"));
    assert!(!is_valid_mac_address("00:11:22:33:44:GG"));
    assert!(!is_valid_mac_address("00:11:22:33:44:55\n"));
    assert!(!is_valid_mac_address("00:11:22:33:44:55\ninjection = 1"));
    assert!(!is_valid_mac_address("malicious_key = 1"));
}

#[tokio::test]
async fn test_resolve_bluetooth_mac_rejects_invalid_provided_address() {
    let temp_dir = tempfile::tempdir().unwrap();
    let avd_dir = temp_dir.path().join("secure.avd");
    fs::create_dir_all(&avd_dir).unwrap();
    let ini = temp_dir.path().join("secure.ini");
    fs::write(&ini, format!("path={}\n", avd_dir.display())).unwrap();
    let ini_path = ini.to_string_lossy().into_owned();

    // Passing invalid MAC or newline injection attempt must fail hard and return empty string.
    let malformed_input = "AA:BB:CC:DD:EE:FF\nmalicious.key = injected";
    let resolved = resolve_bluetooth_mac(&ini_path, malformed_input).await;
    assert_eq!(resolved, "");

    // Verify netsim.ini was not created or poisoned.
    assert!(!avd_dir.join("netsim.ini").exists());
}

#[tokio::test]
async fn test_resolve_bluetooth_mac_with_directory_avd_path() {
    let temp_dir = tempfile::tempdir().unwrap();
    let avd_dir = temp_dir.path().join("direct.avd");
    fs::create_dir_all(&avd_dir).unwrap();
    let avd_dir_path = avd_dir.to_string_lossy().into_owned();

    let mac = resolve_bluetooth_mac(&avd_dir_path, "").await;
    assert!(mac.starts_with("BB:BB:BB:"));
    assert!(!mac.is_empty());

    // Verify netsim.ini was created directly inside the directory.
    assert!(avd_dir.join("netsim.ini").exists());

    // Subsequent resolution returns the same MAC.
    let resolved_again = resolve_bluetooth_mac(&avd_dir_path, "").await;
    assert_eq!(resolved_again, mac);
}

#[tokio::test]
async fn test_resolve_bluetooth_mac_rejects_nonexistent_avd_path() {
    let temp_dir = tempfile::tempdir().unwrap();
    let nonexistent = temp_dir.path().join("does_not_exist.avd");

    let resolved = resolve_bluetooth_mac(&nonexistent.to_string_lossy(), "").await;
    assert_eq!(resolved, "");
}
