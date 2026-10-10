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

# Wrapper to run android networking end-to-end test suite

RUNNER_BIN=$1
APK_PATH=$2
NETSIM_BIN=$3
NETSIM_CLI_BIN=$4
EXTRA_FLAGS=("${@:5}")

echo "[runner@host] Running ntest run"
echo "[runner@host] APK: $APK_PATH"
echo "[runner@host] Netsimd: $NETSIM_BIN"
echo "[runner@host] Netsim CLI: $NETSIM_CLI_BIN"
echo "[runner@host] Extra Flags: ${EXTRA_FLAGS[*]}"

# Run ntest run command
$RUNNER_BIN run \
    --apk-path "$APK_PATH" \
    --netsim-path "$NETSIM_BIN" \
    --netsim-cli-path "$NETSIM_CLI_BIN" \
    "${EXTRA_FLAGS[@]}"
