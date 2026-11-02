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
# script to bring up a tap interface for netsim wifi
# usage: ./tap-up.sh <interface_name> [ip_address/mask]

IFACE=${1:-tap0}
IP=${2:-192.168.10.1/24}

echo "Creating TAP interface $IFACE..."
sudo ip tuntap add dev "$IFACE" mode tap user "$USER"
sudo ip addr add "$IP" dev "$IFACE"
sudo ip link set "$IFACE" up
echo "TAP interface $IFACE created with IP $IP"
