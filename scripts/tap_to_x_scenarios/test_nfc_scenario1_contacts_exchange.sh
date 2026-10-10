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
# Tap-to-X Scenario 1: Contacts Exchange Background Reader Mode Verification

set -euo pipefail

if [[ "$(uname)" == "Darwin" ]]; then
    DEFAULT_SDK="${HOME}/Library/Android/sdk"
else
    DEFAULT_SDK="${HOME}/Android/Sdk"
fi
SDK_DIR="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-${DEFAULT_SDK}}}"
ADB="${SDK_DIR}/platform-tools/adb"
NETSIM_BIN="${SDK_DIR}/emulator/netsim"

export LD_LIBRARY_PATH="${SDK_DIR}/emulator/lib64${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

SENDER="${1:-emulator-5554}"
RECEIVER="${2:-emulator-5556}"

echo "=========================================================="
echo " Starting Tap-to-X Scenario 1: Contacts Exchange"
echo " Sender:   ${SENDER}"
echo " Receiver: ${RECEIVER}"
echo " Specification: Both devices on Home Screen (Background Reader Mode)"
echo "=========================================================="

# 1. Reset both devices to Home screen and unlock
echo "[1/5] Resetting both devices to Home screen & unlocking..."
"${ADB}" -s "${SENDER}" shell input keyevent KEYCODE_WAKEUP || true
"${ADB}" -s "${SENDER}" shell wm dismiss-keyguard || true
"${ADB}" -s "${SENDER}" shell input keyevent KEYCODE_HOME || true

"${ADB}" -s "${RECEIVER}" shell input keyevent KEYCODE_WAKEUP || true
"${ADB}" -s "${RECEIVER}" shell wm dismiss-keyguard || true
"${ADB}" -s "${RECEIVER}" shell input keyevent KEYCODE_HOME || true

# 2. Clear logcat buffers & configure verbose logging
echo "[2/5] Configuring logging, clearing logcats, and preparing receiver..."
"${ADB}" -s "${SENDER}" shell setprop log.tag.GestureExchange VERBOSE || true
"${ADB}" -s "${RECEIVER}" shell setprop log.tag.GestureExchange VERBOSE || true
"${ADB}" -s "${RECEIVER}" shell "am start -a com.google.android.gms.RECEIVE_NEARBY -n com.google.android.gms/.nearby.sharing.main.MainActivity" || true
sleep 1
"${ADB}" -s "${SENDER}" logcat -c
"${ADB}" -s "${RECEIVER}" logcat -c

# 3. Position devices out of NFC range (1.0m)
echo "[3/5] Moving devices apart to 1.0m..."
DEV_SENDER=$("${NETSIM_BIN}" devices 2>/dev/null | grep -E "^[[:space:]]*(P10|Pixel 10|Pixel_10)[[:space:]]*$" | head -n 1 | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' || true)
DEV_RECEIVER=$("${NETSIM_BIN}" devices 2>/dev/null | grep -E "(P10_2|Pixel 10 \(2\)|Pixel_10_2)" | head -n 1 | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' || true)
DEV_SENDER="${DEV_SENDER:-Pixel 10}"
DEV_RECEIVER="${DEV_RECEIVER:-Pixel 10 (2)}"
echo "Identified Netsim Devices: Sender=${DEV_SENDER}, Receiver=${DEV_RECEIVER}"
"${NETSIM_BIN}" move "${DEV_SENDER}" 1.0 0.0 0.0
"${NETSIM_BIN}" move "${DEV_RECEIVER}" 0.0 0.0 0.0
sleep 2

# 4. Launch Quick Share on Sender to activate Tap-to-Share / Contacts Exchange Reader Mode
echo "[4/5] Launching Quick Share on Sender..."
"${ADB}" -s "${SENDER}" shell "am start -a com.google.android.gms.SHARE_NEARBY -n com.google.android.gms/.nearby.sharing.main.MainActivity"

echo "Waiting for GMS Reader Mode to register (flags: 385)..."
for i in {1..10}; do
    if "${ADB}" -s "${SENDER}" logcat -d -s NfcService | grep -q "flags: 385"; then
        echo "GMS Reader Mode registered successfully!"
        break
    fi
    sleep 0.5
done
"${ADB}" -s "${SENDER}" shell am broadcast -a com.google.android.gesture_exchange.START_GESTURE_INITIATOR 2>/dev/null || true
sleep 1

# 5. Trigger Physical NFC Tap Proximity (0.02m) & Motion Sensor Tap
echo "[5/5] Moving ${DEV_SENDER} to 0.02m (Physical Tap Contact) & injecting acceleration pulse..."
"${NETSIM_BIN}" move "${DEV_SENDER}" 0.02 0.0 0.0

# Simulate physical bump / tap acceleration pulse on both devices
"${ADB}" -s "${SENDER}" emu sensor set acceleration 0 25.0 5.0 2>/dev/null || true
"${ADB}" -s "${RECEIVER}" emu sensor set acceleration 0 25.0 5.0 2>/dev/null || true
sleep 0.2
"${ADB}" -s "${SENDER}" emu sensor set acceleration 0 9.8 0.8 2>/dev/null || true
"${ADB}" -s "${RECEIVER}" emu sensor set acceleration 0 9.8 0.8 2>/dev/null || true

# Allow NFC carrier handshake and dynamic glowing animation
echo "Waiting for NFC handshake & glowing animation..."
for i in {1..6}; do
    echo "  Holding proximity (contact time: ${i}s)..."
    sleep 1
done

echo ""
echo "=========================================================="
echo " Scenario 1 Logcat Analysis (Sender):"
echo "=========================================================="
"${ADB}" -s "${SENDER}" logcat -d -s GestureExchange:V NearbySharing:V NfcService:D HostEmulationManager:V | tail -n 25 || true

echo ""
echo "=========================================================="
echo " Scenario 1 Logcat Analysis (Receiver):"
echo "=========================================================="
"${ADB}" -s "${RECEIVER}" logcat -d -s GestureExchange:V NearbySharing:V NfcService:D HostEmulationManager:V | tail -n 25 || true

echo ""
echo "=========================================================="
echo " Checking Foreground Activities on Both Devices:"
echo "=========================================================="
echo "--- Sender Foreground Activity ---"
"${ADB}" -s "${SENDER}" shell dumpsys window | grep -E "mCurrentFocus|mFocusedApp" || true
echo "--- Receiver Foreground Activity ---"
"${ADB}" -s "${RECEIVER}" shell dumpsys window | grep -E "mCurrentFocus|mFocusedApp" || true

echo ""
echo "=========================================================="
echo " Final Radio Statistics:"
echo "=========================================================="
"${NETSIM_BIN}" devices

echo "=========================================================="
echo " Scenario 1 Verification Evaluation:"
echo "=========================================================="
# Assert Contact Exchange or Quick Share activity
SENDER_EVENTS=$("${ADB}" -s "${SENDER}" logcat -d | grep -iE "transceive|SELECT_PRIMARY_AID|A00000047609|F00000FE2C" | wc -l)
RECEIVER_EVENTS=$("${ADB}" -s "${RECEIVER}" logcat -d | grep -iE "HostApdu|A00000047609|handleSelectAid|ContactExchange|F00000FE2C" | wc -l)

echo "=========================================================="
if [ "${SENDER_EVENTS}" -gt 0 ] && [ "${RECEIVER_EVENTS}" -gt 0 ]; then
    echo " [RESULT: PASS] Tap-to-X Scenario 1: Contacts Exchange NFC handshake verified (${SENDER_EVENTS} sender / ${RECEIVER_EVENTS} receiver events)"
    echo "=========================================================="
else
    echo " [RESULT: FAIL] Tap-to-X Scenario 1: NFC handshake failed (Sender events: ${SENDER_EVENTS}, Receiver events: ${RECEIVER_EVENTS})"
    echo "=========================================================="
    exit 1
fi
