#!/usr/bin/env python
# Copyright 2021 The Android Open Source Project
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
import ast
import glob
import json
import logging
import os
from pathlib import Path
import platform
from queue import Queue
import re
import shutil
import socket
import subprocess
import sys
import threading

from time_formatter import TimeFormatter

AOSP_ROOT = Path(__file__).absolute().parents[3]
NETSIM_ROOT = Path(__file__).absolute().parents[1]
WINDOWS_TMP_OBJS_PATH = Path("C:\\netsim\\objs")
TOOLS = Path(AOSP_ROOT, "tools")
EMULATOR_ARTIFACT_PATH = Path(AOSP_ROOT, "tools", "netsim", "emulator_tmp")
PYTHON_EXE = sys.executable or "python3"
TARGET_MAP = {
    "windows": "windows_msvc-x86_64",
    "windows_x64": "windows_msvc-x86_64",
    "windows_x86_64": "windows_msvc-x86_64",
    "linux": "linux-x86_64",
    "linux_x64": "linux-x86_64",
    "linux_x86_64": "linux-x86_64",
    "linux_aarch64": "linux-aarch64",
    "darwin": "darwin-x86_64",
    "darwin_x64": "darwin-x86_64",
    "darwin_x86_64": "darwin-x86_64",
    "darwin_aarch64": "darwin-aarch64",
}


def rust_version() -> str:
  """Returns rust version"""
  with open(
      AOSP_ROOT / "build" / "bazel" / "toolchains" / "tool_versions.json",
      encoding="utf-8",
  ) as f:
    return json.load(f)["rust"]


def default_target() -> str:
  """Returns default value for target"""
  # If Mac M1, the default target should be 'darwin-aarch64'
  if platform.system() == "Darwin" and platform.machine() == "arm64":
    return "darwin-aarch64"
  return platform.system()


def create_emulator_artifact_path():
  """Refresh or construct EMULATOR_ARTIFACT_PATH"""
  if EMULATOR_ARTIFACT_PATH.exists():
    shutil.rmtree(EMULATOR_ARTIFACT_PATH)
  EMULATOR_ARTIFACT_PATH.mkdir(exist_ok=True, parents=True)


def fetch_build_chaining_artifacts(out_dir, presubmit):
  """Fetch the Emulator prebuilts for build_bots"""
  try:
    out = Path(out_dir)
    prebuilt_path = out / "prebuilt_cached" / "artifacts"
    files = glob.glob(str(prebuilt_path / f"*.zip"))
    for file in files:
      shutil.copy2(prebuilt_path / file, EMULATOR_ARTIFACT_PATH)
  except Exception as e:
    if presubmit:
      raise e
    else:
      logging.warn(
          f"An error occurred during fetch_build_chaining_artifacts: {e}"
      )


def binary_extension(filename):
  """Appends exe extension in case of Windows"""
  if platform.system() == "Windows":
    return filename + ".exe"
  return filename


def get_netsim_binaries():
  """Returns a dictionary mapping netsim binary names to their Bazel output paths."""
  if platform.system().lower() in ["linux", "darwin"]:
    return {
        "netsim": "next/cli/_stripped/netsim",
        "netsimd": "next/daemon/_stripped/daemon",
    }
  else:
    return {
        "netsim": "next/cli/netsim",
        "netsimd": "next/daemon/daemon",
    }


def platform_to_target_name(target):
  """Translates platform to target name"""
  return TARGET_MAP[target.lower().replace("-", "_")]


def is_presubmit(build_id):
  return build_id.startswith("P")


def get_host_and_ip():
  """Try to get my hostname and ip address."""
  st = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
  try:
    st.connect(("10.255.255.255", 1))
    my_ip = st.getsockname()[0]
  except Exception:
    my_ip = "127.0.0.1"
  finally:
    st.close()

  try:
    hostname = socket.gethostname()
  except Exception:
    hostname = "Unkwown"

  return hostname, my_ip


def move_contents(source, destination):
  """Moves the contents of a source directory to a destination directory."""

  if not os.path.exists(source):
    print(f"Source directory '{source}' does not exist.")
    return

  if not os.path.exists(destination):
    os.makedirs(destination)  # Create destination if it doesn't exist

  for item in os.listdir(source):
    s = os.path.join(source, item)
    d = os.path.join(destination, item)
    try:
      shutil.move(s, d)
    except Exception as e:
      print(f"Error moving '{s}' to '{d}': {e}")


class LogBelowLevel(logging.Filter):

  def __init__(self, exclusive_maximum, name=""):
    super(LogBelowLevel, self).__init__(name)
    self.max_level = exclusive_maximum

  def filter(self, record):
    return True if record.levelno < self.max_level else False


def config_logging():
  logging_handler_out = logging.StreamHandler(sys.stdout)
  logging_handler_out.setFormatter(
      TimeFormatter("%(asctime)s %(threadName)s | %(message)s")
  )
  logging_handler_out.setLevel(logging.DEBUG)
  logging_handler_out.addFilter(LogBelowLevel(logging.WARNING))

  logging_handler_err = logging.StreamHandler(sys.stderr)
  logging_handler_err.setFormatter(
      TimeFormatter("%(asctime)s %(threadName)s | %(message)s")
  )
  logging_handler_err.setLevel(logging.WARNING)

  logging.root = logging.getLogger("build")
  logging.root.setLevel(logging.INFO)
  logging.root.addHandler(logging_handler_out)
  logging.root.addHandler(logging_handler_err)

  threading.current_thread().name = "inf"


def log_system_info():
  """Log some useful system information."""
  version = "{0[0]}.{0[1]}.{0[2]}".format(sys.version_info)
  hostname, my_ip = get_host_and_ip()

  logging.info(
      "Hello from %s (%s). I'm a %s build bot",
      hostname,
      my_ip,
      platform.system(),
  )
  logging.info("My uname is: %s", platform.uname())
  logging.info(
      "I'm happy to build the emulator using Python %s (%s)",
      PYTHON_EXE,
      version,
  )


def run(
    cmd, env, log_prefix, cwd=AOSP_ROOT, throw_on_failure=True, log_output=True
):
  threading.current_thread().name = log_prefix
  cmd_env = os.environ.copy()
  cmd_env.update(env)
  is_windows = platform.system() == "Windows"

  cmd = [str(x) for x in cmd]
  # logging.info("=" * 140)
  # logging.info(json.dumps(cmd_env, sort_keys=True))
  logging.info("%s $> %s", cwd, " ".join(cmd))
  # logging.info("=" * 140)

  if log_output:
    stdout = subprocess.PIPE
    stderr = subprocess.PIPE
  else:
    stdout = subprocess.DEVNULL
    stderr = subprocess.DEVNULL

  proc = subprocess.Popen(
      cmd,
      stdout=stdout,
      stderr=stderr,
      shell=is_windows,  # Make sure windows propagates ENV vars properly.
      cwd=cwd,
      env=cmd_env,
  )

  if log_output:
    _log_proc(proc, log_prefix)
  proc.wait()
  if proc.returncode != 0 and throw_on_failure:
    raise Exception("Failed to run %s - %s" % (" ".join(cmd), proc.returncode))
  return proc.returncode


def log_to_queue(q, line):
  """Logs the output of the given process."""
  if q.full():
    q.get()

  strip = line.strip()
  logging.info(strip)
  q.put(strip)


def _reader(pipe, logfn):
  try:
    with pipe:
      for line in iter(pipe.readline, b""):
        lg = line[:-1]
        try:
          lg = lg.decode("utf-8")
        except Exception as e:
          logfn("Failed to utf-8 decode line, {}".format(e))
          lg = str(lg)
        logfn(lg.strip())
  finally:
    pass


def _log_proc(proc, log_prefix):
  """Logs the output of the given process."""
  q = Queue()
  for args in [[proc.stdout, logging.info], [proc.stderr, logging.error]]:
    t = threading.Thread(target=_reader, args=args)
    t.name = log_prefix
    t.start()

  return q


def get_bazel_path():
  """Returns the path to the bazel binary."""
  os_name = platform.system().lower()
  system = f"{os_name}-x86_64"
  bazel_binary = "bazel.exe" if os_name == "windows" else "bazel"
  return AOSP_ROOT / "prebuilts" / "bazel" / system / bazel_binary


def run_gcloud_auth(env):
  """Authenticates with gcloud for hermetic builds."""
  # This is required for hermetic builds to access GCS for dependencies.
  # Check if we already have credentials to avoid browser popup
  if (
      run(
          [
              "gcloud",
              "auth",
              "application-default",
              "print-access-token",
          ],
          env,
          "gcloud auth check",
          AOSP_ROOT,
          throw_on_failure=False,
          log_output=False,
      )
      == 0
  ):
    print("Gcloud already authenticated, skipping login")
  else:
    run(
        [
            "gcloud",
            "auth",
            "application-default",
            "login",
            "--project=emulator-builds",
        ],
        env,
        "gcloud auth",
        AOSP_ROOT,
    )
  # This is required to access the quota project for GCS dependencies.
  run(
      [
          "gcloud",
          "auth",
          "application-default",
          "set-quota-project",
          "emulator-builds",
      ],
      env,
      "gcloud auth",
      AOSP_ROOT,
  )


def get_bazel_startup_options():
  """Returns the bazel startup options."""
  startup_options = []
  tmp_dir = Path(os.environ.get("TMPDIR")) if os.environ.get("TMPDIR") else None
  if tmp_dir:
    startup_options += [
        f"--output_base={tmp_dir / 'output'}",
        f"--install_base={tmp_dir / 'install'}",
    ]
  return startup_options


def configure_android_sdk():
  """Determines and validates the Android SDK path.

  Returns:
      tuple: (sdk_path, is_prebuilt) where is_prebuilt is True if using
             in-repo prebuilts, False if using ANDROID_HOME.
  """
  # Determine OS directory for prebuilt SDK
  system = platform.system().lower()
  os_map = {"darwin": "macosx", "linux": "linux", "windows": "windows"}
  os_dir = os_map.get(system, system)

  prebuilt_sdk_path = os.path.join(
      AOSP_ROOT,
      "prebuilts",
      "android-emulator-build",
      "system-images",
      "android-sdk",
      os_dir,
  )

  # Check if prebuilt SDK is available and valid
  if os.path.isdir(prebuilt_sdk_path):
    platforms_dir = os.path.join(prebuilt_sdk_path, "platforms")
    build_tools_dir = os.path.join(prebuilt_sdk_path, "build-tools")
    if os.path.isdir(platforms_dir) and os.path.isdir(build_tools_dir):
      logging.info("Using prebuilt Android SDK at %s", prebuilt_sdk_path)
      return prebuilt_sdk_path, True

  logging.warning(
      "Prebuilt Android SDK at %s not found or invalid. Falling back to"
      " ANDROID_HOME.",
      prebuilt_sdk_path,
  )
  # Fallback to environment variable
  android_home = os.environ.get("ANDROID_HOME")
  if android_home and os.path.isdir(android_home):
    platforms_dir = os.path.join(android_home, "platforms")
    build_tools_dir = os.path.join(android_home, "build-tools")
    if os.path.isdir(platforms_dir) and os.path.isdir(build_tools_dir):
      logging.info("Android SDK detected at %s via ANDROID_HOME.", android_home)
      return android_home, False
    else:
      raise Exception(
          "ANDROID_HOME is set at %s but does not appear to be a valid SDK"
          " (missing platforms or build-tools)." % android_home
      )
  else:
    raise Exception(
        "No valid Android SDK found. Tried prebuilts at %s and ANDROID_HOME."
        % prebuilt_sdk_path
    )


def get_bazel_build_configs(args, env):
  """Returns the bazel build configurations."""
  configs = ["release"]
  if args.buildbot:
    configs.append("ci")
  elif args.hermetic:
    run_gcloud_auth(env)
    configs.append("hermetic")

  build_configs = [f"--config={c}" for c in configs]
  if platform.system().lower() == "windows":
    # Force Static CRT linking to avoid ABI mismatches with the Emulator's prebuilt DLLs.
    build_configs.append("--features=static_link_msvcrt")
  if not getattr(args, "enable_repo_cache", False):
    build_configs.append("--repo_contents_cache=")

  # Configure Android SDK and update environment if available
  try:
    sdk_path, _ = configure_android_sdk()
    env["ANDROID_HOME"] = sdk_path
  except Exception as e:
    logging.warning("Android SDK configuration skipped: %s", e)

  return build_configs


def _has_test_target(pkg_dir: Path) -> bool:
  """Checks if a package directory defines a :test target."""
  build_file = (
      pkg_dir / "BUILD"
      if (pkg_dir / "BUILD").is_file()
      else (pkg_dir / "BUILD.bazel")
  )
  if not build_file.is_file():
    return False

  try:
    tree = ast.parse(build_file.read_text(), filename=str(build_file))
    for node in ast.walk(tree):
      if isinstance(node, ast.Call):
        # netsim_rust_library implicitly creates a :test target.
        if (
            isinstance(node.func, ast.Name)
            and node.func.id == "netsim_rust_library"
        ):
          return True
        for kw in node.keywords:
          if kw.arg == "name":
            val = getattr(kw.value, "value", getattr(kw.value, "s", None))
            if val == "test":
              return True
  except Exception:
    # Fallback to regex if AST parsing fails
    try:
      for line in build_file.read_text().splitlines():
        line = line.split("#")[0].strip()
        if re.search(r"\bnetsim_rust_library\b", line) or re.search(
            r'\bname\s*=\s*["\']test["\']', line
        ):
          return True
    except Exception:
      pass
  return False


def _resolve_crate_target(crate_name: str) -> str:
  """Resolves a crate name to its bazel target, falling back to :all if :test does not exist."""
  clean_name = crate_name.replace("netsim-", "").replace("netsim_", "")
  if clean_name == "netsim":
    clean_name = "cli"
  for base_dir in ["next", ""]:
    for candidate in [clean_name, crate_name]:
      rel_path = f"{base_dir}/{candidate}" if base_dir else candidate
      pkg_dir = NETSIM_ROOT / rel_path
      if pkg_dir.is_dir():
        suffix = ":test" if _has_test_target(pkg_dir) else ":all"
        return f"//{rel_path}{suffix}"
  return f"//next/{clean_name}:all"


def get_bazel_targets(args):
  """Returns the bazel targets."""
  if args.bazel_targets:
    return args.bazel_targets

  if getattr(args, "crate", None):
    return [_resolve_crate_target(c) for c in args.crate]

  return [
      "//:all",
      "//next/...",
  ]
