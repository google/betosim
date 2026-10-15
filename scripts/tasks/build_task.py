#!/usr/bin/env python3
# Copyright 2024 The Android Open Source Project
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

from pathlib import Path
import platform
import shutil

from tasks.task import Task
from utils import (
    NETSIM_ROOT,
    get_bazel_build_configs,
    get_bazel_path,
    get_bazel_startup_options,
    get_bazel_targets,
    run,
)


class BuildTask(Task):

  def __init__(self, args, env):
    super().__init__("Build")
    self.args = args
    self.out = Path(args.out_dir)
    self.env = env

  def do_run(self):
    return self._run_bazel()

  def _run_bazel(self):
    bazel = get_bazel_path()
    build_configs = get_bazel_build_configs(self.args, self.env)
    startup_options = get_bazel_startup_options()
    targets = get_bazel_targets(self.args)

    run(
        [bazel] + startup_options + ["build"] + targets + build_configs,
        self.env,
        "bazel build",
        NETSIM_ROOT,
    )
    return True
