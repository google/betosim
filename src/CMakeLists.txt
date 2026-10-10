# Copyright 2022 The Android Open Source Project
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
add_subdirectory(util)
if(NOT NETSIM_EXT)
  add_subdirectory(backend)
endif()

if(TARGET Rust::Rustc)
  set(cxx_bridge_binary_folder
      ${CMAKE_BINARY_DIR}/cargo/build/${Rust_CARGO_TARGET_CACHED}/cxxbridge)
  set(common_header ${cxx_bridge_binary_folder}/rust/cxx.h)
  set(cxx_bridge_source_file "src/ffi.rs")
  set(crate_name "netsim-daemon")
  set(binding_header
      ${cxx_bridge_binary_folder}/${crate_name}/${cxx_bridge_source_file}.h)
  set(binding_source
      ${cxx_bridge_binary_folder}/${crate_name}/${cxx_bridge_source_file}.cc)

  # Make sure we have the cxx files generated before we build them.
  add_custom_command(OUTPUT ${common_header} ${binding_header} ${binding_source}
                     COMMAND DEPENDS cargo-build_${crate_name})

  android_add_library(
    TARGET netsimd-lib
    LICENSE Apache-2.0
    SRC ${binding_header}
        ${binding_source}
        ${common_header}
        hci/async_manager.cc
        hci/bluetooth_facade.cc
        hci/bluetooth_facade.h
        hci/hci_packet_transport.cc
        hci/hci_packet_transport.h
        hci/rust_device.cc
        hci/rust_device.h
    DEPS grpc++ libbt-rootcanal netsimd-proto-lib packet-streamer-proto-lib
         protobuf::libprotobuf util-lib)

  target_link_libraries(
    netsimd-lib PUBLIC android-emu-base android-emu-net
    PRIVATE libslirp glib2 ssl hostapd android-emu-base-headers)

  # Update to protobuf 26.x introduces some warnings.
  target_compile_options(netsimd-lib PRIVATE -Wno-unused-result)

  target_include_directories(netsimd-lib PRIVATE . ${PROTOBUF_INCLUDE_DIR}
                             PUBLIC ${cxx_bridge_binary_folder})
  target_compile_definitions(netsimd-lib PUBLIC NETSIM_ANDROID_EMULATOR)
  # Make sure we have the cxx files generated before we build them.
  add_dependencies(netsimd-lib cargo-build_netsim-daemon)
endif()
