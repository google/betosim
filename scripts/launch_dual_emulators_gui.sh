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
# Launch Netsim Daemon & Dual Emulators with GUI, Host DNS & Nfc Features

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ "$(uname)" == "Darwin" ]]; then
    DEFAULT_SDK="${HOME}/Library/Android/sdk"
    GPU_OPTS=()
else
    DEFAULT_SDK="${HOME}/Android/Sdk"
    GPU_OPTS=("-gpu" "swiftshader_indirect")
    export DISPLAY="${DISPLAY:-:1}"
fi
SDK_DIR="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-${DEFAULT_SDK}}}"
export LD_LIBRARY_PATH="${SDK_DIR}/emulator/lib64${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
EMULATOR_BIN="${SDK_DIR}/emulator/emulator"
NETSIM_BIN="${SDK_DIR}/emulator/netsim"
NETSIMD_BIN="${SDK_DIR}/emulator/netsimd"
ADB_BIN="${SDK_DIR}/platform-tools/adb"

echo "=========================================================="
echo " Starting Netsim Daemon & Dual Pixel 10 Emulators (GUI Mode)"
echo " Features: Nfc"
echo " Display:  ${DISPLAY:-none}"
echo "=========================================================="

# Helper function for resilient daemonization
run_daemon() {
    local log_file="$1"
    shift
    nohup "$@" > "${log_file}" 2>&1 &
}

# 1. Clean up existing processes & stale AVD locks
echo "[1/4] Terminating existing emulator and netsimd instances..."
pkill -9 -f "qemu-system-x86_64" 2>/dev/null || true
pkill -9 -f "qemu-system-aarch64" 2>/dev/null || true
pkill -9 -f "netsimd" 2>/dev/null || true
rm -f ~/.android/avd/Pixel_10*.avd/*.lock 2>/dev/null || true
sleep 2

# 2. Start Netsim Daemon with Host DNS routing
echo "[2/4] Starting Netsim Daemon (${NETSIMD_BIN}) with --host-dns 8.8.8.8,8.8.4.4..."
run_daemon /tmp/netsimd_runtime.log "${NETSIMD_BIN}" --host-dns 8.8.8.8,8.8.4.4
sleep 2

# 3. Launch Dual Emulators WITH GUI & Feature Flags (-feature Nfc)
echo "[3/4] Launching Pixel_10 (5554) and Pixel_10_2 (5556) with GUI & Features..."
run_daemon /tmp/emu_5554.log "${EMULATOR_BIN}" -avd Pixel_10 -port 5554 -no-snapshot-load -dns-server 8.8.8.8,8.8.4.4 -feature Nfc ${GPU_OPTS[@]+"${GPU_OPTS[@]}"}
run_daemon /tmp/emu_5556.log "${EMULATOR_BIN}" -avd Pixel_10_2 -port 5556 -no-snapshot-load -dns-server 8.8.8.8,8.8.4.4 -feature Nfc ${GPU_OPTS[@]+"${GPU_OPTS[@]}"}
disown -a 2>/dev/null || true

# 4. Wait for boot completion
echo "[4/4] Waiting for boot completion..."
START_TIME=$(date +%s)

while true; do
    CURRENT_TIME=$(date +%s)
    ELAPSED=$((CURRENT_TIME - START_TIME))

    if [ "$ELAPSED" -ge 240 ]; then
        echo "ERROR: Timed out waiting for emulators to boot after 240s!"
        exit 1
    fi

    B1=$("${ADB_BIN}" -s emulator-5554 shell getprop sys.boot_completed 2>/dev/null || echo "0")
    B2=$("${ADB_BIN}" -s emulator-5556 shell getprop sys.boot_completed 2>/dev/null || echo "0")

    if [[ "$B1" == "1" && "$B2" == "1" ]]; then
        echo "Both emulators booted successfully in ${ELAPSED}s!"
        break
    fi

    sleep 2
done

# Run Mobile Utilities & Flags setup
"${SCRIPT_DIR}/setup_mobile_utilities_flags.sh" emulator-5554 emulator-5556

echo "=========================================================="
echo " Netsim Topology:"
echo "=========================================================="
"${NETSIM_BIN}" devices

echo "=========================================================="
echo " GUI Emulators Ready with NFC Enabled!"
echo "=========================================================="
