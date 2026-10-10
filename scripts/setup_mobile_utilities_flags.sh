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
# Setup network and Mobile Utilities app on connected Android emulators.

set -euo pipefail

if [[ "$(uname)" == "Darwin" ]]; then
    DEFAULT_SDK="${HOME}/Library/Android/sdk"
else
    DEFAULT_SDK="${HOME}/Android/Sdk"
fi
SDK_DIR="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-${DEFAULT_SDK}}}"
ADB_BIN="${SDK_DIR}/platform-tools/adb"
MOBUTILS_APK="${HOME}/Downloads/mobileutilities_binary.apk"

if [ $# -eq 0 ]; then
    DEVICES=("emulator-5554" "emulator-5556")
else
    DEVICES=("$@")
fi

echo "=========================================================="
echo " Setting Up Network & Mobile Utilities App"
echo " Target Devices: ${DEVICES[*]}"
echo "=========================================================="

for DEV in "${DEVICES[@]}"; do
    echo "--- Configuring $DEV ---"

    # 1. Ensure adbd is running as root for component enablement & settings
    "${ADB_BIN}" -s "$DEV" root 2>/dev/null || true
    sleep 1

    # 2. Wake & unlock
    "${ADB_BIN}" -s "$DEV" shell input keyevent KEYCODE_WAKEUP 2>/dev/null || true
    "${ADB_BIN}" -s "$DEV" shell wm dismiss-keyguard 2>/dev/null || true

    # 3. Fix Wi-Fi & Private DNS for SLIRP NAT
    echo "  [1/5] Enabling Wi-Fi and setting private DNS off..."
    "${ADB_BIN}" -s "$DEV" shell svc wifi enable 2>/dev/null || true
    "${ADB_BIN}" -s "$DEV" shell settings put global private_dns_mode off 2>/dev/null || true

    # 3. Install Mobile Utilities APK if available
    if [ -f "$MOBUTILS_APK" ]; then
        echo "  [2/5] Installing Mobile Utilities from $MOBUTILS_APK..."
        "${ADB_BIN}" -s "$DEV" install -r -g "$MOBUTILS_APK" 2>/dev/null || true
    fi

    # 4. Override Phenotype flags for Nearby Sharing & Gesture Exchange
    echo "  [3/5] Overriding Phenotype flags for Tap-to-Share & Gesture Exchange..."

    # Gesture Exchange flags
    for PKG in "com.google.android.gms.gestureexchange#com.google.android.gms" "com.google.android.gms.gestureexchange"; do
        "${ADB_BIN}" -s "$DEV" shell am broadcast -a 'com.google.android.gms.phenotype.FLAG_OVERRIDE' \
            --es package "$PKG" \
            --es user "\*" \
            --esa flags "enable_gestureexchange,enable_noise_handshake,enable_glow_animation_v2,default_gesture_enabled,min_sdk,glow_start_waiting_timeout_millis,nfc_isodep_transceive_timeout_ms,enable_contacts_provider_single_thread_context,enable_v30_vcard_generation,check_supported_profile,check_supported_form_factor,check_disallow_outgoing_beam,check_disallow_bluetooth_sharing" \
            --esa values "true,true,true,true,30,15000,5000,true,true,false,false,false,false" \
            --esa types "boolean,boolean,boolean,boolean,long,long,long,boolean,boolean,boolean,boolean,boolean,boolean" \
            --ez commit true \
            --user 0 \
            com.google.android.gms 2>/dev/null || true
    done

    # Nearby Sharing & Tap to share flags
    for PKG in "com.google.android.gms.nearby#com.google.android.gms" "com.google.android.gms.nearby"; do
        "${ADB_BIN}" -s "$DEV" shell am broadcast -a 'com.google.android.gms.phenotype.FLAG_OVERRIDE' \
            --es package "$PKG" \
            --es user "\*" \
            --esa flags "sharing_default_visibility,TapToShareFeature__quick_share_integration,TapToShareFeature__quick_share_timeout_ms,TapToShareFeature__enable_ttx_unified_send_surface,TapToShareFeature__enable_unified_receive_surface,TapToShareFeature__always_register_foreground_in_ttx,TapToShareFeature__support_preview_for_ttx,TapToShareFeature__dedicated_tap_to_share_flow,SharingFeature__remove_permanent_everyone,SharingFeature__enable_external_provider_background_visibility,SharingFeature__keep_advertising_after_transfer,sharing_deprecate_enable_api,SharingFeature__deprecate_enable_api" \
            --esa values "3,true,30000,true,true,true,true,true,false,true,true,true,true" \
            --esa types "long,boolean,long,boolean,boolean,boolean,boolean,boolean,boolean,boolean,boolean,boolean,boolean" \
            --ez commit true \
            --user 0 \
            com.google.android.gms 2>/dev/null || true
    done
    # 5. Enable Gesture Exchange Chimera and Host APDU components
    echo "  [4/5] Enabling Gesture Exchange and Host APDU components..."
    for COMPONENT in \
        "com.google.android.gms.gestureexchange.service.HostApduService" \
        "com.google.android.gms.gestureexchange.service.HostNdefApduService" \
        "com.google.android.gms.gestureexchange.service.GestureExchangeService" \
        "com.google.android.gms.gestureexchange.service.TapToShareService" \
        "com.google.android.gms.gestureexchange.ui.GestureExchangeActivity" \
        "com.google.android.gms.gestureexchange.contacts.ContactExchangeActivity" \
        "com.google.android.gms.gestureexchange.contacts.ContactExchangeReceiver"; do
        "${ADB_BIN}" -s "$DEV" shell pm enable "com.google.android.gms/${COMPONENT}" 2>/dev/null || true
    done

    # 6. Configure AOSP NFC and Gesture Exchange secure settings
    echo "  [5/5] Configuring NFC secure settings for Gesture Exchange routing..."
    "${ADB_BIN}" -s "$DEV" shell settings put secure nfc.gesture_exchange_component "com.google.android.gms/com.google.android.gms.gestureexchange.ui.GestureExchangeActivity" 2>/dev/null || true
    "${ADB_BIN}" -s "$DEV" shell settings put secure tap_event_service_component "com.google.android.gms/com.google.android.gms.gestureexchange.service.TapToShareService" 2>/dev/null || true
    "${ADB_BIN}" -s "$DEV" shell settings put secure tap_share_fulfillment_activity_component "com.google.android.gms/com.google.android.gms.gestureexchange.ui.taptoshare.IntentFulfillmentActivity" 2>/dev/null || true
    "${ADB_BIN}" -s "$DEV" shell settings put secure nfc.gesture_poll_frame "6a01cf0000" 2>/dev/null || true
    "${ADB_BIN}" -s "$DEV" shell settings put secure nfc.gesture_poll_frame_foreground "6a01cf0000" 2>/dev/null || true
    "${ADB_BIN}" -s "$DEV" shell settings put secure nfc.gesture_exchange_secondary_aid "F00000FE2C" 2>/dev/null || true
    "${ADB_BIN}" -s "$DEV" shell settings put secure nfc.ttx_conflict_handling "1" 2>/dev/null || true

    "${ADB_BIN}" -s "$DEV" shell am force-stop com.google.android.gms 2>/dev/null || true
done

echo "=========================================================="
echo " Network & Mobile Utilities setup complete!"
echo "=========================================================="
