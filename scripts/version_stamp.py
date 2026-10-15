#!/usr/bin/env python3
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

import os
import subprocess
import sys

sys.stdout.reconfigure(newline="\n")

if build_id := os.getenv("BUILD_NUMBER"):
  suffix = f"+{build_id}"
else:
  try:
    sha = subprocess.check_output(
        ["git", "describe", "--always", "--dirty=.dirty", "--exclude=*", "--abbrev=8"],
        stderr=subprocess.DEVNULL,
        text=True,
    ).strip()
    suffix = f"-dev+{sha}"
  except (OSError, subprocess.SubprocessError):
    suffix = "-dev"

print(f"STABLE_NETSIM_VERSION_SUFFIX {suffix}")
