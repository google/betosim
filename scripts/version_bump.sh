# Copyright 2022 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#      http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

#!/usr/bin/env bash
#
# Bump the version.
#
# The CARGO_PKG_VERSION is not available for Android.bp builds
# so bump versions using a script.
#

NO_UPLOAD=false
if [[ "$1" == "--no-upload" ]]; then
  NO_UPLOAD=true
fi

# Absolute path to this script
SCRIPT=$(dirname $(readlink -f "$0"))
export VERSION_FILE=$SCRIPT/../next/daemon/src/version.rs

NEW_VERSION=$(python3 - <<EOF
import re
import os

version_file = os.environ["VERSION_FILE"]
with open(version_file, "r+") as f:
    lines = f.readlines()
    next_ver_regex = re.compile(r'^const VERSION:\s&str\s=\s"(\d+)\.(\d+)\.(\d+)";$')
    new_version = ""
    for i, line in enumerate(lines):
        m = next_ver_regex.match(line)
        if m:
            new_version = "{0}.{1}.{2}".format(m[1], m[2], int(m[3]) + 1)
            lines[i] = 'const VERSION: &str = "{}";\n'.format(new_version)
            break
    f.seek(0)
    f.writelines(lines)
    f.truncate()

print(new_version)
EOF
)

if [[ -z "$NEW_VERSION" ]]; then
  echo "Error: Failed to find VERSION in $VERSION_FILE" >&2
  exit 1
fi

echo "Bumping Netsim version to $NEW_VERSION"

# Create a CL
cd "$SCRIPT/.."
repo start "version_bump_${NEW_VERSION}" .
git commit -m "Version Bump to $NEW_VERSION" "$VERSION_FILE"

if [ "$NO_UPLOAD" = true ]; then
  echo "Created commit for version $NEW_VERSION. Skipped repo upload."
  exit 0
fi

repo upload -y --cbr -o nokeycheck --label Presubmit-Ready+1 --re=formosa@google.com,shuohsu@google.com --cc=schilit@google.com .
