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

#![allow(clippy::empty_line_after_doc_comments)]

/// Version library.

const VERSION: &str = "1.0.30";

pub(crate) fn get_version() -> String {
    format!("{VERSION}{}", option_env!("NETSIM_VERSION_SUFFIX").unwrap_or_default())
}

pub(crate) fn strip_version_suffix(version: &str) -> &str {
    version.split_once('+').map_or(version, |(v, _)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_version_suffix() {
        assert_eq!(strip_version_suffix("1.0.26+15953120"), "1.0.26");
        assert_eq!(strip_version_suffix("1.0.26-dev+34fa5eff"), "1.0.26-dev");
    }
}
