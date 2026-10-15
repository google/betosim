#!/bin/bash
# Copyright 2025 The Android Open Source Project
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
# script to show status of tap interfaces
# usage: ./tap-status.sh [interface_name]

IFACE=${1:-}

if [ -n "$IFACE" ]; then
    echo "Status for $IFACE:"
    ip addr show "$IFACE"
    ip link show "$IFACE"
else
    echo "All TAP interfaces:"
    ip tuntap list
    echo ""
    echo "Active TAP interfaces details:"
    ip -d link show type tun
fi
