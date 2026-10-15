#!/bin/bash
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
# Simplified script to download Android Emulator & System Image artifacts using test_seq.
#
# Configuration & Environment Variables:
#   IMAGE_BUILD_ID      System image Build ID (default: 15956065)
#   EMULATOR_BUILD_ID   Emulator binary Build ID (default: 15953120)
#   IMAGE_TARGET        Build target for system image (default: sdk_gphone64_arm64-user)
#   EMULATOR_TARGET     Build target for emulator (default: emulator-mac_aarch64_gfxstream)
#   ANDROID_SDK_ROOT    Path to local Android SDK (default: ~/Library/Android/sdk)
#   TEST_SEQ_HOME       Location of test_seq harness (default: ~/test_seq)

set -e

# Default configurations
DEFAULT_IMAGE_BUILD_ID="15956065"
DEFAULT_EMULATOR_BUILD_ID="15953120"
DEFAULT_IMAGE_TARGET="sdk_gphone64_arm64-user"
DEFAULT_EMULATOR_TARGET="emulator-mac_aarch64_gfxstream"

IMAGE_BUILD_ID="${IMAGE_BUILD_ID:-$DEFAULT_IMAGE_BUILD_ID}"
EMULATOR_BUILD_ID="${EMULATOR_BUILD_ID:-$DEFAULT_EMULATOR_BUILD_ID}"
IMAGE_TARGET="${IMAGE_TARGET:-$DEFAULT_IMAGE_TARGET}"
EMULATOR_TARGET="${EMULATOR_TARGET:-$DEFAULT_EMULATOR_TARGET}"

SDK_DIR="${ANDROID_SDK_ROOT:-${HOME}/Library/Android/sdk}"
TEST_SEQ_HOME="${TEST_SEQ_HOME:-${HOME}/test_seq}"

# Auto-detect existing test_seq release directory
TEST_SEQ_RC="${TEST_SEQ_RC:-$(ls -1 "$TEST_SEQ_HOME" 2>/dev/null | grep -E "^test_seq_" | tail -n 1 || true)}"
TEST_SEQ_RC="${TEST_SEQ_RC:-test_seq_20260316_1239_RC00}"
TEST_SEQ_DIR="$TEST_SEQ_HOME/$TEST_SEQ_RC"

usage() {
    echo "Usage: $0 [options]"
    echo ""
    echo "Options:"
    echo "  -i, --image-build-id <id>      System image build ID (default: $DEFAULT_IMAGE_BUILD_ID)"
    echo "  -e, --emulator-build-id <id>   Emulator build ID (default: $DEFAULT_EMULATOR_BUILD_ID)"
    echo "  -t, --image-target <target>    System image target (default: $DEFAULT_IMAGE_TARGET)"
    echo "  -sdk, --sdk-dir <path>         Target Android SDK directory (default: $SDK_DIR)"
    echo "  --no-install                   Only download artifacts, do not copy to SDK directory"
    echo "  -h, --help                     Show this help message"
    exit 0
}

INSTALL_TO_SDK=true

while [[ $# -gt 0 ]]; do
    case "$1" in
        -i|--image-build-id)
            IMAGE_BUILD_ID="$2"
            shift 2
            ;;
        -e|--emulator-build-id)
            EMULATOR_BUILD_ID="$2"
            shift 2
            ;;
        -t|--image-target)
            IMAGE_TARGET="$2"
            shift 2
            ;;
        -sdk|--sdk-dir)
            SDK_DIR="$2"
            shift 2
            ;;
        --no-install)
            INSTALL_TO_SDK=false
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "Unknown option: $1"
            usage
            ;;
    esac
done

echo "=========================================================="
echo " Android Emulator & System Image Downloader"
echo "=========================================================="
echo " Emulator Build ID: $EMULATOR_BUILD_ID ($EMULATOR_TARGET)"
echo " System Image ID:   $IMAGE_BUILD_ID ($IMAGE_TARGET)"
echo " SDK Directory:     $SDK_DIR"
echo " test_seq Path:     $TEST_SEQ_DIR"
echo " Install to SDK:    $INSTALL_TO_SDK"
echo "=========================================================="

# 1. Ensure test_seq directory exists
if [ ! -d "$TEST_SEQ_DIR" ]; then
    echo "Downloading test_seq release binary..."
    if ! command -v gcloud >/dev/null 2>&1; then
        echo "ERROR: gcloud CLI is required to fetch test_seq releases."
        exit 1
    fi
    mkdir -p "$TEST_SEQ_HOME/tmp"
    cd "$TEST_SEQ_HOME/tmp"
    gcloud storage cp "gs://test-seq-releases/$TEST_SEQ_RC/test_seq_darwin_arm64.zip" "$TEST_SEQ_RC.zip"
    unzip -q "$TEST_SEQ_RC.zip"
    mv test_seq "../$TEST_SEQ_RC"
    rm "$TEST_SEQ_RC.zip"
    cd - >/dev/null
    rmdir "$TEST_SEQ_HOME/tmp" || true
fi

# 2. Generate download configuration textpb
CONFIG_PB="$TEST_SEQ_DIR/download_sequence.txtpb"

cat > "$CONFIG_PB" << EOF
agent: {
  id: "goldfish_fetch"
  ab_fetch_and_extract: {
    build_id: "${EMULATOR_BUILD_ID}"
    build_target: "${EMULATOR_TARGET}"
    resource: "sdk-repo-darwin_aarch64-emulator-BUILD_ID.zip"
  }
}
agent: {
  id: "image_fetch"
  ab_fetch_and_extract: {
    build_id: "${IMAGE_BUILD_ID}"
    build_target: "${IMAGE_TARGET}"
    resource: "sdk-repo-linux-system-images-BUILD_ID.zip"
  }
}
EOF

echo "Generated config at: $CONFIG_PB"

# 3. Execute test_seq fetcher
cd "$TEST_SEQ_DIR"
./test_seq download_sequence.txtpb

echo "=========================================================="
echo " Download Complete!"
echo "=========================================================="

# 4. Optionally install artifacts to local Android SDK directory
if [ "$INSTALL_TO_SDK" = true ]; then
    EMU_EXTRACT_DIR="$TEST_SEQ_DIR/runtime/goldfish_fetch/extract_dir"
    IMG_EXTRACT_DIR="$TEST_SEQ_DIR/runtime/image_fetch/extract_dir"

    if [ -d "$EMU_EXTRACT_DIR/emulator" ]; then
        echo "Installing updated emulator binaries to $SDK_DIR/emulator..."
        mkdir -p "$SDK_DIR/emulator"
        cp -R "$EMU_EXTRACT_DIR/emulator/"* "$SDK_DIR/emulator/"
    fi

    if [ -d "$IMG_EXTRACT_DIR" ]; then
        TOT_SYS_IMG_DIR="$SDK_DIR/system-images/android-ToT/google_apis_playstore/arm64-v8a"
        echo "Installing updated system image to $TOT_SYS_IMG_DIR..."
        mkdir -p "$TOT_SYS_IMG_DIR"
        
        # Locate extracted system image contents (handles nested directories)
        SYS_SRC="$(find "$IMG_EXTRACT_DIR" -name "system.img" -exec dirname {} \; | head -n 1)"
        if [ -n "$SYS_SRC" ]; then
            cp -R "$SYS_SRC/"* "$TOT_SYS_IMG_DIR/"
            echo "System image installed successfully to $TOT_SYS_IMG_DIR."
        else
            echo "WARNING: system.img not found in extracted files ($IMG_EXTRACT_DIR)."
        fi
    fi
    echo "=========================================================="
    echo " Installation to SDK Complete!"
    echo "=========================================================="
fi
