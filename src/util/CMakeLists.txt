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

android_add_library(
  TARGET util-lib
  LICENSE Apache-2.0
  SRC crash_report.cc
      crash_report.h
      filesystem.h
      ini_file.cc
      ini_file.h
      log.cc
      log.h
      os_utils.cc
      os_utils.h
      string_utils.cc
      string_utils.h)
target_include_directories(util-lib PRIVATE ..)
target_compile_definitions(util-lib PUBLIC NETSIM_ANDROID_EMULATOR)
