#!/bin/bash -eu
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

# Get the directory of the script
REPO=$(dirname "$0")/../../..

# The possible values are "linux" and "darwin".
OS=$(uname | tr '[:upper:]' '[:lower:]')

OUT_PATH="$1"
RUST_VERSION="$2"
CLIPPY_FLAGS="$3"

source $REPO/tools/netsim/scripts/cargo_env.sh $OUT_PATH

pushd $REPO/tools/netsim/rust
# Run the cargo command
# TODO(360874898): prebuilt rust toolchain for darwin-aarch64 is supported from 1.77.1
if [[ "$OS" == "darwin" && $(uname -m) == "arm64" ]]; then
  cargo clippy -- $CLIPPY_FLAGS
else
  $REPO/prebuilts/rust/$OS-x86/$RUST_VERSION/bin/cargo clippy -- $CLIPPY_FLAGS
fi
popd
