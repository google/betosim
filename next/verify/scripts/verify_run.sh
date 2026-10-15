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
# A robust wrapper for testing the netsimd E2E runner.
# This script manages the lifecycle of netsimd and android emulators, ensuring
# proper daemon cleanup via bash traps and robust wifi network state verification
# prior to invoking the bazel runner-e2e test suite.

set -e
set -o pipefail

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
WORKSPACE_DIR="$( cd "$DIR/../../../../../" && pwd )"

OS_NAME=$(uname -s | tr '[:upper:]' '[:lower:]')
case "$OS_NAME" in
  linux*) OS_NAME="linux" ;;
  darwin*) OS_NAME="darwin" ;;
  mingw*|cygwin*|msys*) OS_NAME="windows" ;;
  *) echo "Unsupported OS: $OS_NAME"; exit 1 ;;
esac

bazel() {
  local BAZEL_BINARY="bazel"
  if [ "$OS_NAME" = "windows" ]; then
    BAZEL_BINARY="bazel.exe"
  fi

  local bazel_path="$WORKSPACE_DIR/prebuilts/bazel/$OS_NAME-x86_64/$BAZEL_BINARY"
  if [[ -x "$bazel_path" ]]; then
    "$bazel_path" "$@"
  else
    command bazel "$@"
  fi
}

export ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-${ANDROID_SDK_HOME:-$HOME/Android/Sdk}}}"
export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH"

# Default number of emulators. Can be overridden with -n 3 for UWB tests.
NUM_EMULATORS=2
TEST_FILTER=""
DRY_RUN=false
VERBOSE=false
NO_BUILD=false
NETSIMD_PATH=""
RUNNER_PATH=""
APK_PATH=""
CLI_PATH=""
SPEC_DIR=
EMULATOR_PATH="emulator"
KEEP_GOING=false
IGNORE_TAGS_SET=false

while [[ "$#" -gt 0 ]]; do
  case $1 in
    -v|--verbose) VERBOSE=true ;;
    -n|--num-emulators) NUM_EMULATORS="$2"; shift ;;
    -f|--filter) TEST_FILTER="$2"; shift ;;
    -d|--dry-run) DRY_RUN=true ;;
    -k|--keep-going) KEEP_GOING=true ;;
    --no-build) NO_BUILD=true ;;
    --netsimd-path) NETSIMD_PATH="$2"; shift ;;
    --runner-path) RUNNER_PATH="$2"; shift ;;
    --apk-path) APK_PATH="$2"; shift ;;
    --cli-path) CLI_PATH="$2"; shift ;;
    --spec-dir) SPEC_DIR="$2"; shift ;;
    --ignore-tags) IGNORE_TAGS="$2"; IGNORE_TAGS_SET=true; shift ;;
    --emulator-path) EMULATOR_PATH="$2"; shift ;;
    -h|--help)
      echo "Usage: $0 [options]"
      echo "Options:"
      echo "  -n, --num-emulators <count>   Number of emulators to start (1, 2, or 3). Default: 2"
      echo "  -f, --filter <regex>          Regex to filter tests to run"
      echo "  -d, --dry-run                 Simulation mode (does not start emulators, passes --dry-run to runner)"
      echo "  -v, --verbose                 Enable verbose test runner output"
      echo "  -k, --keep-going              Continue running tests after a failure"
      echo "  --no-build                    Skip Bazel build step"
      echo "  --netsimd-path <path>         Path to netsimd binary"
      echo "  --runner-path <path>          Path to runner binary"
      echo "  --apk-path <path>             Path to vbs APK"
      echo "  --cli-path <path>             Path to netsim CLI binary"
      echo "  --spec-dir <path>             Directory containing feature files (specs)"
      echo "  --ignore-tags <tags>          Comma-separated list of tags to ignore"
      echo "  --emulator-path <path>        Path to emulator binary"
      exit 0
      ;;
    *) echo "Unknown parameter passed: $1"; exit 1 ;;
  esac
  shift
done

if [[ "$NUM_EMULATORS" -lt 1 || "$NUM_EMULATORS" -gt 3 ]]; then
  echo "ERROR: --num-emulators must be 1, 2, or 3"
  exit 1
fi

# Dynamic configuration based on available AVDs.
SELECTED_EMULATORS=()
if [ "$DRY_RUN" = false ]; then
  AVDS=($(emulator -list-avds))
  if [ ${#AVDS[@]} -lt "$NUM_EMULATORS" ]; then
    echo "ERROR: Need at least $NUM_EMULATORS AVDs, found ${#AVDS[@]}"
    exit 1
  fi
  for ((i=0; i<NUM_EMULATORS; i++)); do
    SELECTED_EMULATORS+=("@${AVDS[i]}")
  done
fi


NETSIMD_PID=""
EMU_PIDS=()

cleanup() {
  echo -n "Cleaning up dangling processes from E2E suite..."

  # Kill locally tracked PIDs if they exist
  if [ -n "$NETSIMD_PID" ]; then kill -TERM $NETSIMD_PID 2>/dev/null || true; fi
  for pid in "${EMU_PIDS[@]}"; do
    kill -TERM "$pid" 2>/dev/null || true
  done

  # Force-kill any lingering netsimd instances started by other means
  pkill -u $(whoami) -9 -f "netsim\+/next/daemon/daemon" 2>/dev/null || true
  for emu in "${SELECTED_EMULATORS[@]}"; do
    pkill -u $(whoami) -9 -f "qemu-system-x86_64.*${emu#@}" 2>/dev/null || true
  done
  killall -9 netsimd 2>/dev/null || true

  echo " completed."
}

# Ensure cleanup runs on exit or interrupt
trap cleanup EXIT INT TERM

echo "================================================="
echo "1. Building the netsimd binary and E2E runner via Bazel"
echo "================================================="
if [ "$NO_BUILD" = false ]; then
  cd "$WORKSPACE_DIR"
  bazel build \
    --spawn_strategy=standalone \
    @netsim//next/daemon:daemon \
    @netsim//next/cli:netsim \
    @netsim//next/verify/runner:runner-e2e
else
  echo "Skipping Bazel build as requested."
fi

echo "================================================="
echo "2. Aborting any previous runs"
echo "================================================="
cleanup

echo "================================================="
echo "3. Starting Netsim daemon"
echo "================================================="

BAZEL_BIN=$(bazel info bazel-bin 2>/dev/null || echo "$WORKSPACE_DIR/bazel-bin")
BAZEL_OUT=$(dirname $(dirname "$BAZEL_BIN"))

if [ -z "$NETSIMD_PATH" ]; then
  NETSIMD_BIN=$(find "$BAZEL_OUT" -path "*/daemon/daemon" -type f | head -n 1)
  if [ -z "$NETSIMD_BIN" ]; then
    NETSIMD_BIN="$BAZEL_BIN/external/netsim+/next/daemon/daemon"
  fi
else
  NETSIMD_BIN="$NETSIMD_PATH"
fi
$NETSIMD_BIN --no-shutdown -v --logtostderr > /tmp/netsimd_e2e.log 2>&1 &
NETSIMD_PID=$!

echo -n "Waiting for netsim gRPC to bind..."
WAIT_CYCLES=0

if [ -z "$CLI_PATH" ]; then
  CLI_PATH=$(find "$BAZEL_OUT" -path "*/cli/netsim" -type f | head -n 1)
  if [ -z "$CLI_PATH" ]; then
    CLI_PATH="$BAZEL_BIN/external/netsim+/next/cli/netsim"
  fi
fi
until $CLI_PATH version 2>/dev/null | grep -i -q "version"; do
  sleep 1
  WAIT_CYCLES=$((WAIT_CYCLES + 1))
  if [ $WAIT_CYCLES -gt 30 ]; then
    echo " ERROR: netsimd failed to start or bind within 30 seconds."
    exit 1
  fi
done
echo " connected!"

if [ "$DRY_RUN" = false ]; then
  echo "================================================="
  echo "4. Launching $NUM_EMULATORS Emulator(s) (logs in /tmp/emulator_*)"
  echo "================================================="
  for emu in "${SELECTED_EMULATORS[@]}"; do
    "$EMULATOR_PATH" "$emu" -no-window -no-audio -no-snapshot -wipe-data > "/tmp/emulator_${emu#@}_e2e.log" 2>&1 &
    EMU_PIDS+=($!)
  done

  WAIT_BOOT=0
  until [ "$(adb devices | grep -c -w "device")" -ge "$NUM_EMULATORS" ]; do
    for i in "${!EMU_PIDS[@]}"; do
      if ! kill -0 "${EMU_PIDS[i]}" 2>/dev/null; then
        echo "ERROR: Emulator $((i+1)) crashed!"
        exit 1
      fi
    done
    if [ $WAIT_BOOT -gt 60 ]; then
      echo "ERROR: Emulators failed to appear in adb within 5 minutes."
      exit 1
    fi
    sleep 5
    WAIT_BOOT=$((WAIT_BOOT + 1))
  done

  for dev in $(adb devices | grep -w "device" | awk '{print $1}'); do
    START_TIME=$SECONDS
    echo -n "Waiting for $dev initialization (boot, stack, wlan0)... "
    until [ "$(adb -s $dev shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]; do
      sleep 3
    done

    until adb -s $dev shell ping -c 1 -W 1 10.0.2.2 > /dev/null 2>&1; do
      sleep 2
    done

    adb -s $dev shell cmd wifi connect-network AndroidWifi open > /dev/null 2>&1 || true

    # Time out after 30 seconds if IP fails
    WAIT_CYCLES=0
    until adb -s $dev shell ip addr show wlan0 2>/dev/null | grep -q "inet "; do
      sleep 2
      WAIT_CYCLES=$((WAIT_CYCLES + 1))
      if [ $WAIT_CYCLES -gt 15 ]; then
          echo " ERROR: $dev wlan0 failed to associate or pull an IP!"
          exit 1
      fi
    done
    ELAPSED=$(( SECONDS - START_TIME ))
    echo "completed in ${ELAPSED}s."
    adb -s $dev shell ip addr show wlan0 | grep "inet " > /dev/null 2>&1 || true
  done
else
  echo "================================================="
  echo "4. Skipping Emulator Launch (Dry Run Mode)"
  echo "================================================="
fi

echo "================================================="
echo "5. Running E2E Test Suite via Runner Binary"
echo "================================================="

if [ -z "$RUNNER_PATH" ]; then
  RUNNER_PATH=$(find "$BAZEL_OUT" -path "*/verify/runner/runner" -type f -exec ls -t {} + | head -n 1)
  if [ -z "$RUNNER_PATH" ]; then
    RUNNER_PATH="$BAZEL_BIN/external/netsim+/next/verify/runner/runner"
  fi
fi

if [ -z "$APK_PATH" ]; then
  APK_PATH=$(find "$BAZEL_OUT" -path "*/verify/instrumentation/vbs/vbs.apk" -type f -exec ls -t {} + | head -n 1)
  if [ -z "$APK_PATH" ]; then
    APK_PATH="$BAZEL_BIN/external/netsim+/next/verify/instrumentation/vbs/vbs.apk"
  fi
fi

echo "INFO: Using APK_PATH=$APK_PATH"
if [ ! -f "$APK_PATH" ]; then
  echo "ERROR: APK file not found at $APK_PATH"
  exit 1
fi

RUNNER_ARGS=()
if [ -n "$TEST_FILTER" ]; then
  RUNNER_ARGS+=(--filter "$TEST_FILTER")
fi

if [ "$DRY_RUN" = true ]; then
  RUNNER_ARGS+=(--dry-run)
fi

if [ "$VERBOSE" = true ]; then
  RUNNER_ARGS+=(--verbose)
fi

if [ -z "$SPEC_DIR" ]; then
  SPEC_DIR="$WORKSPACE_DIR/tools/netsim/next/reqs/bdd"
fi
RUNNER_ARGS+=(--spec-dir "$SPEC_DIR")

if [ "$IGNORE_TAGS_SET" = true ]; then
  RUNNER_ARGS+=(--ignore-tags "$IGNORE_TAGS")
fi

if [ "$KEEP_GOING" = true ]; then
  RUNNER_ARGS+=(--keep-going)
fi

"$RUNNER_PATH" run \
  --android-home "$ANDROID_HOME" \
  --apk-path "$APK_PATH" \
  "${RUNNER_ARGS[@]}"

echo "Tests finished successfully!"
# Trap will trigger the cleanup of all tracked PIDs here.
