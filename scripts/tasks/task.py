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


class Task:
  """General Task class for modularizing tasks in Building netsim"""

  def __init__(self, name: str, enabled=False):
    self.enabled = enabled
    self.name = name

  def enable(self, enable: bool):
    self.enabled = enable

  def run(self):
    """Runs the task if it's enabled."""
    if self.enabled:
      logging.info("Running %s", self.name)
      if self.do_run():
        logging.info("%s completed!", self.name)
      else:
        logging.info("%s incomplete", self.name)
    else:
      logging.info("Skipping %s", self.name)

  def do_run(self) -> bool:
    """Subclasses should implement the concrete task.

    Returns True if the run is successful
    """
    return True
