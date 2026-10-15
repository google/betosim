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

//! # IniFile class

use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::io::prelude::*;
use std::path::PathBuf;

use log::error;

use super::os_utils::get_discovery_directory;

/// A simple class to process init file. Based on
/// external/qemu/android/android-emu-base/android/base/files/IniFile.h
struct IniFile {
    /// The data stored in the ini file.
    data: HashMap<String, String>,
    /// The path to the ini file.
    filepath: PathBuf,
}

impl IniFile {
    /// Creates a new IniFile with the given filepath.
    ///
    /// # Arguments
    ///
    /// * `filepath` - The path to the ini file.
    fn new(filepath: PathBuf) -> IniFile {
        IniFile { data: HashMap::new(), filepath }
    }

    /// Reads data into IniFile from the backing file, overwriting any
    /// existing data.
    ///
    /// # Returns
    ///
    /// `Ok` if the write was successful, `Error` otherwise.
    fn read(&mut self) -> Result<(), Box<dyn Error>> {
        self.data.clear();

        let mut f = File::open(self.filepath.clone())?;
        let reader = BufReader::new(&mut f);

        for line in reader.lines() {
            let line = line?;
            let parts = line.split_once('=');
            if parts.is_none() {
                continue;
            }
            let key = parts.unwrap().0.trim();
            let value = parts.unwrap().1.trim();
            self.data.insert(key.to_owned(), value.to_owned());
        }

        Ok(())
    }

    /// Writes the current IniFile to the backing file.
    ///
    /// # Returns
    ///
    /// `Ok` if the write was successful, `Error` otherwise.
    fn write(&self) -> std::io::Result<()> {
        let mut f = create_new(self.filepath.clone())?;
        for (key, value) in &self.data {
            writeln!(&mut f, "{key}={value}")?;
        }
        f.flush()?;
        Ok(())
    }

    /// Gets value.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to get the value for.
    ///
    /// # Returns
    ///
    /// An `Option` containing the value if it exists, `None` otherwise.
    fn get(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(|v| v.as_str())
    }

    /// Inserts a key-value pair.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to set the value for.
    /// * `value` - The value to set.
    fn insert(&mut self, key: &str, value: &str) {
        self.data.insert(key.to_owned(), value.to_owned());
    }
}

// TODO: Replace with std::fs::File::create_new once Rust toolchain is upgraded to 1.77
/// Create new file, errors if it already exists.
fn create_new<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<File> {
    std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(path.as_ref())
}

/// Write ports to ini file
pub fn create_ini(
    instance_num: u16,
    grpc_port: u32,
    web_port: Option<u16>,
    websocket_port: Option<u16>,
) -> std::io::Result<()> {
    // Instantiate IniFile
    let filepath = get_ini_filepath(instance_num);
    let mut ini_file = IniFile::new(filepath);

    // Write ports to ini file
    if let Some(num) = web_port {
        ini_file.insert("web.port", &num.to_string());
    }
    if let Some(num) = websocket_port {
        ini_file.insert("ws.port", &num.to_string())
    }
    ini_file.insert("grpc.port", &grpc_port.to_string());
    ini_file.write()
}

/// Remove netsim ini file
pub fn remove_ini(instance_num: u16) -> std::io::Result<()> {
    let filepath = get_ini_filepath(instance_num);
    std::fs::remove_file(filepath)
}

/// Get the filepath of netsim.ini under discovery directory
fn get_ini_filepath(instance_num: u16) -> PathBuf {
    let mut discovery_dir = get_discovery_directory();
    let filename = if instance_num == 1 {
        "netsim.ini".to_string()
    } else {
        format!("netsim_{instance_num}.ini")
    };
    discovery_dir.push(filename);
    discovery_dir
}

/// Get the grpc server address for netsim
pub fn get_server_address(instance_num: u16) -> Option<String> {
    let filepath = get_ini_filepath(instance_num);
    if !filepath.exists() {
        error!("Unable to find netsim ini file: {filepath:?}");
        return None;
    }
    if !filepath.is_file() {
        error!("Not a file: {filepath:?}");
        return None;
    }
    let mut ini_file = IniFile::new(filepath);
    if let Err(err) = ini_file.read() {
        error!("Error reading ini file: {err:?}");
    }
    ini_file
        .get("grpc.port")
        .map(|s: &str| if s.contains(':') { s.to_string() } else { format!("localhost:{s}") })
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::path::PathBuf;
    use std::{env, time::SystemTime};

    use super::IniFile;
    use super::get_ini_filepath;

    use crate::tests::ENV_MUTEX;

    impl IniFile {
        /// Checks if a certain key exists in the file.
        ///
        /// # Arguments
        ///
        /// * `key` - The key to check.
        ///
        /// # Returns
        ///
        /// `true` if the key exists, `false` otherwise.
        fn contains_key(&self, key: &str) -> bool {
            self.data.contains_key(key)
        }
    }

    fn get_temp_ini_filepath(prefix: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "{prefix}_{}.ini",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_string()
                + "_"
                + &rand::random::<u64>().to_string()
        ))
    }

    // NOTE: ctest run a test at least twice tests in parallel, so we need to use unique temp file
    // to prevent tests from accessing the same file simultaneously.
    #[test]
    fn test_read() {
        for test_case in ["port=123", "port= 123", "port =123", " port = 123 "] {
            let filepath = get_temp_ini_filepath("test_read");

            {
                let mut tmpfile = match File::create(&filepath) {
                    Ok(f) => f,
                    Err(_) => return,
                };
                writeln!(tmpfile, "{test_case}").unwrap();
            }

            let mut inifile = IniFile::new(filepath.clone());
            inifile.read().unwrap();

            assert!(!inifile.contains_key("unknown-key"));
            assert!(inifile.contains_key("port"), "Fail in test case: {test_case}");
            assert_eq!(inifile.get("port").unwrap(), "123");
            assert_eq!(inifile.get("unknown-key"), None);

            // Note that there is no guarantee that the file is immediately deleted (e.g.,
            // depending on platform, other open file descriptors may prevent immediate removal).
            // https://doc.rust-lang.org/std/fs/fn.remove_file.html.
            std::fs::remove_file(filepath).unwrap();
        }
    }

    #[test]
    fn test_read_no_newline() {
        let filepath = get_temp_ini_filepath("test_read_no_newline");

        {
            let mut tmpfile = match File::create(&filepath) {
                Ok(f) => f,
                Err(_) => return,
            };
            write!(tmpfile, "port=123").unwrap();
        }

        let mut inifile = IniFile::new(filepath.clone());
        inifile.read().unwrap();

        assert!(!inifile.contains_key("unknown-key"));
        assert!(inifile.contains_key("port"));
        assert_eq!(inifile.get("port").unwrap(), "123");
        assert_eq!(inifile.get("unknown-key"), None);

        std::fs::remove_file(filepath).unwrap();
    }

    #[test]
    fn test_read_no_file() {
        let filepath = get_temp_ini_filepath("test_read_no_file");
        let mut inifile = IniFile::new(filepath.clone());
        assert!(inifile.read().is_err());
    }

    #[test]
    fn test_read_multiple_lines() {
        let filepath = get_temp_ini_filepath("test_read_multiple_lines");

        {
            let mut tmpfile = match File::create(&filepath) {
                Ok(f) => f,
                Err(_) => return,
            };
            write!(tmpfile, "port=123\nport2=456\n").unwrap();
        }

        let mut inifile = IniFile::new(filepath.clone());
        inifile.read().unwrap();

        assert!(!inifile.contains_key("unknown-key"));
        assert!(inifile.contains_key("port"));
        assert!(inifile.contains_key("port2"));
        assert_eq!(inifile.get("port").unwrap(), "123");
        assert_eq!(inifile.get("port2").unwrap(), "456");
        assert_eq!(inifile.get("unknown-key"), None);

        std::fs::remove_file(filepath).unwrap();
    }

    #[test]
    fn test_insert_and_contains_key() {
        let filepath = get_temp_ini_filepath("test_insert_and_contains_key");

        let mut inifile = IniFile::new(filepath);

        assert!(!inifile.contains_key("port"));
        assert!(!inifile.contains_key("unknown-key"));

        inifile.insert("port", "123");

        assert!(inifile.contains_key("port"));
        assert!(!inifile.contains_key("unknown-key"));
        assert_eq!(inifile.get("port").unwrap(), "123");
        assert_eq!(inifile.get("unknown-key"), None);

        // Update the value of an existing key.
        inifile.insert("port", "234");

        assert!(inifile.contains_key("port"));
        assert!(!inifile.contains_key("unknown-key"));
        assert_eq!(inifile.get("port").unwrap(), "234");
        assert_eq!(inifile.get("unknown-key"), None);
    }

    #[test]
    fn test_write() {
        let filepath = get_temp_ini_filepath("test_write");

        let mut inifile = IniFile::new(filepath.clone());

        assert!(!inifile.contains_key("port"));
        assert!(!inifile.contains_key("unknown-key"));

        inifile.insert("port", "123");

        assert!(inifile.contains_key("port"));
        assert!(!inifile.contains_key("unknown-key"));
        assert_eq!(inifile.get("port").unwrap(), "123");
        assert_eq!(inifile.get("unknown-key"), None);

        if inifile.write().is_err() {
            return;
        }
        let mut file = File::open(&filepath).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();

        assert_eq!(contents, "port=123\n");

        std::fs::remove_file(filepath).unwrap();
    }

    #[test]
    fn test_write_and_read() {
        let filepath = get_temp_ini_filepath("test_write_and_read");

        {
            let mut inifile = IniFile::new(filepath.clone());

            assert!(!inifile.contains_key("port"));
            assert!(!inifile.contains_key("port2"));
            assert!(!inifile.contains_key("unknown-key"));

            inifile.insert("port", "123");
            inifile.insert("port2", "456");

            assert!(inifile.contains_key("port"));
            assert!(!inifile.contains_key("unknown-key"));
            assert_eq!(inifile.get("port").unwrap(), "123");
            assert_eq!(inifile.get("unknown-key"), None);

            if inifile.write().is_err() {
                return;
            }
        }

        let mut inifile = IniFile::new(filepath.clone());
        inifile.read().unwrap();

        assert!(!inifile.contains_key("unknown-key"));
        assert!(inifile.contains_key("port"));
        assert!(inifile.contains_key("port2"));
        assert_eq!(inifile.get("port").unwrap(), "123");
        assert_eq!(inifile.get("port2").unwrap(), "456");
        assert_eq!(inifile.get("unknown-key"), None);

        std::fs::remove_file(filepath).unwrap();
    }

    #[test]
    fn test_get_ini_filepath() {
        let _locked = ENV_MUTEX.lock();

        // Test with TMPDIR variable
        // SAFETY: Test code
        // Note: set_var and remove_var have always been unsound when called in
        // a multi-threaded context. They are marked unsafe in edition 2024.
        // See https://doc.rust-lang.org/edition-guide/rust-2024/newly-unsafe-functions.html#stdenvset_var-remove_var
        unsafe { std::env::set_var("TMPDIR", "/tmpdir") };

        // Test get_netsim_ini_filepath
        assert_eq!(get_ini_filepath(1), PathBuf::from("/tmpdir/netsim.ini"));
        assert_eq!(get_ini_filepath(2), PathBuf::from("/tmpdir/netsim_2.ini"));
    }
}
