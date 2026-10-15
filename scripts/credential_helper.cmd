@echo off
REM Copyright 2026 The Android Open Source Project
REM
REM Licensed under the Apache License, Version 2.0 (the "License");
REM you may not use this file except in compliance with the License.
REM You may obtain a copy of the License at
REM
REM     http://www.apache.org/licenses/LICENSE-2.0
REM
REM Unless required by applicable law or agreed to in writing, software
REM distributed under the License is distributed on an "AS IS" BASIS,
REM WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
REM See the License for the specific language governing permissions and
REM limitations under the License.
REM
REM Bazel credential helper used to fetch the GCS-hosted hermetic SDK archives
REM selected by --config=hermetic. See credential_helper.sh for why this thin
REM launcher exists instead of pointing --credential_helper at the shared
REM build/bazel/utils/credential_helper.cmd.
REM
REM %~dp0 is tools\netsim\scripts\, so ..\..\.. is the Android tree root.
"%~dp0..\..\..\prebuilts\python\windows-x86\python.exe" "%~dp0..\..\..\build\bazel\utils\cred_helper.py" %*
