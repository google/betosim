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
import zipfile

from tasks.task import Task
from utils import NETSIM_ROOT, platform_to_target_name


class ZipArtifactTask(Task):

  def __init__(self, args):
    super().__init__("ZipArtifact")
    self.build_id = args.build_id
    self.out = Path(args.out_dir)
    self.target = args.target or platform.system()
    self.dist = Path(args.dist_dir).absolute()

  def _verify_proto_files(self, search_dir: Path):
    """Verifies staged proto/ files match netsim and rootcanal protos."""
    expected_netsim_protos = [
        p for p in sorted((NETSIM_ROOT / "proto" / "netsim").glob("*.proto"))
        if p.is_file()
    ]
    if not expected_netsim_protos:
      raise FileNotFoundError(
          f"No source .proto files found in {NETSIM_ROOT / 'proto' / 'netsim'}"
      )
    expected_rel = {
        f"proto/netsim/{p.name}" for p in expected_netsim_protos
    } | {"proto/rootcanal/configuration.proto"}

    proto_root = search_dir / "proto"
    actual_rel = (
        {
            p.relative_to(search_dir).as_posix()
            for p in proto_root.rglob("*")
            if p.is_file() or p.is_symlink()
        }
        if proto_root.is_dir()
        else set()
    )

    missing = sorted(expected_rel - actual_rel)
    if missing:
      raise FileNotFoundError(
          "Missing required .proto files in release distribution: "
          + ", ".join(missing)
      )
    unexpected = sorted(actual_rel - expected_rel)
    if unexpected:
      raise ValueError(
          "Unexpected files under proto/ in release distribution: "
          + ", ".join(unexpected)
      )

  def do_run(self):
    # Make sure the dist directory exists.
    self.dist.mkdir(exist_ok=True, parents=True)

    # Zip results..
    zip_fname = (
        self.dist
        / f"netsim-{platform_to_target_name(self.target)}-{self.build_id}.zip"
    )
    search_dir = self.out / "distribution" / "emulator"
    if not search_dir.is_dir():
      logging.warning(
          f"Artifact directory not found: {search_dir}. Skipping zip."
      )
      return True
    self._verify_proto_files(search_dir)
    search_glob = sorted(search_dir.glob("**/*"))

    logging.info("Creating zip file: %s", zip_fname)
    with zipfile.ZipFile(
        zip_fname, "w", zipfile.ZIP_DEFLATED, allowZip64=True
    ) as zipf:
      logging.info("Searching %s", search_dir)
      for fname in search_glob:
        if not fname.is_file():
          continue
        arcname = fname.relative_to(search_dir).as_posix()
        logging.info("Adding %s as %s", fname, arcname)
        zipf.write(fname, arcname)
    return True
