// Copyright 2023 The Android Open Source Project
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

//! Inspection and manipulation of the system environment.

use std::{env, path::PathBuf};

/// Get or create the netsimd temporary directory.
///
/// This is based on emu System.cpp android::base::getTempDir()
///
/// Under Forge temp directory is `$ANDROID_TMP/android-$USER/netsimd`,
/// otherwise it is `$TMP/android-$USER/netsimd`
pub fn netsimd_temp_dir() -> PathBuf {
    let path = netsimd_temp_dir_pathbuf();
    if !path.is_dir() {
        std::fs::create_dir_all(&path).expect("Failed to create temporary netsimd directory");
    }
    path
}

/// Helper function for netsimd_temp_dir() to allow Read Only
/// Unit tests.
fn netsimd_temp_dir_pathbuf() -> PathBuf {
    netsimd_temp_dir_pathbuf_with_env(|k| env::var(k), env::temp_dir)
}

fn netsimd_temp_dir_pathbuf_with_env<F, G>(get_env: F, get_temp_dir: G) -> PathBuf
where
    F: Fn(&str) -> Result<String, env::VarError>,
    G: Fn() -> PathBuf,
{
    // allow Forge to override the system temp
    let mut path = match get_env("ANDROID_TMP") {
        Ok(var) => PathBuf::from(var),
        _ => get_temp_dir(),
    };
    // On Windows the GetTempPath() is user-dependent so we don't need
    // to append $USER to the result -- otherwise allow multiple users
    // to co-exist on a system.
    #[cfg(not(target_os = "windows"))]
    {
        let user = match get_env("USER") {
            Ok(var) => format!("android-{var}"),
            _ => "android".to_string(),
        };
        path.push(user);
    };
    // netsimd files are stored in their own directory
    path.push("netsimd");
    path
}

#[cfg(not(target_os = "windows"))]
#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf};

    use super::netsimd_temp_dir_pathbuf_with_env;

    #[test]
    fn test_forge() {
        let mock_env = |key: &str| match key {
            "ANDROID_TMP" => Ok("/tmp/forge".to_string()),
            "USER" => Ok("ryle".to_string()),
            _ => Err(env::VarError::NotPresent),
        };
        let mock_temp = || PathBuf::from("/tmp");
        let tmp_dir = netsimd_temp_dir_pathbuf_with_env(mock_env, mock_temp);
        assert_eq!(tmp_dir.to_str().unwrap(), "/tmp/forge/android-ryle/netsimd");
    }

    #[test]
    fn test_non_forge() {
        let mock_env = |key: &str| match key {
            "USER" => Ok("ryle".to_string()),
            _ => Err(env::VarError::NotPresent),
        };
        let mock_temp = || PathBuf::from("/tmp");
        let netsimd_temp_dir = netsimd_temp_dir_pathbuf_with_env(mock_env, mock_temp);
        assert_eq!(netsimd_temp_dir.to_str().unwrap(), "/tmp/android-ryle/netsimd");
    }
}
