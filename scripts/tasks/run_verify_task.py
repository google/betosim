#!/usr/bin/env python3
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

from contextlib import ExitStack
import gzip
import logging
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import textwrap
import time
from tasks.task import Task
from utils import AOSP_ROOT, EMULATOR_ARTIFACT_PATH, binary_extension, configure_android_sdk, run


class RunVerifyTask(Task):
  """Task to run verify integration tests with emulators."""

  def __init__(self, args, env) -> None:
    super().__init__("RunVerify")
    self.args = args
    self.env = env
    self.out = Path(args.out_dir)

  def do_run(self) -> bool:
    # TODO(b/557273666): RunVerify is temporarily disabled due to the removal
    # of guest Android instrumentation APKs during the standalone Bazel
    # workspace migration. Re-enable or restore verification once guest test
    # artifacts are decoupled or integrated into the new standalone test runner.
    logging.info("RunVerify task is disabled.")
    return True


# NOTE(b/557273666): Inactive / dormant class.
# VerifyManager is currently unused while RunVerifyTask.do_run() is disabled.
# Retained for reactivation when guest test orchestration is reinstated.
class VerifyManager:
  """[DORMANT] Handles the heavy lifting of orchestration for verification.

  This class orchestrates the end-to-end verification process by performing
  the following steps:
  1. Setup: Creates output directories and resolves the Android SDK path.
  2. SDK Verification: Checks for required binaries (avdmanager, sdkmanager,
  adb, emulator).
  3. Image Acquisition: Uses sdkmanager to download missing system images if
  needed.
  4. AVD Creation: Uses avdmanager to create AVDs for the specified
  configurations.
  5. Netsimd Startup: Launches the network simulation daemon.
  6. Emulator Startup: Launches the emulators in the background.
  7. Device Mapping: Waits for devices to appear in ADB and maps them to AVD
  names.
  8. Boot Verification: Waits for sys.boot_completed on all devices.
  9. Network Verification: Waits for the network stack to be ready (ping
  10.0.2.2).
  10. WiFi Connection: Connects devices to the simulated AndroidWifi.
  11. IP Acquisition: Waits for an IP address on wlan0.
  12. Logcat Streaming: Starts streaming logcats for all devices to files.
  13. Test Execution: Runs the verify runner with the specified APK and feature
  files.
  14. Cleanup: Terminates processes and deletes created AVDs.
  """

  BOOT_TIMEOUT_CYCLES = 60
  BOOT_SLEEP_SECONDS = 3
  NETWORK_TIMEOUT_CYCLES = 30
  NETWORK_SLEEP_SECONDS = 2
  IP_TIMEOUT_CYCLES = 15
  IP_SLEEP_SECONDS = 2

  def __init__(self, args, env) -> None:
    self.args = args
    self.env = env
    self.out = Path(args.out_dir)
    self.avd_home = self.out / "avd"
    self.sdk_root = None
    self.emulator_bin = None
    self.emulator_dir = None
    self.is_prebuilt = False
    self.avds = []
    self.avd_configs = []
    self.emu_procs = []
    self.netsimd_proc = None

    self.avd_to_serial = {}
    self.logcat_procs = []

  def _log_subprocess_error(
      self,
      message: str,
      e: subprocess.CalledProcessError,
      is_error: bool = False,
  ) -> None:
    log_func = logging.error if is_error else logging.warning
    log_func(f"{message}: {e}")
    if e.stdout:
      log_func(f"Stdout: {e.stdout.decode('utf-8')}")
    if e.stderr:
      log_func(f"Stderr: {e.stderr.decode('utf-8')}")

  def process(self) -> bool:
    configs = [
        {
            "api": "36",
            "tag.id": "google_apis",
            "abi": (
                "x86_64"
                if platform.machine() in ["x86_64", "AMD64"]
                else "arm64-v8a"
            ),
            "AvdId": f"VERIFY-CI-AVD-{i}",
        }
        for i in range(3)
    ]

    try:
      with ExitStack() as stack:
        if not self.setup():
          return False
        if not self.create_avds(configs):
          return False
        if not self.start_netsimd(stack):
          return False
        if not self.start_emulators(stack):
          return False
        if not self.wait_for_devices(len(configs)):
          return False
        if not self.wait_for_boot_and_network():
          return False
        self.start_logcat_streaming(stack)
        if not self.run_tests():
          return False
    finally:
      self.cleanup()

    return True

  def setup(self) -> bool:
    """Setup directories and configure SDK."""
    self.out.mkdir(parents=True, exist_ok=True)
    self.avd_home.mkdir(parents=True, exist_ok=True)
    sdk_path, self.is_prebuilt = configure_android_sdk()
    self.sdk_root = Path(sdk_path)

    if self.is_prebuilt:
      self.sdk_root = (
          AOSP_ROOT
          / "prebuilts"
          / "android-emulator-build"
          / "system-images"
          / platform.system().lower()
      )
      self.env = os.environ.copy()
      # Clean self.env of any OS-inherited ANDROID_* variables
      for key in list(self.env.keys()):
        if key.startswith("ANDROID_"):
          del self.env[key]

      # Set our own
      self.env["ANDROID_SDK_ROOT"] = str(self.sdk_root)
      self.env["ANDROID_HOME"] = str(self.sdk_root)
      self.env["ANDROID_AVD_HOME"] = str(self.avd_home)

      if self.args.buildbot:
        self.emulator_dir = EMULATOR_ARTIFACT_PATH / "emulator"
      else:
        self.emulator_dir = self.out / "distribution" / "emulator"
      self.emulator_bin = self.emulator_dir / binary_extension("emulator")
    else:
      self.emulator_bin = (
          self.sdk_root / "emulator" / binary_extension("emulator")
      )

    self.avdmanager_bin = (
        self.sdk_root / "cmdline-tools" / "latest" / "bin" / "avdmanager"
    )
    self.sdkmanager_bin = (
        self.sdk_root / "cmdline-tools" / "latest" / "bin" / "sdkmanager"
    )
    self.adb_bin = self.sdk_root / "platform-tools" / "adb"

    if not self.emulator_bin.exists():
      logging.error(f"emulator binary not found at {self.emulator_bin}")
      return False

    self.runner_bin = (
        AOSP_ROOT
        / "bazel-bin/external/netsim+/next/verify/runner"
        / binary_extension("runner")
    )
    self.apk_path = (
        AOSP_ROOT
        / "bazel-bin/external/netsim+/next/verify/instrumentation/vbs/vbs.apk"
    )
    self.spec_dir = (
        AOSP_ROOT / "tools/netsim/next/verify/examples/01-basic/features"
    )
    if not self.avdmanager_bin.exists():
      logging.error(f"avdmanager not found at {self.avdmanager_bin}")
      return False
    if not self.adb_bin.exists():
      logging.error(f"adb not found at {self.adb_bin}")
      return False
    if not self.sdkmanager_bin.exists():
      logging.error(f"sdkmanager not found at {self.sdkmanager_bin}")
      return False

    return True

  def create_avds(self, configs: list[dict]) -> bool:
    """Create AVDs based on configs."""
    logging.info("Cleaning up old AVDs if they exist...")
    for cfg in configs:
      avd_name = cfg["AvdId"]
      try:
        logging.info(f"Checking and deleting old AVD {avd_name}...")
        subprocess.run(
            [self.avdmanager_bin, "delete", "avd", "--name", avd_name],
            capture_output=True,
            env=self.env,
            # Ignore failure if it doesn't exist
            check=False,
        )
      except Exception as e:
        logging.warning(f"Failed to delete old AVD {avd_name}: {e}")

    logging.info("Creating AVDs...")
    for cfg in configs:
      name = cfg["AvdId"]
      package = (
          f"system-images;android-{cfg['api']};{cfg['tag.id']};{cfg['abi']}"
      )

      if self.is_prebuilt:
        self._ensure_system_images(cfg)

      logging.info(f"Creating AVD {name} with package {package}...")

      try:
        subprocess.run(
            [self.avdmanager_bin, "create", "avd", "-n", name, "-k", package],
            # no to custom hardware prompt
            input=b"no\n",
            capture_output=True,
            check=True,
            env=self.env,
        )
      except subprocess.CalledProcessError as e:
        self._log_subprocess_error(
            f"Failed to create AVD {name} with avdmanager", e, is_error=True
        )
        return False
      self.avds.append(name)
    return True

  def _ensure_system_images(self, cfg: dict) -> None:
    """Download system images if missing."""
    image_dir = (
        self.sdk_root
        / "system-images"
        / f"android-{cfg['api']}"
        / cfg["tag.id"]
        / cfg["abi"]
    )
    if not image_dir.exists():
      package = (
          f"system-images;android-{cfg['api']};{cfg['tag.id']};{cfg['abi']}"
      )
      logging.info(f"System image missing. Downloading {package}...")
      try:
        subprocess.run(
            [self.sdkmanager_bin, package],
            # accept licenses
            input=b"y\n" * 10,
            capture_output=True,
            check=True,
            env=self.env,
        )
      except subprocess.CalledProcessError as e:
        self._log_subprocess_error(f"Failed to download package {package}", e)

  def start_netsimd(self, stack: ExitStack) -> bool:
    """Start netsimd process."""
    netsimd_bin = (
        AOSP_ROOT
        / "bazel-bin/external/netsim+/next/daemon"
        / binary_extension("daemon")
    )
    if not netsimd_bin.exists():
      logging.error(f"netsimd binary not found at {netsimd_bin}")
      return False
    netsimd_log_path = self.out / "netsimd_e2e.log"
    logging.info(f"Logging netsimd to {netsimd_log_path}...")
    netsimd_log = stack.enter_context(open(netsimd_log_path, "w"))
    self.netsimd_proc = subprocess.Popen(
        [netsimd_bin, "--no-shutdown", "-v", "--logtostderr"],
        stderr=netsimd_log,
        stdout=netsimd_log,
        env=self.env,
    )
    return True

  def start_emulators(self, stack: ExitStack) -> bool:
    """Start emulator processes."""
    logging.info("Starting emulators...")
    for avd_name in self.avds:
      emu_log_path = self.out / f"emulator_{avd_name}_e2e.log"
      logging.info(f"Logging emulator {avd_name} to {emu_log_path}...")
      emu_log = stack.enter_context(open(emu_log_path, "w"))
      proc = subprocess.Popen(
          [
              self.emulator_bin,
              f"@{avd_name}",
              "-no-window",
              "-no-audio",
              "-no-snapshot",
              "-wipe-data",
              "-verbose",
          ],
          stderr=emu_log,
          stdout=emu_log,
          env=self.env,
      )
      self.emu_procs.append(proc)
    return True

  def start_logcat_streaming(self, stack: ExitStack) -> None:
    """Start logcat streaming for all devices."""
    logging.info("Starting logcat streaming...")
    for avd_name, dev in self.avd_to_serial.items():
      logcat_path = self.out / f"logcat_{avd_name}_{dev}.log"
      logging.info(
          f"Streaming logcat for {avd_name} ({dev}) to {logcat_path}..."
      )
      logcat_file = stack.enter_context(open(logcat_path, "w"))
      proc = subprocess.Popen(
          [self.adb_bin, "-s", dev, "logcat"],
          stderr=logcat_file,
          stdout=logcat_file,
          env=self.env,
      )
      self.logcat_procs.append(proc)

  def wait_for_devices(self, num_emulators: int) -> bool:
    """Wait for devices to appear in adb and map them."""
    logging.info("Waiting for devices to appear in adb...")
    while True:
      output = subprocess.check_output(
          [self.adb_bin, "devices"], encoding="utf-8", env=self.env
      )
      connected_ids = [
          line.split()[0]
          for line in output.splitlines()
          if "device" in line and not line.startswith("List")
      ]

      for dev in connected_ids:
        if dev not in self.avd_to_serial.values():
          try:
            avd_name = subprocess.check_output(
                [
                    self.adb_bin,
                    "-s",
                    dev,
                    "shell",
                    "getprop",
                    "ro.boot.qemu.avd_name",
                ],
                encoding="utf-8",
                env=self.env,
            ).strip()
            if avd_name in self.avds:
              self.avd_to_serial[avd_name] = dev
              logging.info(f"Mapped {avd_name} to {dev}")
          except subprocess.CalledProcessError:
            pass

      if len(self.avd_to_serial) >= num_emulators:
        break
      time.sleep(5)
    return True

  def wait_for_boot_and_network(self) -> bool:
    """Wait for boot completed and network setup on all devices."""
    for avd_name in self.avds:
      dev = self.avd_to_serial.get(avd_name)
      if not dev:
        logging.error(f"No ADB serial found for AVD {avd_name}!")
        return False

      if not self._wait_for_boot(dev):
        return False
      if not self._wait_for_network(dev):
        return False
      self._connect_to_wifi(dev)
      if not self._wait_for_ip(dev):
        return False
    return True

  def _wait_for_boot(self, dev: str) -> bool:
    logging.info(f"Waiting for {dev} boot...")
    wait_cycles = 0
    while True:
      try:
        boot_completed = subprocess.check_output(
            [self.adb_bin, "-s", dev, "shell", "getprop", "sys.boot_completed"],
            encoding="utf-8",
            env=self.env,
        ).strip()
        if boot_completed == "1":
          return True
      except subprocess.CalledProcessError:
        pass
      wait_cycles += 1
      if wait_cycles > self.BOOT_TIMEOUT_CYCLES:
        logging.error(f"{dev} failed to boot within timeout!")
        return False
      time.sleep(self.BOOT_SLEEP_SECONDS)

  def _wait_for_network(self, dev: str) -> bool:
    logging.info(f"Waiting for {dev} network stack (ping 10.0.2.2)...")
    wait_cycles = 0
    result = None
    last_exception = None
    while True:
      try:
        result = subprocess.run(
            [
                self.adb_bin,
                "-s",
                dev,
                "shell",
                "ping",
                "-c",
                "1",
                "-W",
                "1",
                "10.0.2.2",
            ],
            capture_output=True,
            text=True,
            env=self.env,
        )
        if result.returncode == 0:
          return True
      except Exception as e:
        last_exception = e
      wait_cycles += 1
      if wait_cycles > self.NETWORK_TIMEOUT_CYCLES:
        logging.error(f"{dev} network stack failed to come up!")
        if result:
          logging.error(f"Last ping stdout: {result.stdout}")
          logging.error(f"Last ping stderr: {result.stderr}")
        if last_exception:
          logging.error(
              f"Last ping attempt failed with exception: {last_exception}"
          )
        return False
      time.sleep(self.NETWORK_SLEEP_SECONDS)

  def _connect_to_wifi(self, dev: str) -> None:
    logging.info(f"Connecting {dev} to AndroidWifi...")
    logging.info(f"Switching adb to root for {dev}...")
    subprocess.run(
        [self.adb_bin, "-s", dev, "root"], capture_output=True, env=self.env
    )
    subprocess.run(
        [self.adb_bin, "-s", dev, "wait-for-device"],
        capture_output=True,
        env=self.env,
    )
    result = subprocess.run(
        [
            self.adb_bin,
            "-s",
            dev,
            "shell",
            "cmd",
            "wifi",
            "connect-network",
            "AndroidWifi",
            "open",
        ],
        capture_output=True,
        text=True,
        env=self.env,
    )
    if result.returncode != 0:
      logging.warning(f"Failed to connect {dev} to AndroidWifi")
      logging.warning(f"stdout: {result.stdout}")
      logging.warning(f"stderr: {result.stderr}")

  def _wait_for_ip(self, dev: str) -> bool:
    logging.info(f"Waiting for {dev} IP address on wlan0...")
    wait_cycles = 0
    while True:
      try:
        output = subprocess.check_output(
            [self.adb_bin, "-s", dev, "shell", "ip", "addr", "show", "wlan0"],
            encoding="utf-8",
            env=self.env,
        )
        if "inet " in output:
          return True
      except subprocess.CalledProcessError:
        pass
      wait_cycles += 1
      if wait_cycles > self.IP_TIMEOUT_CYCLES:
        logging.error(f"{dev} wlan0 failed to associate or pull an IP!")
        return False
      time.sleep(self.IP_SLEEP_SECONDS)

  def run_tests(self) -> bool:
    """Run the verify runner."""
    if not self.runner_bin.exists():
      logging.error(f"runner binary not found at {self.runner_bin}")
      return False
    if not self.apk_path.exists():
      logging.error(f"vbs.apk not found at {self.apk_path}")
      return False

    netsim_cli_bin = (
        AOSP_ROOT
        / "bazel-bin/external/netsim+/next/cli"
        / binary_extension("netsim")
    )
    if not netsim_cli_bin.exists():
      logging.error(f"netsim CLI binary not found at {netsim_cli_bin}")
      return False

    logging.info("Running verify tests...")
    run(
        [
            self.runner_bin,
            "run",
            "--android-home",
            self.sdk_root,
            "--apk-path",
            self.apk_path,
            "--netsim-cli-path",
            netsim_cli_bin,
            "--spec-dir",
            self.spec_dir,
            "--keep-going",
        ],
        self.env,
        "verify_runner",
    )
    return True

  def cleanup(self) -> None:
    """Cleanup processes and AVDs."""
    logging.info("Cleaning up...")
    for proc in self.logcat_procs:
      try:
        proc.terminate()
        proc.wait(timeout=2)
      except subprocess.TimeoutExpired:
        logging.warning("Logcat process did not terminate in time, killing...")
        proc.kill()
        proc.wait()
      except ProcessLookupError:
        pass
      except OSError as e:
        logging.warning(f"Failed to terminate logcat process: {e}")

    for proc in self.emu_procs:
      try:
        proc.terminate()
        proc.wait(timeout=5)
      except subprocess.TimeoutExpired:
        logging.warning(
            "Emulator process did not terminate in time, killing..."
        )
        proc.kill()
        proc.wait()
      except ProcessLookupError:
        pass
      except OSError as e:
        logging.warning(f"Failed to terminate emulator process: {e}")

    if self.netsimd_proc:
      try:
        self.netsimd_proc.terminate()
        self.netsimd_proc.wait(timeout=2)
      except subprocess.TimeoutExpired:
        logging.warning("netsimd process did not terminate in time, killing...")
        self.netsimd_proc.kill()
        self.netsimd_proc.wait()
      except ProcessLookupError:
        pass
      except OSError as e:
        logging.warning(f"Failed to terminate netsimd process: {e}")

    for avd_name in self.avds:
      try:
        logging.info(f"Deleting AVD {avd_name}...")
        subprocess.run(
            [self.avdmanager_bin, "delete", "avd", "--name", avd_name],
            capture_output=True,
            check=True,
            env=self.env,
        )
      except subprocess.CalledProcessError as e:
        self._log_subprocess_error(f"Failed to delete AVD {avd_name}", e)
      except Exception as e:
        logging.warning(f"Failed to delete AVD {avd_name}: {e}")
