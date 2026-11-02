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

use std::{
    collections::{HashMap, hash_map::Entry},
    env,
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    time::Duration,
};

use tracing::{error, warn};

use super::os_utils::get_discovery_directory_with_env;

/// Maximum time spent probing a resolved socket address from `netsim.ini`.
pub const SERVER_PROBE_TIMEOUT: Duration = Duration::from_millis(100);

/// Checks whether `addr` accepts a TCP connection in the current network
/// namespace within [`SERVER_PROBE_TIMEOUT`].
pub fn is_server_reachable(addr: &str) -> bool {
    let Ok(sock_addrs) = addr.to_socket_addrs() else {
        return false;
    };
    sock_addrs
        .into_iter()
        .any(|sock_addr| TcpStream::connect_timeout(&sock_addr, SERVER_PROBE_TIMEOUT).is_ok())
}

#[derive(Default)]
pub struct IniParserOptions {
    pub strict: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum IniParseErrorKind {
    #[error("Empty key")]
    EmptyKey,
    #[error("Missing `=`")]
    MissingDelimiter,
    #[error("Duplicate key `{key}` with conflicting values: `{existing_value}` vs `{new_value}`")]
    DuplicateKey { key: String, existing_value: String, new_value: String },
}

#[derive(Debug, thiserror::Error)]
#[error("{line_number}:{column_number} {kind} ({line_content})")]
pub struct IniParseError {
    pub line_number: usize,
    pub column_number: usize,
    pub kind: IniParseErrorKind,
    pub line_content: String,
}

pub fn parse_ini(
    content: &str,
    options: &IniParserOptions,
) -> Result<HashMap<String, String>, IniParseError> {
    let mut map: HashMap<String, String> = HashMap::new();
    for (line_idx, line) in content.lines().enumerate() {
        let line_number = line_idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            if options.strict && key.is_empty() {
                let column_number = line.find('=').unwrap_or(0) + 1;
                return Err(IniParseError {
                    line_number,
                    column_number,
                    kind: IniParseErrorKind::EmptyKey,
                    line_content: line.to_string(),
                });
            }

            let entry = map.entry(key.to_string());
            if options.strict
                && let Entry::Occupied(ref occupied) = entry
            {
                let existing_value = occupied.get();
                if existing_value != value {
                    let column_number = line.find(key).unwrap_or(0) + 1;
                    return Err(IniParseError {
                        line_number,
                        column_number,
                        kind: IniParseErrorKind::DuplicateKey {
                            key: key.to_string(),
                            existing_value: existing_value.to_string(),
                            new_value: value.to_string(),
                        },
                        line_content: line.to_string(),
                    });
                }
            }
            entry.insert_entry(value.to_string());
        } else if options.strict {
            let column_number = line.find(trimmed).unwrap_or(0) + 1;
            return Err(IniParseError {
                line_number,
                column_number,
                kind: IniParseErrorKind::MissingDelimiter,
                line_content: line.to_string(),
            });
        }
    }
    Ok(map)
}

/// Get the filename of netsim.ini for a given instance number.
pub fn get_ini_filename(instance_num: u16) -> String {
    if instance_num == 1 { "netsim.ini".to_string() } else { format!("netsim_{instance_num}.ini") }
}

/// Get the filepath of netsim.ini under discovery directory
pub fn get_ini_filepath(instance_num: u16) -> PathBuf {
    get_ini_filepath_with_env(|k| env::var(k), instance_num)
}

/// Get the grpc server address for netsim
fn get_address_by_key_with_env<F>(get_env: F, instance_num: u16, key: &str) -> Option<String>
where
    F: Fn(&str) -> Result<String, env::VarError>,
{
    let filepath = get_ini_filepath_with_env(get_env, instance_num);
    if !filepath.exists() {
        error!("Unable to find netsim ini file: {filepath:?}");
        return None;
    }
    if !filepath.is_file() {
        error!("Not a file: {filepath:?}");
        return None;
    }
    let content = match std::fs::read_to_string(&filepath) {
        Ok(c) => c,
        Err(err) => {
            error!(error = %err, "Error reading ini file");
            return None;
        }
    };
    let map = match parse_ini(&content, &IniParserOptions::default()) {
        Ok(m) => m,
        Err(err) => {
            error!(error = %err, "Error parsing ini file");
            return None;
        }
    };
    let addr = map
        .get(key)
        .map(|s| if s.contains(':') { s.to_string() } else { format!("localhost:{s}") })?;
    if !is_server_reachable(&addr) {
        warn!(
            key,
            %addr,
            ?filepath,
            "Server address in netsim ini file is unreachable in current network namespace"
        );
        return None;
    }
    Some(addr)
}

fn get_address_by_key(instance_num: u16, key: &str) -> Option<String> {
    get_address_by_key_with_env(|k| env::var(k), instance_num, key)
}

/// Get the grpc server address for netsim
pub fn get_server_address(instance_num: u16) -> Option<String> {
    get_address_by_key(instance_num, "grpc.port")
}

/// Get the TCP server address for netsim packet stream
pub fn get_tcp_server_address(instance_num: u16) -> Option<String> {
    get_address_by_key(instance_num, "tcp.port")
}

/// Get the WebSocket server address for netsim packet stream
pub fn get_ws_server_address(instance_num: u16) -> Option<String> {
    get_address_by_key(instance_num, "ws.port")
}

/// Get the HTTP server address for netsim REST API
pub fn get_http_server_address(instance_num: u16) -> Option<String> {
    get_address_by_key(instance_num, "http.port")
}

pub(crate) fn get_ini_filepath_with_env<F>(get_env: F, instance_num: u16) -> PathBuf
where
    F: Fn(&str) -> Result<String, env::VarError>,
{
    let mut discovery_dir = get_discovery_directory_with_env(get_env);
    discovery_dir.push(get_ini_filename(instance_num));
    discovery_dir
}

#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf};

    use super::{IniParseErrorKind, IniParserOptions, get_ini_filepath_with_env, parse_ini};

    #[test]
    fn test_read() {
        for test_case in ["port=123", "port= 123", "port =123", " port = 123 "] {
            let ini_map = parse_ini(test_case, &Default::default()).unwrap();
            assert_eq!(ini_map.get("port").unwrap(), "123");
        }
    }

    #[test]
    fn test_read_no_newline() {
        let ini_map = parse_ini("port=123", &Default::default()).unwrap();
        assert_eq!(ini_map.get("port").unwrap(), "123");
    }

    #[test]
    fn test_read_multiple_lines() {
        let ini_map = parse_ini("port=123\nport2=456\n", &Default::default()).unwrap();
        assert_eq!(ini_map.get("port").unwrap(), "123");
        assert_eq!(ini_map.get("port2").unwrap(), "456");
    }

    #[test]
    fn test_get_ini_filepath() {
        let mock_env = |key: &str| {
            if key == "TMPDIR" { Ok("/tmpdir".to_string()) } else { Err(env::VarError::NotPresent) }
        };

        // Test get_netsim_ini_filepath
        assert_eq!(get_ini_filepath_with_env(mock_env, 1), PathBuf::from("/tmpdir/netsim.ini"));
        assert_eq!(get_ini_filepath_with_env(mock_env, 2), PathBuf::from("/tmpdir/netsim_2.ini"));
    }
    #[test]
    fn test_parse_ini_strict() {
        // Test empty key
        let content = "=value";
        let result = parse_ini(content, &IniParserOptions { strict: true });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.line_number, 1);
        assert_eq!(err.column_number, 1);
        assert!(matches!(err.kind, IniParseErrorKind::EmptyKey));
        assert_eq!(err.line_content, "=value");

        // Test missing delimiter
        let content = "key_without_equals";
        let result = parse_ini(content, &IniParserOptions { strict: true });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.line_number, 1);
        assert_eq!(err.column_number, 1);
        assert!(matches!(err.kind, IniParseErrorKind::MissingDelimiter));
        assert_eq!(err.line_content, "key_without_equals");

        // Test duplicate key
        let content = "key=value1\nkey=value2";
        let result = parse_ini(content, &IniParserOptions { strict: true });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.line_number, 2);
        assert_eq!(err.column_number, 1);
        if let IniParseErrorKind::DuplicateKey { key, existing_value, new_value } = err.kind {
            assert_eq!(key, "key");
            assert_eq!(existing_value, "value1");
            assert_eq!(new_value, "value2");
        } else {
            panic!("Expected DuplicateKey error");
        }
        assert_eq!(err.line_content, "key=value2");

        // Test duplicate key with same value passes
        let content = "key=value1\nkey=value1";
        let result = parse_ini(content, &IniParserOptions { strict: true });
        assert!(result.is_ok());
        let map = result.unwrap();
        assert_eq!(map.get("key").unwrap(), "value1");
        assert_eq!(map.len(), 1);

        // Test non-strict mode ignores these
        let content = "=value\nkey_without_equals\nkey=value1\nkey=value2";
        let result = parse_ini(content, &IniParserOptions { strict: false });
        assert!(result.is_ok());
        let map = result.unwrap();
        assert_eq!(map.get("").unwrap(), "value");
        assert_eq!(map.get("key").unwrap(), "value2");
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_get_address_by_key() {
        let temp_dir = tempfile::tempdir().unwrap();
        let tmp_dir_path = temp_dir.path().to_path_buf();
        let tmp_dir_path_clone = tmp_dir_path.clone();

        let mock_env = move |key: &str| {
            if key == "TMPDIR" {
                Ok(tmp_dir_path_clone.to_str().unwrap().to_string())
            } else {
                Err(env::VarError::NotPresent)
            }
        };

        // Test case 1: Reachable loopback ports resolved as localhost:<port>
        let grpc_listener = std::net::TcpListener::bind("localhost:0").unwrap();
        let tcp_listener = std::net::TcpListener::bind("localhost:0").unwrap();
        let ws_listener = std::net::TcpListener::bind("localhost:0").unwrap();
        let grpc_port = grpc_listener.local_addr().unwrap().port();
        let tcp_port = tcp_listener.local_addr().unwrap().port();
        let ws_port = ws_listener.local_addr().unwrap().port();

        let ini_content =
            format!("grpc.port={grpc_port}\ntcp.port={tcp_port}\nws.port={ws_port}\n");
        std::fs::write(tmp_dir_path.join("netsim.ini"), ini_content).unwrap();

        let grpc_addr = super::get_address_by_key_with_env(&mock_env, 1, "grpc.port");
        assert_eq!(grpc_addr.unwrap(), format!("localhost:{grpc_port}"));

        let tcp_addr = super::get_address_by_key_with_env(&mock_env, 1, "tcp.port");
        assert_eq!(tcp_addr.unwrap(), format!("localhost:{tcp_port}"));

        let ws_addr = super::get_address_by_key_with_env(&mock_env, 1, "ws.port");
        assert_eq!(ws_addr.unwrap(), format!("localhost:{ws_port}"));

        // Test case 2: Port with explicit host:port value
        let explicit_grpc = std::net::TcpListener::bind("localhost:0").unwrap();
        let explicit_tcp = std::net::TcpListener::bind("localhost:0").unwrap();
        let explicit_grpc_addr = explicit_grpc.local_addr().unwrap().to_string();
        let explicit_tcp_addr = explicit_tcp.local_addr().unwrap().to_string();

        let ini_content = format!("grpc.port={explicit_grpc_addr}\ntcp.port={explicit_tcp_addr}\n");
        std::fs::write(tmp_dir_path.join("netsim.ini"), ini_content).unwrap();

        let grpc_addr = super::get_address_by_key_with_env(&mock_env, 1, "grpc.port");
        assert_eq!(grpc_addr.unwrap(), explicit_grpc_addr);

        let tcp_addr = super::get_address_by_key_with_env(&mock_env, 1, "tcp.port");
        assert_eq!(tcp_addr.unwrap(), explicit_tcp_addr);
    }

    #[test]
    fn test_get_address_by_key_rejects_unreachable_port() {
        let temp_dir = tempfile::tempdir().unwrap();
        let tmp_dir_path = temp_dir.path().to_path_buf();
        let tmp_dir_path_clone = tmp_dir_path.clone();

        let mock_env = move |key: &str| {
            if key == "TMPDIR" {
                Ok(tmp_dir_path_clone.to_str().unwrap().to_string())
            } else {
                Err(env::VarError::NotPresent)
            }
        };

        let closed_listener = std::net::TcpListener::bind("localhost:0").unwrap();
        let closed_port = closed_listener.local_addr().unwrap().port();
        drop(closed_listener);

        assert!(!super::is_server_reachable(&format!("localhost:{closed_port}")));
        assert!(!super::is_server_reachable("invalid:address:port"));

        let ini_content = format!(
            "grpc.port={closed_port}\ntcp.port={closed_port}\nws.port={closed_port}\nhttp.port={closed_port}\n"
        );
        std::fs::write(tmp_dir_path.join("netsim.ini"), ini_content).unwrap();

        assert!(super::get_address_by_key_with_env(&mock_env, 1, "grpc.port").is_none());
        assert!(super::get_address_by_key_with_env(&mock_env, 1, "tcp.port").is_none());
        assert!(super::get_address_by_key_with_env(&mock_env, 1, "ws.port").is_none());
        assert!(super::get_address_by_key_with_env(&mock_env, 1, "http.port").is_none());
    }
}
