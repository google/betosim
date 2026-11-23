//  Copyright 2023 The Android Open Source Project
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

//! # os utility functions

#[cfg(windows)]
use std::os::windows::io::{FromRawHandle, IntoRawHandle};
use std::{
    env,
    fmt::Write as _,
    fs::File,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use tracing::warn;

use crate::system::netsimd_temp_dir;

const DEFAULT_HCI_PORT: u32 = 6402;

struct DiscoveryDir {
    root_env: &'static str,
    subdir: &'static str,
}

#[cfg(target_os = "linux")]
const DISCOVERY: DiscoveryDir = DiscoveryDir { root_env: "XDG_RUNTIME_DIR", subdir: "" };
#[cfg(target_os = "macos")]
const DISCOVERY: DiscoveryDir =
    DiscoveryDir { root_env: "HOME", subdir: "Library/Caches/TemporaryItems" };
#[cfg(target_os = "windows")]
const DISCOVERY: DiscoveryDir = DiscoveryDir { root_env: "LOCALAPPDATA", subdir: "Temp" };
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!("netsim only supports linux, Mac, and Windows");

/// Get discovery directory for netsim
pub fn get_discovery_directory() -> PathBuf {
    get_discovery_directory_with_env(|k| env::var(k))
}

pub(crate) fn get_discovery_directory_with_env<F>(get_env: F) -> PathBuf
where
    F: Fn(&str) -> Result<String, env::VarError>,
{
    // $TMPDIR is the temp directory on buildbots
    if let Ok(test_env_p) = get_env("TMPDIR") {
        return PathBuf::from(test_env_p);
    }
    let mut path = if let Ok(env_p) = get_env(DISCOVERY.root_env) {
        PathBuf::from(env_p)
    } else {
        warn!(
            code = crate::err::Code::DiscoveryEnvMissing as u32,
            "No discovery env for {}, using /tmp", DISCOVERY.root_env
        );
        PathBuf::from("/tmp")
    };
    path.push(DISCOVERY.subdir);
    path
}

const DEFAULT_INSTANCE: u16 = 1;

/// Get the netsim instance number which is always > 0
///
/// The following priorities are used to determine the instance number:
///
/// 1. The environment variable `NETSIM_INSTANCE`.
/// 2. The CLI flag `--instance`.
/// 3. The default value `DEFAULT_INSTANCE`.
pub fn get_instance(instance_flag: Option<u16>) -> u16 {
    get_instance_with_env(|k| env::var(k), instance_flag)
}

fn get_instance_with_env<F>(get_env: F, instance_flag: Option<u16>) -> u16
where
    F: Fn(&str) -> Result<String, env::VarError>,
{
    let instance_env: Option<u16> = get_env("NETSIM_INSTANCE").ok().and_then(|i| i.parse().ok());
    match (instance_env, instance_flag) {
        (Some(i), _) | (_, Some(i)) if i > 0 => i,
        (_, _) => DEFAULT_INSTANCE,
    }
}

/// Get the hci port number for netsim
///
/// `hci_port_flag` comes from cli args or the `NETSIM_HCI_PORT` env var.
pub fn get_hci_port(hci_port_flag: u32, instance: u16) -> u32 {
    if hci_port_flag != 0 { hci_port_flag } else { DEFAULT_HCI_PORT + u32::from(instance) }
}

/// Get the netsim instance name used for log filename creation
pub fn get_instance_name(instance_num: Option<u16>, connector_instance: Option<u16>) -> String {
    get_instance_name_with_env(|k| env::var(k), instance_num, connector_instance)
}

fn get_instance_name_with_env<F>(
    get_env: F,
    instance_num: Option<u16>,
    connector_instance: Option<u16>,
) -> String
where
    F: Fn(&str) -> Result<String, env::VarError>,
{
    let mut instance_name = String::new();
    let instance = get_instance_with_env(get_env, instance_num);
    if instance > 1 {
        let _ = write!(instance_name, "{instance}_");
    }
    // Note: This does not differentiate multiple connectors to the same instance.
    if connector_instance.is_some() {
        instance_name.push_str("connector_");
    }
    instance_name
}

/// Redirects the whole process's stdout and stderr to log files in [`netsimd_temp_dir`].
///
/// Creates (truncating) `netsim_{instance_name}stdout.log` and
/// `netsim_{instance_name}stderr.log` and points stdout and stderr at them so
/// Rust, C, and C++ output all go to the logs. Flushes Rust stdio before
/// swapping; any buffered C stdio output written beforehand is not flushed.
///
/// Safe to call again: each call replaces the previous redirect and closes the
/// old files. If either file cannot be created, nothing is redirected; a later
/// failure can leave stdout redirected but not stderr.
pub fn redirect_std_stream(instance_name: &str) -> io::Result<()> {
    redirect_std_stream_in(&netsimd_temp_dir(), instance_name)
}

fn redirect_std_stream_in(dir: &Path, instance_name: &str) -> io::Result<()> {
    let stdout_file = File::create(dir.join(format!("netsim_{instance_name}stdout.log")))?;
    let stderr_file = File::create(dir.join(format!("netsim_{instance_name}stderr.log")))?;

    // Hold the Rust stdio locks so other threads can't write mid-swap.
    let mut stdout_lock = io::stdout().lock();
    let mut stderr_lock = io::stderr().lock();
    let _ = stdout_lock.flush();
    let _ = stderr_lock.flush();

    redirect_stdio(stdout_file, stderr_file)?;

    drop((stdout_lock, stderr_lock));
    Ok(())
}

#[cfg(unix)]
fn redirect_stdio(stdout_file: File, stderr_file: File) -> io::Result<()> {
    // try_clone dups with F_DUPFD_CLOEXEC at fd >= 3, so the logs can't alias stdio.
    let (stdout, stderr) = (stdout_file.try_clone()?, stderr_file.try_clone()?);
    drop((stdout_file, stderr_file));
    nix::unistd::dup2_stdout(&stdout)?;
    nix::unistd::dup2_stderr(&stderr)?;
    Ok(())
}

#[cfg(windows)]
fn redirect_stdio(stdout_file: File, stderr_file: File) -> io::Result<()> {
    redirect_stream(stdout_file, StdStream::Stdout)?;
    redirect_stream(stderr_file, StdStream::Stderr)?;
    Ok(())
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum StdStream {
    Stdout = 1,
    Stderr = 2,
}

/// Points stdout or stderr (CRT fd and, via `_dup2`, the Win32 std handle) at `file`.
#[cfg(windows)]
fn redirect_stream(file: File, stream: StdStream) -> io::Result<()> {
    let target_fd = stream as libc::c_int;
    let raw = file.into_raw_handle();
    // SAFETY: `raw` is an open handle we own; on success the CRT fd takes ownership.
    let fd = unsafe { libc::open_osfhandle(raw as libc::intptr_t, libc::O_WRONLY | libc::O_TEXT) };
    if fd == -1 {
        // SAFETY: `_open_osfhandle` failed, so `raw` is still open and owned only by us.
        drop(unsafe { File::from_raw_handle(raw) });
        return Err(io::Error::other("_open_osfhandle failed"));
    }
    // UCRT `_dup2` updates the Win32 std handles for console apps, so Rust stdio follows.
    // SAFETY: `fd` is an open CRT fd and `stream` is this process's stdout or stderr fd.
    let dup_res = unsafe { libc::dup2(fd, target_fd) };
    if fd != target_fd {
        // SAFETY: `fd` is owned from `_open_osfhandle` and is not used after this call.
        let _ = unsafe { libc::close(fd) };
    }
    if dup_res != 0 {
        return Err(io::Error::other("_dup2 failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_discovery_directory() {
        // Test with no environment variables
        let mock_env_none = |_key: &str| Err(env::VarError::NotPresent);
        let actual = get_discovery_directory_with_env(mock_env_none);
        let mut expected = PathBuf::from("/tmp");
        expected.push(DISCOVERY.subdir);
        assert_eq!(actual, expected);

        // Test with root_env variable
        let mock_env_root = |key: &str| {
            if key == DISCOVERY.root_env {
                Ok("/netsim-test".to_string())
            } else {
                Err(env::VarError::NotPresent)
            }
        };
        let actual = get_discovery_directory_with_env(mock_env_root);
        let mut expected = PathBuf::from("/netsim-test");
        expected.push(DISCOVERY.subdir);
        assert_eq!(actual, expected);

        // Test with TMPDIR variable
        let mock_env_tmpdir = |key: &str| {
            if key == "TMPDIR" { Ok("/tmpdir".to_string()) } else { Err(env::VarError::NotPresent) }
        };
        assert_eq!(get_discovery_directory_with_env(mock_env_tmpdir), PathBuf::from("/tmpdir"));
    }

    #[test]
    fn test_get_instance_and_instance_name() {
        // Set NETSIM_INSTANCE environment variable
        let mock_env_100 = |key: &str| {
            if key == "NETSIM_INSTANCE" {
                Ok("100".to_string())
            } else {
                Err(env::VarError::NotPresent)
            }
        };
        assert_eq!(get_instance_with_env(mock_env_100, Some(0)), 100);
        assert_eq!(get_instance_with_env(mock_env_100, Some(1)), 100);

        // Remove NETSIM_INSTANCE environment variable
        let mock_env_none = |_key: &str| Err(env::VarError::NotPresent);
        assert_eq!(get_instance_with_env(mock_env_none, None), DEFAULT_INSTANCE);
        assert_eq!(get_instance_with_env(mock_env_none, Some(0)), DEFAULT_INSTANCE);
        assert_eq!(get_instance_with_env(mock_env_none, Some(1)), 1);

        // Default cases - instance name should be empty string
        assert_eq!(get_instance_name_with_env(mock_env_none, None, None), "");
        assert_eq!(get_instance_name_with_env(mock_env_none, Some(1), None), "");

        // Default instance but connector set - Expect instance name to be "connector_"
        assert_eq!(get_instance_name_with_env(mock_env_none, None, Some(3)), "connector_");
        assert_eq!(get_instance_name_with_env(mock_env_none, Some(1), Some(1)), "connector_");
        assert_eq!(get_instance_name_with_env(mock_env_none, Some(1), Some(2)), "connector_");

        // Both instance and connector set - Expect instance name to be
        // "<instance>_connector_"
        assert_eq!(get_instance_name_with_env(mock_env_none, Some(2), Some(1)), "2_connector_");
        assert_eq!(get_instance_name_with_env(mock_env_none, Some(3), Some(3)), "3_connector_");
    }

    #[test]
    fn test_get_hci_port() {
        // Test if hci_port flag is honored
        assert_eq!(get_hci_port(1, u16::MAX), 1);
        assert_eq!(get_hci_port(1, u16::MIN), 1);

        // hci_port_flag = 0 should fall back to the default
        assert_eq!(get_hci_port(0, 0), DEFAULT_HCI_PORT);
        assert_eq!(get_hci_port(0, 1), DEFAULT_HCI_PORT + 1);
    }

    #[test]
    fn test_get_discovery_directory_missing_env_records_error_code() {
        let (table, _guard) = crate::util::netsim_logger::isolated_error_stats();
        let actual = get_discovery_directory_with_env(|_key| Err(env::VarError::NotPresent));

        let mut expected = PathBuf::from("/tmp");
        expected.push(DISCOVERY.subdir);
        assert_eq!(actual, expected);
        assert_eq!(table.snapshot(), vec![(crate::err::Code::DiscoveryEnvMissing as u32, 1)]);
    }

    #[test]
    fn test_redirect_std_stream() {
        const DIR_ENV: &str = "NETSIM_REDIRECT_TEST_DIR";
        const STDOUT_MARKER: &str = "netsim_redirect_stdout_marker";
        const STDERR_MARKER: &str = "netsim_redirect_stderr_marker";

        if let Some(dir) = env::var_os(DIR_ENV) {
            redirect_std_stream_in(Path::new(&dir), "test_").expect("redirect_std_stream_in");
            writeln!(io::stdout(), "{STDOUT_MARKER}").expect("write stdout");
            writeln!(io::stderr(), "{STDERR_MARKER}").expect("write stderr");
            io::stdout().flush().expect("flush stdout");
            io::stderr().flush().expect("flush stderr");
            return;
        }

        let dir = tempfile::tempdir().expect("create temp dir");
        let output = std::process::Command::new(env::current_exe().expect("current_exe"))
            .args(["--exact", "util::os_utils::tests::test_redirect_std_stream", "--nocapture"])
            .stdin(std::process::Stdio::null())
            .env(DIR_ENV, dir.path())
            .output()
            .expect("run child test");
        let out = String::from_utf8_lossy(&output.stdout);
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "child failed: {out}\n{err}");
        assert!(!out.contains(STDOUT_MARKER), "stdout leaked: {out}");
        assert!(!err.contains(STDERR_MARKER), "stderr leaked: {err}");
        let stdout_log = std::fs::read_to_string(dir.path().join("netsim_test_stdout.log"))
            .expect("read stdout log");
        let stderr_log = std::fs::read_to_string(dir.path().join("netsim_test_stderr.log"))
            .expect("read stderr log");
        assert!(stdout_log.contains(STDOUT_MARKER), "missing stdout marker: {stdout_log}");
        assert!(stderr_log.contains(STDERR_MARKER), "missing stderr marker: {stderr_log}");
    }
}
