#!/bin/bash
# Copyright 2025 The Android Open Source Project
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

set -euo pipefail
# A script to build the documentation for the daemon workspace,
# including generated dependency graphs.

# The script is in tools/netsim/next, which is the workspace root.
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &> /dev/null && pwd)

# All commands should be run from the script directory.
cd "${SCRIPT_DIR}"

# Default values
UPLOAD_DOCS=false
DEST_DIR="/google/data/rw/teams/betosim"

# Function to display usage information
usage() {
  echo "Usage: $(basename "$0") [--upload]"
  echo ""
  echo "Builds the Cargo documentation for the daemon crate and optionally uploads it to x20 Betosim team folder."
  echo ""
  echo "Arguments:"
  echo "  --upload                Uploads the generated documentation to the default destination directory ('${DEST_DIR}')."
  echo ""
  echo "Example:"
  echo "  $(basename "$0")                                  # Builds docs, skips upload."
  echo "  $(basename "$0") --upload                         # Builds docs, uploads to default location."
  exit 1
}

# Parse arguments
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --upload)
      UPLOAD_DOCS=true
      ;;
    -h|--help)
      usage
      ;;
    *)
      echo "Unknown argument: $1"
      usage
      ;;
  esac
  shift
done

# Check if cargo-modules is installed, and install it if not.
if ! cargo modules --version > /dev/null 2>&1; then
  echo "cargo-modules not found. Installing..."
  cargo install cargo-modules
fi

cargo clean --doc

echo "Building cargo doc for all workspace members..."
cargo doc --no-deps

# Define the output directory for daemon docs to avoid repetition.
DOC_DIR="target/doc/daemon"

echo "Generating module graph for daemon..."
#cargo modules dependencies \
#    --no-externs \
#    --no-sysroot \
#    --no-traits \
#    --no-types \
#    --no-uses \
#    --package daemon \
#    --lib | dot -Tpng > "${DOC_DIR}/modules.png"

echo "Documentation build complete."
echo "You can view the main page at: file://${SCRIPT_DIR}/${DOC_DIR}/index.html"

# Upload the file to x20 Betosim folders if --upload flag is set.
if [ "$UPLOAD_DOCS" = true ]; then
  echo "Uploading documentation to ${DEST_DIR}..."
  mkdir -p "${DEST_DIR}"
  fileutil --parallelism 100 cp -R target/doc "${DEST_DIR}"
  echo "Upload complete."
else
  echo "Skipped documentation upload. Use --upload flag to enable."
fi
