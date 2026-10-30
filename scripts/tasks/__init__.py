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

import logging
import platform
from typing import Mapping

from tasks.build_task import BuildTask
from tasks.compile_install_task import CompileInstallTask
from tasks.configure_task import ConfigureTask
from tasks.install_emulator_task import InstallEmulatorTask
from tasks.run_pytest_task import RunPyTestTask
from tasks.run_verify_task import RunVerifyTask
from tasks.task import Task
from tasks.test_task import TestTask
from tasks.zip_artifact_task import ZipArtifactTask
from utils import is_cross_compiling

TASK_ALIASES = ["compile", "runtest"]

TASK_LIST = [
    "Configure",
    "Build",
    "CompileInstall",
    "Test",
    "ZipArtifact",
    "InstallEmulator",
    "RunPyTest",
    "RunVerify",
    "LocalRunAll",
]


def log_enabled_tasks(tasks):
  enabled_tasks = [
      task_name for task_name, task in tasks.items() if task.enabled
  ]
  logging.info(f"Enabled Tasks are {enabled_tasks}")


def get_tasks(args, env) -> Mapping[str, Task]:
  """A list of tasks that should be executed"""

  # Mapping of tasks
  tasks = {
      "Configure": ConfigureTask(args, env),
      "Build": BuildTask(args, env),
      "CompileInstall": CompileInstallTask(args, env),
      "Test": TestTask(args, env),
      "ZipArtifact": ZipArtifactTask(args),
      "InstallEmulator": InstallEmulatorTask(args),
      "RunPyTest": RunPyTestTask(args),
      "RunVerify": RunVerifyTask(args, env),
  }

  # Enable all tasks for buildbots
  if args.buildbot:
    for task_name in [
        "Configure",
        "CompileInstall",
        "Test",
        "ZipArtifact",
        "InstallEmulator",
        "RunPyTest",
    ]:
      tasks[task_name].enable(True)
    if is_cross_compiling(args):
      tasks["InstallEmulator"].enable(False)
      tasks["RunPyTest"].enable(False)
    return tasks

  # Define the complete task map declaratively.
  task_map = {
      "configure": ["Configure"],
      "build": ["Build"],
      "compile": ["Build"],
      "compileinstall": ["CompileInstall"],
      "test": ["Test"],
      "runtest": ["Test"],
      "zipartifact": ["ZipArtifact"],
      "installemulator": ["InstallEmulator"],
      "runpytest": ["RunPyTest"],
      "runverify": ["RunVerify"],
      "fullbuild": ["Configure", "Build", "InstallEmulator"],
      "localrunall": [
          "Configure",
          "CompileInstall",
          "Test",
          "InstallEmulator",
          "RunPyTest",
      ],
  }

  # Handle the default case and convert to a set for efficient lookup.
  # If `localrunall` is present, it becomes the only task.
  user_tasks = {t.lower() for t in args.task or ["configure"]}
  if "localrunall" in user_tasks:
    user_tasks = {"localrunall"}

  # A single, clean loop to enable all required tasks.
  for task_name in user_tasks:
    if task_name in task_map:
      for task_to_enable in task_map[task_name]:
        tasks[task_to_enable].enable(True)
    else:
      logging.error(f"Unknown task: {task_name}")

  return tasks
