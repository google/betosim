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
from collections import UserDict
import json
import logging
import os
from pathlib import Path
import platform
import subprocess
from utils import rust_version


class BaseEnvironment(UserDict):
  """Base class for environment management, providing a common foundation for

  both Posix and Windows environments.
  """

  def __init__(self, aosp: Path):
    paths = [
        str(
            aosp
            / "external"
            / "qemu"
            / "android"
            / "third_party"
            / "chromium"
            / "depot_tools"
        )
    ]

    # Append prebuilt rust toolchain except for darwin_aarch64
    # TODO(360874898): aarch64-apple-darwin prebuilt rust toolchain supported from 1.77.1
    if not (platform.system() == "Darwin" and platform.machine() == "arm64"):
      paths.append(
          str(
              aosp
              / "prebuilts"
              / "rust"
              / f"{platform.system().lower()}-x86"
              / f"{rust_version()}"
              / "bin"
          )
      )

    paths.append(os.environ.get("PATH", ""))

    super().__init__({"PATH": os.pathsep.join(paths)})


class PosixEnvironment(BaseEnvironment):

  def __init__(self, aosp: Path):
    super().__init__(aosp)


class VisualStudioNotFoundException(Exception):
  pass


class VisualStudioMissingVarException(Exception):
  pass


class VisualStudioNativeWorkloadNotFoundException(Exception):
  pass


class WindowsEnvironment(BaseEnvironment):
  """Environment manager for Windows systems, specifically handling Visual Studio integration."""

  def __init__(self, aosp: Path):
    assert platform.system() == "Windows"
    super().__init__(aosp)
    for key in os.environ:
      self[key.upper()] = os.environ[key]

    vs = self._visual_studio()
    logging.info("Loading environment from %s", vs)
    env_lines = subprocess.check_output(
        [vs, "&&", "set"], encoding="utf-8"
    ).splitlines()
    for line in env_lines:
      if "=" in line:
        key, val = line.split("=", 1)
        # Variables in windows are case insensitive, but not in python dict!
        self[key.upper()] = val

    # Set PYTHONUTF8 to 1
    self["PYTHONUTF8"] = "1"

    if not "VSINSTALLDIR" in self:
      raise VisualStudioMissingVarException(
          "Missing VSINSTALLDIR in environment"
      )

    if not "VCTOOLSINSTALLDIR" in self:
      raise VisualStudioMissingVarException(
          "Missing VCTOOLSINSTALLDIR in environment"
      )

  def _visual_studio(self) -> Path:
    """Locates the Visual Studio installation and its Native Desktop workload.

    Raises:
        VisualStudioNotFoundException: When Visual Studio is not found.
        VisualStudioNativeWorkloadNotFoundException: When the Native Desktop
        workload is not found.

    Returns:
        Path: Path to the Visual Studio vcvars64.bat file.
    """
    prgrfiles = Path(os.getenv("ProgramFiles(x86)", r"C:\Program Files (x86)"))
    res = subprocess.check_output([
        str(
            prgrfiles / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
        ),
        "-requires",
        "Microsoft.VisualStudio.Workload.NativeDesktop",
        "-sort",
        "-format",
        "json",
        "-utf8",
    ])
    vsresult = json.loads(res)
    if len(vsresult) == 0:
      raise VisualStudioNativeWorkloadNotFoundException(
          "No visual studio with the native desktop load available."
      )

    for install in vsresult:
      logging.debug("Considering %s", install["displayName"])
      candidates = list(
          Path(install["installationPath"]).glob("**/vcvars64.bat")
      )

      if len(candidates) > 0:
        return candidates[0].absolute()

    # Oh oh, no visual studio..
    raise VisualStudioNotFoundException(
        "Unable to detect a visual studio installation with the native desktop"
        " workload."
    )


def get_default_environment(aosp: Path):
  """Returns the appropriate environment manager based on the current operating system.

  The environment will make sure the following things hold:

  - Ninja will be on the PATH
  - The visual studio tools environment will be loaded
  """
  if platform.system() == "Windows":
    return WindowsEnvironment(aosp)
  return PosixEnvironment(aosp)
