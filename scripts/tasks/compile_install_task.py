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

import hashlib
import json
import logging
import os
from pathlib import Path
import platform
import shutil
import stat

from tasks.task import Task
from utils import (
    AOSP_ROOT,
    NETSIM_ROOT,
    binary_extension,
    get_bazel_build_configs,
    get_bazel_path,
    get_bazel_startup_options,
    get_bazel_targets,
    get_netsim_binaries,
    platform_to_target_name,
    run,
)


class CompileInstallTask(Task):

  def __init__(self, args, env):
    super().__init__("CompileInstall")
    self.args = args
    self.out = Path(args.out_dir)
    self.env = env
    self.binaries = get_netsim_binaries()

  def on_rm_error(self, func, path, exc_info):
    """Error handler for ``shutil.rmtree``.

    If the error is due to an access error (read only file)
    it attempts to add write permission and then retries.

    If the error is for another reason it re-raises the error.

    Usage : ``shutil.rmtree(path, onerror=on_rm_error)``
    """
    # Is the error an access error?
    if not os.access(path, os.W_OK):
      os.chmod(path, stat.S_IWRITE)
      func(path)
    else:
      raise

  def _copy_file(self, src_file: Path, dest_file: Path):
    """Copies src_file to dest_file, ensuring user-writable permissions."""
    dest_file.parent.mkdir(exist_ok=True, parents=True)
    if dest_file.is_file():
      try:
        dest_file.unlink()
      except PermissionError:
        self.on_rm_error(os.unlink, str(dest_file), None)
    shutil.copy(src_file, dest_file)
    dest_file.chmod(dest_file.stat().st_mode | stat.S_IWRITE)

  def _resolve_rootcanal_proto_src(self, search_dir: Path) -> Path:
    """Locates rootcanal/configuration.proto across AOSP and Bazel layouts."""
    rel_proto = Path("proto") / "rootcanal" / "configuration.proto"
    candidates = []

    def _add_candidates(ext_dir: Path):
      matched = (
          [d for d in ext_dir.glob("rootcanal*") if d.is_dir()]
          if ext_dir.is_dir()
          else []
      )
      if matched:
        for repo_dir in sorted(matched):
          cand = repo_dir / rel_proto
          if cand not in candidates:
            candidates.append(cand)
      else:
        for repo_name in ("rootcanal", "rootcanal+", "rootcanal~"):
          cand = ext_dir / repo_name / rel_proto
          if cand not in candidates:
            candidates.append(cand)

    # Tier 1 (Active Bazel execroot provenance)
    execroot_probes = [search_dir]
    bazel_netsim = NETSIM_ROOT / "bazel-netsim"
    if bazel_netsim.exists():
      execroot_probes.append(bazel_netsim)
    for probe in execroot_probes:
      resolved = probe.resolve()
      for parent in [resolved, *resolved.parents]:
        if parent.name == "execroot":
          ext_root = parent.parent / "external"
          _add_candidates(ext_root)
          break

    # Tier 2 (Fallback to AOSP_ROOT)
    for aosp_cand in (
        AOSP_ROOT / "tools" / "rootcanal" / rel_proto,
        AOSP_ROOT
        / "packages"
        / "modules"
        / "Bluetooth"
        / "tools"
        / "rootcanal"
        / rel_proto,
    ):
      if aosp_cand not in candidates:
        candidates.append(aosp_cand)

    # Tier 3 (Fallback to TMPDIR sandbox)
    tmp_dir = os.environ.get("TMPDIR")
    if tmp_dir:
      tmp_ext = Path(tmp_dir) / "output" / "external"
      _add_candidates(tmp_ext)

    for candidate in candidates:
      if candidate.is_file():
        return candidate
    raise FileNotFoundError(
        "rootcanal/configuration.proto not found in any expected location: "
        + ", ".join(str(c) for c in candidates)
    )

  def _copy_proto_files(self, dest_dir: Path, search_dir: Path):
    """Copies netsim and rootcanal .proto files into dest_dir/proto/."""
    proto_dest_root = dest_dir / "proto"
    if proto_dest_root.exists():
      shutil.rmtree(proto_dest_root, onerror=self.on_rm_error)

    netsim_proto_src_dir = NETSIM_ROOT / "proto" / "netsim"
    proto_files = [
        p for p in sorted(netsim_proto_src_dir.glob("*.proto")) if p.is_file()
    ]
    if not proto_files:
      raise FileNotFoundError(
          f"No .proto files found in {netsim_proto_src_dir}"
      )
    netsim_proto_dest_dir = proto_dest_root / "netsim"
    for proto_file in proto_files:
      dest_file = netsim_proto_dest_dir / proto_file.name
      logging.info("Copying %s to %s", proto_file, dest_file)
      self._copy_file(proto_file, dest_file)

    rootcanal_src = self._resolve_rootcanal_proto_src(search_dir)
    rootcanal_dest = proto_dest_root / "rootcanal" / "configuration.proto"
    logging.info("Copying %s to %s", rootcanal_src, rootcanal_dest)
    self._copy_file(rootcanal_src, rootcanal_dest)

  def _generate_installed_files_json(self):
    """Generates installed-files.json for size tracking."""
    search_dir = self.out / "distribution" / "emulator"
    if not search_dir.is_dir():
      logging.warning(
          f"Artifact directory not found: {search_dir}. Skipping JSON"
          " generation."
      )
      return

    installed_files = []
    for bin_name in self.binaries:
      actual_name = binary_extension(bin_name)
      bin_path = search_dir / actual_name
      if bin_path.is_file():
        size = bin_path.stat().st_size

        # Calculate SHA256
        sha256_hash = hashlib.sha256()
        with open(bin_path, "rb") as f:
          while byte_block := f.read(4096):
            sha256_hash.update(byte_block)
        sha256 = sha256_hash.hexdigest()

        installed_files.append(
            {"Name": bin_name, "Size": size, "SHA256": sha256}
        )

    if not installed_files:
      logging.warning("No binaries found to track for size report.")

    target_str = platform_to_target_name(self.args.target or platform.system())
    dist_dir = Path(self.args.dist_dir).absolute()
    dist_dir.mkdir(exist_ok=True, parents=True)

    json_fname = dist_dir / f"installed-files-{target_str}.json"
    logging.info("Creating installed-files JSON: %s", json_fname)
    with open(json_fname, "w", encoding="utf-8") as f:
      json.dump(installed_files, f, indent=2)

  def do_run(self):
    res = self._run_bazel()
    if res:
      self._generate_installed_files_json()
    return res

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

    # Bazel Install
    search_dir = NETSIM_ROOT / "bazel-bin"
    if not search_dir.is_dir():
      legacy_dir = AOSP_ROOT / "bazel-bin" / "external" / "netsim+"
      if legacy_dir.is_dir():
        search_dir = legacy_dir
    dest_dir = self.out / "distribution" / "emulator"
    logging.info(f"Installing artifacts from {search_dir} to {dest_dir}")

    try:
      if not search_dir.is_dir():
        logging.error(f"Bazel output directory not found: {search_dir}")
        return False

      dest_dir.mkdir(exist_ok=True, parents=True)

      # Copy netsim binaries
      for binary, src in self.binaries.items():
        binary_name = binary_extension(binary)
        src_name = binary_extension(src)

        src_file = search_dir / src_name
        logging.info("Copying %s to %s", src_file, dest_dir)
        dest_file = dest_dir / binary_name
        self._copy_file(src_file, dest_file)

      self._copy_proto_files(dest_dir, search_dir)

    except FileNotFoundError as e:
      logging.error(
          f"Artifact not found: {e}. A successful Bazel build is required."
      )
      raise e
    except shutil.Error as e:
      logging.error(f"Error copying artifacts: {e}")
      raise e
    return True
