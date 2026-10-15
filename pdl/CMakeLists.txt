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

message(STATUS "PDL netsim packets module.")

get_property(pdlc_EXECUTABLE TARGET pdlc PROPERTY EXECUTABLE_PATH)

pdl_gen(
  NAME
  NetlinkPackets_rs
  INPUT
  ${CMAKE_CURRENT_LIST_DIR}/netlink.pdl
  OUTPUT
  netlink_packets.rs
  LANG
  rust)

pdl_gen(
  NAME
  Mac80211HwsimPackets_rs
  INPUT
  ${CMAKE_CURRENT_LIST_DIR}/mac80211_hwsim.pdl
  OUTPUT
  mac80211_hwsim_packets.rs
  LANG
  rust)

pdl_gen(
  NAME
  Ieee80211Packets_rs
  INPUT
  ${CMAKE_CURRENT_LIST_DIR}/ieee80211.pdl
  OUTPUT
  ieee80211_packets.rs
  LANG
  rust)

pdl_gen(
  NAME
  LlcPackets_rs
  INPUT
  ${CMAKE_CURRENT_LIST_DIR}/llc.pdl
  OUTPUT
  llc_packets.rs
  LANG
  rust)

pdl_gen(
  NAME
  ArpPackets_rs
  INPUT
  ${CMAKE_CURRENT_LIST_DIR}/arp.pdl
  OUTPUT
  arp_packets.rs
  LANG
  rust)
