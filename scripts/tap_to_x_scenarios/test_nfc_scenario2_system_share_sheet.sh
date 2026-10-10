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
# Tap-to-X Scenario 2: System Share Sheet Tap-To-Share Verification

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
echo " Starting NFC Tap-to-Share (Scenario 2) Verification"
echo " Sender:   ${SENDER}"
echo " Receiver: ${RECEIVER}"
echo " Specification: Native System Share Sheet (ACTION_SEND) Tap-to-Share"
echo "=========================================================="

# 1. Clear logcats and prepare Receiver on high-power receive surface
echo "[1/6] Clearing logcats and preparing receiver..."
"${ADB}" -s "${SENDER}" logcat -c || true
"${ADB}" -s "${RECEIVER}" logcat -c || true

"${ADB}" -s "${SENDER}" shell input keyevent KEYCODE_WAKEUP || true
"${ADB}" -s "${SENDER}" shell wm dismiss-keyguard || true
"${ADB}" -s "${SENDER}" shell input keyevent KEYCODE_HOME || true
"${ADB}" -s "${RECEIVER}" shell input keyevent KEYCODE_WAKEUP || true
"${ADB}" -s "${RECEIVER}" shell wm dismiss-keyguard || true
"${ADB}" -s "${RECEIVER}" shell "am start -a com.google.android.gms.RECEIVE_NEARBY -n com.google.android.gms/.nearby.sharing.main.MainActivity" || true
sleep 2

# 2. Reset devices to 1.0m (out of NFC range) and wait for FastInitiation warming to end
echo "[2/6] Moving devices apart to 1.0m and waiting for FastInitiation warmup..."
DEV_SENDER=$("${NETSIM_BIN}" devices 2>/dev/null | grep -E "^[[:space:]]*(P10|Pixel 10|Pixel_10)[[:space:]]*$" | head -n 1 | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' || true)
DEV_RECEIVER=$("${NETSIM_BIN}" devices 2>/dev/null | grep -E "(P10_2|Pixel 10 \(2\)|Pixel_10_2)" | head -n 1 | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' || true)
DEV_SENDER="${DEV_SENDER:-Pixel 10}"
DEV_RECEIVER="${DEV_RECEIVER:-Pixel 10 (2)}"
echo "Identified Netsim Devices: Sender=${DEV_SENDER}, Receiver=${DEV_RECEIVER}"
"${NETSIM_BIN}" move "${DEV_SENDER}" 1.0 0.0 0.0
"${NETSIM_BIN}" move "${DEV_RECEIVER}" 0.0 0.0 0.0
sleep 4

# 3. Launch System Share Sheet on Sender (activates GestureExchange Reader Mode)
echo "[3/6] Launching System Share Sheet on Sender..."
"${ADB}" -s "${SENDER}" shell "am start -a android.intent.action.SEND -t 'text/plain' --es android.intent.extra.TEXT 'GestureExchange Tap-to-Share Verification'"

echo "Waiting for GMS TapToShare Reader Mode to register (flags: 385)..."
for i in {1..10}; do
    if "${ADB}" -s "${SENDER}" logcat -d -s NfcService | grep -q "flags: 385"; then
        echo "GMS Reader Mode registered successfully!"
        break
    fi
    sleep 0.5
done
"${ADB}" -s "${SENDER}" shell "am broadcast -a com.google.android.gesture_exchange.START_GESTURE_INITIATOR" 2>/dev/null || true
sleep 1

# 4. Trigger Physical NFC Tap Proximity (0.02m) & Motion Pulse
echo "[4/6] Moving ${DEV_SENDER} to 0.02m (Physical Tap Contact) & injecting acceleration pulse..."
"${NETSIM_BIN}" move "${DEV_SENDER}" 0.02 0.0 0.0
"${ADB}" -s "${SENDER}" emu sensor set acceleration 0 25.0 5.0 2>/dev/null || true
"${ADB}" -s "${RECEIVER}" emu sensor set acceleration 0 25.0 5.0 2>/dev/null || true
sleep 0.2
"${ADB}" -s "${SENDER}" emu sensor set acceleration 0 9.8 0.8 2>/dev/null || true
"${ADB}" -s "${RECEIVER}" emu sensor set acceleration 0 9.8 0.8 2>/dev/null || true

# 5. Allow Autonomous NFC Carrier Discovery & Handshake to complete
echo "[5/6] Waiting for autonomous NFC polling and observe mode handover..."
for i in {1..8}; do
    echo "  Holding proximity (contact time: ${i}s)..."
    sleep 1
done

# 6. Auto-accept transfer on Receiver UI if prompt appears
echo "[6/6] Handling incoming transfer acceptance on receiver..."
for i in {1..10}; do
    RECV_FOCUS=$("${ADB}" -s "${RECEIVER}" shell dumpsys window | grep -E "mCurrentFocus" || true)
    if echo "${RECV_FOCUS}" | grep -q "nearby.sharing.main.MainActivity"; then
        echo "Receiver showing Quick Share incoming transfer surface. Tapping Accept..."
        "${ADB}" -s "${RECEIVER}" shell input tap 715 1626 2>/dev/null || true
        break
    fi
    sleep 1
done

echo "=========================================================="
echo " GestureExchange Logcat Analysis (Sender):"
echo "=========================================================="
"${ADB}" -s "${SENDER}" logcat -d -s GestureExchange NfcService HostEmulationManager NearbySharing | tail -n 30 || true

echo ""
echo "=========================================================="
echo " GestureExchange Logcat Analysis (Receiver):"
echo "=========================================================="
"${ADB}" -s "${RECEIVER}" logcat -d -s GestureExchange NfcService HostEmulationManager NearbySharing | tail -n 30 || true

echo ""
echo "=========================================================="
echo " APDU Transceive Traces:"
echo "=========================================================="
echo "--- Sender APDUs ---"
"${ADB}" -s "${SENDER}" logcat -d | grep -iE "transceive|SELECT_PRIMARY_AID|A00000047609" | tail -n 15 || true
echo "--- Receiver APDUs ---"
"${ADB}" -s "${RECEIVER}" logcat -d | grep -iE "HostApdu|A00000047609|handleSelectAid" | tail -n 15 || true

echo ""
echo "=========================================================="
echo " Final Radio Statistics:"
echo "=========================================================="
"${NETSIM_BIN}" devices

# Strict assertion: Verify that APDU transceives / AID selection occurred on both devices
SENDER_APDU_COUNT=$("${ADB}" -s "${SENDER}" logcat -d | grep -iE "transceive|SELECT_PRIMARY_AID|A00000047609|F00000FE2C" | wc -l)
RECEIVER_APDU_COUNT=$("${ADB}" -s "${RECEIVER}" logcat -d | grep -iE "HostApdu|A00000047609|handleSelectAid|Gesture exchange|alternative AID|F00000FE2C" | wc -l)

echo "=========================================================="
if [ "${SENDER_APDU_COUNT}" -gt 0 ] && [ "${RECEIVER_APDU_COUNT}" -gt 0 ]; then
    echo " SUCCESS: NFC Tap-to-Share APDU exchange verified (${SENDER_APDU_COUNT} sender / ${RECEIVER_APDU_COUNT} receiver events)"
    echo "=========================================================="
else
    echo " ERROR: NFC Tap-to-Share failed: APDU exchange not detected on both devices (Sender: ${SENDER_APDU_COUNT}, Receiver: ${RECEIVER_APDU_COUNT})"
    echo "=========================================================="
    exit 1
fi

