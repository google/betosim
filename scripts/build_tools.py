#!/usr/bin/env python3
# Copyright 2023 The Android Open Source Project
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
from __future__ import absolute_import, division, print_function

import argparse
import logging
import os
from pathlib import Path

from environment import get_default_environment
from server_config import ServerConfig
from tasks import (
    TASK_ALIASES,
    TASK_LIST,
    get_tasks,
    log_enabled_tasks,
)
from utils import (
    AOSP_ROOT,
    config_logging,
    create_emulator_artifact_path,
    default_target,
    fetch_build_chaining_artifacts,
    is_presubmit,
    log_system_info,
)


def main():
  config_logging()
  log_system_info()
  create_emulator_artifact_path()

  parser = argparse.ArgumentParser(
      description="Configures and builds the android netsim project"
  )
  parser.add_argument(
      "--out_dir",
      type=str,
      default="tools/netsim/objs/",
      help="The output directory",
  )
  parser.add_argument(
      "--dist_dir", type=str, default="dist/", help="The destination directory"
  )
  parser.add_argument(
      "--build-id",
      type=str,
      default="",
      dest="build_id",
      help="The netsim build number",
  )
  parser.add_argument(
      "--target",
      type=str,
      default=default_target(),
      help="The build target, defaults to current os",
  )
  parser.add_argument(
      "--enable_system_rust",
      action="store_true",
      help="Build the netsim with the System Rust on the host machine",
  )
  parser.add_argument(
      "--with_debug", action="store_true", help="Build debug instead of release"
  )
  parser.add_argument(
      "--buildbot", action="store_true", help="Invoked by Android buildbots"
  )
  parser.add_argument(
      "--task",
      nargs="+",
      type=str.lower,
      choices=[choice.lower() for choice in TASK_LIST] + TASK_ALIASES,
      metavar="TASK",
      help=(
          "Tasks to perform (Configure, Build, CompileInstall,"
          " InstallEmulator, RunPyTest, LocalRunAll)"
      ),
  )
  parser.add_argument(
      "--config",
      default="release",
      choices=["debug", "release"],
      help="Whether we are building a release or debug configuration.",
  )
  parser.add_argument(
      "--emulator_build_id",
      type=str,
      default="",
      dest="emulator_build_id",
      help="The emulator build number",
  )
  parser.add_argument(
      "--emulator_target",
      type=str,
      default="emulator-linux_x64_gfxstream",
      help=(
          "The emulator build target to install for local case, defaults to"
          " emulator-linux_x64_gfxstream"
      ),
  )
  parser.add_argument(
      "--local_emulator_dir",
      type=str,
      default="",
      help=(
          "For providing an emulator build artifact in a directory."
          " This will install the emulator from local_emulator_dir instead of"
          " fetching the artifacts"
      ),
  )
  parser.add_argument(
      "--pytest_input_dir",
      type=str,
      default="",
      help=(
          "For providing netsim & emulator binaries and libraries for pytest."
          " This will allow pytest to be run on directory path specified on"
          " pytest_input_dir"
      ),
  )
  parser.add_argument(
      "--crate",
      type=str,
      nargs="+",
      default=[],
      help="Deprecated: Use --bazel_targets instead.",
  )
  # TODO: Remove --bazel argument in the future. It is now the default.
  parser.add_argument(
      "--bazel",
      action="store_true",
      help="Deprecated: Bazel is now the default build system.",
  )
  parser.add_argument(
      "--bazel_targets",
      type=str,
      nargs="+",
      default=[],
      help="The bazel targets to build.",
  )
  parser.add_argument(
      "--hermetic",
      action="store_true",
      help="Whether to run a hermetic build.",
  )
  parser.add_argument(
      "--clean",
      action="store_true",
      help="Clean the build directory before building.",
  )
  parser.add_argument(
      "--enable_repo_cache",
      action="store_true",
      help=(
          "Enable Bazel repository contents cache (it is disabled by default)."
      ),
  )

  args = parser.parse_args()

  if (
      args.crate
      and "test" not in (args.task or [])
      and "runtest" not in (args.task or [])
  ):
    parser.error("argument --crate: can only be used with --task test")

  presubmit = is_presubmit(args.build_id)

  # The environment of build
  env = get_default_environment(AOSP_ROOT)
  if args.buildbot:
    cfg = ServerConfig(presubmit, args)
    env = cfg.get_env()
    if args.build_id:
      env["BUILD_NUMBER"] = args.build_id

  # Set Environment Variables
  os.environ["GIT_DISCOVERY_ACROSS_FILESYSTEM"] = "1"
  # Provide absolute path for args.out_dir
  if not os.path.isabs(args.out_dir):
    args.out_dir = os.path.join(AOSP_ROOT, args.out_dir)

  # Build preparation work for buildbot
  if args.buildbot:
    # Fetch Emulator Artifacts
    fetch_build_chaining_artifacts(args.out_dir, presubmit)
    # Set the out_dir to "out/objs"
    args.out_dir = Path(args.out_dir) / "objs"

  # Obtain tasks
  tasks = get_tasks(args, env)

  # Log enabled tasks
  log_enabled_tasks(tasks)

  # Turn on sccache?
  # if args.buildbot and cfg.sccache:
  #    launcher.append(f"-DOPTION_CCACHE=${cfg.sccache}")

  # Configure
  tasks.get("Configure").run()

  # Build
  tasks.get("Build").run()

  # Install
  tasks.get("CompileInstall").run()

  # Run Tests
  tasks.get("Test").run()

  # Zip results..
  tasks.get("ZipArtifact").run()

  # Install Emulator artifacts and Run PyTests
  tasks.get("InstallEmulator").run()
  tasks.get("RunPyTest").run()

  # Run Verify tests
  tasks.get("RunVerify").run()


if __name__ == "__main__":
  main()
