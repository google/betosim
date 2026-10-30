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
from pathlib import Path
import platform
import shutil
import subprocess
import xml.etree.ElementTree as ET

from tasks.task import Task
from utils import (
    NETSIM_ROOT,
    get_bazel_build_configs,
    get_bazel_path,
    get_bazel_startup_options,
    get_bazel_targets,
    is_cross_compiling,
    run,
)

PLATFORM_SYSTEM = platform.system()


class TestTask(Task):

  def __init__(self, args, env):
    super().__init__("Test")
    self.args = args
    self.buildbot = args.buildbot
    self.out = Path(args.out_dir)
    self.env = env

  def do_run(self):
    # Bazel Test
    bazel = get_bazel_path()
    build_configs = get_bazel_build_configs(self.args, self.env)
    startup_options = get_bazel_startup_options()
    targets = get_bazel_targets(self.args)

    test_cmd = (
        ["test", "--build_tests_only", "--run_tests=false"]
        if is_cross_compiling(self.args)
        else ["test", "--test_output=streamed"]
    )

    try:
      run(
          [bazel]
          + startup_options
          + test_cmd
          + targets
          + build_configs,
          self.env,
          "bazel test",
          NETSIM_ROOT,
      )
    except Exception as e:
      if self.buildbot:
        self._copy_bazel_test_logs()
      raise e
    return True

  def _copy_bazel_test_logs(self):
    bazel = get_bazel_path()
    startup_options = get_bazel_startup_options()

    # Query Bazel for the actual testlogs directory
    try:
      result = subprocess.run(
          [str(bazel)] + startup_options + ["info", "testlogs"],
          capture_output=True,
          text=True,
          cwd=NETSIM_ROOT,
          env=self.env,
          check=True,
      )
      testlogs_dir = Path(result.stdout.strip())
    except subprocess.CalledProcessError as e:
      logging.warning(f"Failed to get testlogs dir via bazel info: {e}")
      testlogs_dir = NETSIM_ROOT / "bazel-testlogs"

    dest_dir = Path(self.args.dist_dir).absolute() / "logs" / "bazel-logs"

    if testlogs_dir.exists():
      logging.info(
          f"Scanning Bazel test logs in {testlogs_dir} for failures..."
      )
      for xml_path in testlogs_dir.glob("**/test.xml"):
        try:
          tree = ET.parse(xml_path)
          root = tree.getroot()
          failed = False
          if root.tag == "testsuites":
            for suite in root.findall("testsuite"):
              failures = int(suite.get("failures", 0))
              errors = int(suite.get("errors", 0))
              if failures > 0 or errors > 0:
                failed = True
                break
          elif root.tag == "testsuite":
            failures = int(root.get("failures", 0))
            errors = int(root.get("errors", 0))
            if failures > 0 or errors > 0:
              failed = True

          log_path = xml_path.parent / "test.log"
          if failed and log_path.exists():
            rel_path = log_path.relative_to(testlogs_dir)
            dest_path = dest_dir / rel_path
            dest_path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(log_path, dest_path)
            logging.info(f"Copied failed test log to {dest_path}")
        except ET.ParseError as e:
          logging.warning(f"Failed to parse XML {xml_path}: {e}")
        except ValueError as e:
          logging.warning(f"Value error processing {xml_path}: {e}")
        except OSError as e:
          logging.warning(f"I/O error processing {xml_path}: {e}")
    else:
      logging.warning(f"Bazel test logs directory not found at {testlogs_dir}")
