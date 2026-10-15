#!/usr/bin/env bash
# Copyright 2026 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# Bazel credential helper used to fetch the GCS-hosted hermetic SDK archives
# selected by --config=hermetic.
#
# This is a thin launcher around build/bazel/utils/cred_helper.py. Two reasons
# it exists rather than pointing --credential_helper straight at the shared
# build/bazel/utils/credential_helper.sh:
#
#   1. Bazel rejects '..' in a --credential_helper path, and the shared script
#      is outside this workspace.
#   2. The shared script resolves the interpreter and cred_helper.py relative to
#      the working directory, assuming that is the Android tree root. Here the
#      Bazel workspace root is tools/netsim, so those relative paths would miss.
#
# Everything below is resolved from this script's own location instead.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# tools/netsim/scripts -> tools/netsim -> tools -> tree root
root="$(cd "${here}/../../.." && pwd)"

case "$(uname -s)" in
  Linux*) python="${root}/prebuilts/python/linux-x86/bin/python3" ;;
  Darwin*) python="${root}/prebuilts/python/darwin-x86/bin/python3" ;;
  *)
    echo "credential_helper: unsupported OS: $(uname -s)" 1>&2
    exit 1
    ;;
esac

exec "${python}" "${root}/build/bazel/utils/cred_helper.py" "$@"
