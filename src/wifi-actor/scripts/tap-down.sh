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
# script to tear down a tap interface for netsim wifi
# usage: ./tap-down.sh <interface_name>

IFACE=${1:-tap0}

echo "Removing TAP interface $IFACE..."
sudo ip link set "$IFACE" down
sudo ip tuntap del dev "$IFACE" mode tap
echo "TAP interface $IFACE removed"
