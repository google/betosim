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

"""Unit tests for CompileInstallTask and ZipArtifactTask."""

import json
import os
from pathlib import Path
import shutil
import stat
import tempfile
import unittest
from unittest import mock
import zipfile

from tasks import compile_install_task
from tasks import zip_artifact_task
from tasks.compile_install_task import CompileInstallTask
from tasks.zip_artifact_task import ZipArtifactTask
import utils


class _TestArgs:

  def __init__(
      self,
      out_dir: Path,
      dist_dir: Path,
      build_id: str = "12345",
      target: str = "linux_x86_64",
  ):
    self.out_dir = str(out_dir)
    self.dist_dir = str(dist_dir)
    self.build_id = build_id
    self.target = target
    self.buildbot = False
    self.hermetic = False
    self.bazel_targets = None
    self.crate = None


class CompileInstallTaskTest(unittest.TestCase):

  def test_compile_install_do_run_copies_binaries_protos_and_size_report(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      sandbox_tmp.mkdir(parents=True, exist_ok=True)
      sandbox_netsim = sandbox_root / "netsim"
      sandbox_aosp = sandbox_root / "aosp"
      out_dir = sandbox_root / "out"
      dist_dir = sandbox_root / "dist"
      dest_dir = out_dir / "distribution" / "emulator"

      stale_proto = dest_dir / "proto" / "netsim" / "obsolete.proto"
      stale_proto.parent.mkdir(parents=True, exist_ok=True)
      stale_proto.write_text("// obsolete")
      stale_proto.chmod(stat.S_IREAD)

      stale_bin = dest_dir / compile_install_task.binary_extension("netsim")
      stale_bin.parent.mkdir(parents=True, exist_ok=True)
      stale_bin.write_bytes(b"stale binary")
      stale_bin.chmod(stat.S_IREAD)

      args = _TestArgs(out_dir, dist_dir)
      task = CompileInstallTask(args, {})

      bazel_bin = sandbox_netsim / "bazel-bin"
      for binary, src in task.binaries.items():
        src_file = bazel_bin / compile_install_task.binary_extension(src)
        src_file.parent.mkdir(parents=True, exist_ok=True)
        src_file.write_text(f"dummy {binary}")

      proto_dir = sandbox_netsim / "proto" / "netsim"
      proto_dir.mkdir(parents=True, exist_ok=True)
      p_common = proto_dir / "common.proto"
      p_common.write_text("// common")
      p_common.chmod(0o444)
      p_model = proto_dir / "model.proto"
      p_model.write_text("// model")

      rc_proto = (
          sandbox_aosp
          / "tools"
          / "rootcanal"
          / "proto"
          / "rootcanal"
          / "configuration.proto"
      )
      rc_proto.parent.mkdir(parents=True, exist_ok=True)
      rc_proto.write_text("// rootcanal config")
      rc_proto.chmod(0o444)

      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(
              compile_install_task, "NETSIM_ROOT", sandbox_netsim
          ),
          mock.patch.object(compile_install_task, "AOSP_ROOT", sandbox_aosp),
          mock.patch.object(compile_install_task, "run", return_value=0),
          mock.patch.object(
              compile_install_task,
              "get_bazel_path",
              return_value=Path("/bin/bazel"),
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_build_configs", return_value=[]
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_startup_options", return_value=[]
          ),
          mock.patch.object(
              compile_install_task,
              "get_bazel_targets",
              return_value=["//:all"],
          ),
      ):
        res = task.do_run()

      self.assertTrue(res)
      self.assertFalse(stale_proto.exists())
      self.assertEqual(stale_bin.read_bytes(), b"dummy netsim")
      self.assertTrue(bool(stale_bin.stat().st_mode & stat.S_IWRITE))

      dest_common = dest_dir / "proto" / "netsim" / "common.proto"
      self.assertTrue(dest_common.is_file())
      self.assertTrue(bool(dest_common.stat().st_mode & stat.S_IWRITE))

      dest_model = dest_dir / "proto" / "netsim" / "model.proto"
      self.assertTrue(dest_model.is_file())

      dest_rc = dest_dir / "proto" / "rootcanal" / "configuration.proto"
      self.assertTrue(dest_rc.is_file())
      self.assertTrue(bool(dest_rc.stat().st_mode & stat.S_IWRITE))

      for binary in task.binaries:
        dest_bin = dest_dir / compile_install_task.binary_extension(binary)
        self.assertTrue(dest_bin.is_file())
        self.assertTrue(bool(dest_bin.stat().st_mode & stat.S_IWRITE))

      json_fname = dist_dir / "installed-files-linux-x86_64.json"
      self.assertTrue(json_fname.is_file())
      data = json.loads(json_fname.read_text(encoding="utf-8"))
      names = [item["Name"] for item in data]
      self.assertEqual(sorted(names), ["netsim", "netsimd"])

  def test_compile_install_resolves_rootcanal_from_bazel_execroot_over_tmpdir(
      self,
  ):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      stale_rc = (
          sandbox_tmp
          / "output"
          / "external"
          / "rootcanal+"
          / "proto"
          / "rootcanal"
          / "configuration.proto"
      )
      stale_rc.parent.mkdir(parents=True, exist_ok=True)
      stale_rc.write_text("// stale tmpdir rootcanal")

      output_base = sandbox_root / "output_base"
      execroot = output_base / "execroot"
      ws_main = execroot / "_main"
      sandbox_netsim = ws_main / "netsim"
      bazel_bin = sandbox_netsim / "bazel-bin"
      bazel_bin.mkdir(parents=True, exist_ok=True)

      args = _TestArgs(sandbox_root / "out", sandbox_root / "dist")
      task = CompileInstallTask(args, {})

      for binary, src in task.binaries.items():
        src_file = bazel_bin / compile_install_task.binary_extension(src)
        src_file.parent.mkdir(parents=True, exist_ok=True)
        src_file.write_text(f"dummy {binary}")

      proto_dir = sandbox_netsim / "proto" / "netsim"
      proto_dir.mkdir(parents=True, exist_ok=True)
      (proto_dir / "common.proto").write_text("// common")

      rc_proto = (
          output_base
          / "external"
          / "rootcanal+"
          / "proto"
          / "rootcanal"
          / "configuration.proto"
      )
      rc_proto.parent.mkdir(parents=True, exist_ok=True)
      rc_proto.write_text("// execroot rootcanal")

      sandbox_aosp = sandbox_root / "aosp"
      aosp_rc = (
          sandbox_aosp
          / "tools"
          / "rootcanal"
          / "proto"
          / "rootcanal"
          / "configuration.proto"
      )
      aosp_rc.parent.mkdir(parents=True, exist_ok=True)
      aosp_rc.write_text("// aosp rootcanal")

      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(
              compile_install_task, "NETSIM_ROOT", sandbox_netsim
          ),
          mock.patch.object(compile_install_task, "AOSP_ROOT", sandbox_aosp),
          mock.patch.object(compile_install_task, "run", return_value=0),
          mock.patch.object(
              compile_install_task,
              "get_bazel_path",
              return_value=Path("/bin/bazel"),
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_build_configs", return_value=[]
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_startup_options", return_value=[]
          ),
          mock.patch.object(
              compile_install_task,
              "get_bazel_targets",
              return_value=["//:all"],
          ),
      ):
        res = task.do_run()

      self.assertTrue(res)
      dest_rc = (
          Path(args.out_dir)
          / "distribution"
          / "emulator"
          / "proto"
          / "rootcanal"
          / "configuration.proto"
      )
      self.assertTrue(dest_rc.is_file())
      self.assertEqual(dest_rc.read_text(), "// execroot rootcanal")

  def test_compile_install_resolves_rootcanal_from_tmpdir_tilde_repo(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      rc_tilde = (
          sandbox_tmp
          / "output"
          / "external"
          / "rootcanal~"
          / "proto"
          / "rootcanal"
          / "configuration.proto"
      )
      rc_tilde.parent.mkdir(parents=True, exist_ok=True)
      rc_tilde.write_text("// tmpdir rootcanal~")

      sandbox_netsim = sandbox_root / "netsim"
      bazel_bin = sandbox_netsim / "bazel-bin"
      bazel_bin.mkdir(parents=True, exist_ok=True)

      args = _TestArgs(sandbox_root / "out", sandbox_root / "dist")
      task = CompileInstallTask(args, {})

      for binary, src in task.binaries.items():
        src_file = bazel_bin / compile_install_task.binary_extension(src)
        src_file.parent.mkdir(parents=True, exist_ok=True)
        src_file.write_text(f"dummy {binary}")

      proto_dir = sandbox_netsim / "proto" / "netsim"
      proto_dir.mkdir(parents=True, exist_ok=True)
      (proto_dir / "common.proto").write_text("// common")

      empty_aosp = sandbox_root / "empty_aosp"
      empty_aosp.mkdir(parents=True, exist_ok=True)

      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(
              compile_install_task, "NETSIM_ROOT", sandbox_netsim
          ),
          mock.patch.object(compile_install_task, "AOSP_ROOT", empty_aosp),
          mock.patch.object(compile_install_task, "run", return_value=0),
          mock.patch.object(
              compile_install_task,
              "get_bazel_path",
              return_value=Path("/bin/bazel"),
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_build_configs", return_value=[]
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_startup_options", return_value=[]
          ),
          mock.patch.object(
              compile_install_task,
              "get_bazel_targets",
              return_value=["//:all"],
          ),
      ):
        res = task.do_run()

      self.assertTrue(res)
      dest_rc = (
          Path(args.out_dir)
          / "distribution"
          / "emulator"
          / "proto"
          / "rootcanal"
          / "configuration.proto"
      )
      self.assertTrue(dest_rc.is_file())
      self.assertEqual(dest_rc.read_text(), "// tmpdir rootcanal~")

  def test_compile_install_raises_when_rootcanal_proto_missing(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      sandbox_tmp.mkdir(parents=True, exist_ok=True)
      sandbox_netsim = sandbox_root / "netsim"
      sandbox_aosp = sandbox_root / "aosp"
      out_dir = sandbox_root / "out"
      dist_dir = sandbox_root / "dist"

      args = _TestArgs(out_dir, dist_dir)
      task = CompileInstallTask(args, {})

      bazel_bin = sandbox_netsim / "bazel-bin"
      for binary, src in task.binaries.items():
        src_file = bazel_bin / compile_install_task.binary_extension(src)
        src_file.parent.mkdir(parents=True, exist_ok=True)
        src_file.write_text(f"dummy {binary}")

      proto_dir = sandbox_netsim / "proto" / "netsim"
      proto_dir.mkdir(parents=True, exist_ok=True)
      (proto_dir / "common.proto").write_text("// common")

      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(
              compile_install_task, "NETSIM_ROOT", sandbox_netsim
          ),
          mock.patch.object(compile_install_task, "AOSP_ROOT", sandbox_aosp),
          mock.patch.object(compile_install_task, "run", return_value=0),
          mock.patch.object(
              compile_install_task,
              "get_bazel_path",
              return_value=Path("/bin/bazel"),
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_build_configs", return_value=[]
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_startup_options", return_value=[]
          ),
          mock.patch.object(
              compile_install_task,
              "get_bazel_targets",
              return_value=["//:all"],
          ),
      ):
        with self.assertRaises(FileNotFoundError) as ctx:
          task.do_run()
        self.assertIn("rootcanal/configuration.proto", str(ctx.exception))

      self.assertFalse(
          (dist_dir / "installed-files-linux-x86_64.json").exists()
      )

  def test_compile_install_raises_when_source_netsim_protos_empty(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      sandbox_tmp.mkdir(parents=True, exist_ok=True)
      sandbox_netsim = sandbox_root / "netsim"
      sandbox_aosp = sandbox_root / "aosp"
      out_dir = sandbox_root / "out"
      dist_dir = sandbox_root / "dist"

      args = _TestArgs(out_dir, dist_dir)
      task = CompileInstallTask(args, {})

      bazel_bin = sandbox_netsim / "bazel-bin"
      for binary, src in task.binaries.items():
        src_file = bazel_bin / compile_install_task.binary_extension(src)
        src_file.parent.mkdir(parents=True, exist_ok=True)
        src_file.write_text(f"dummy {binary}")

      proto_dir = sandbox_netsim / "proto" / "netsim"
      proto_dir.mkdir(parents=True, exist_ok=True)

      rc_proto = (
          sandbox_aosp
          / "tools"
          / "rootcanal"
          / "proto"
          / "rootcanal"
          / "configuration.proto"
      )
      rc_proto.parent.mkdir(parents=True, exist_ok=True)
      rc_proto.write_text("// rootcanal config")

      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(
              compile_install_task, "NETSIM_ROOT", sandbox_netsim
          ),
          mock.patch.object(compile_install_task, "AOSP_ROOT", sandbox_aosp),
          mock.patch.object(compile_install_task, "run", return_value=0),
          mock.patch.object(
              compile_install_task,
              "get_bazel_path",
              return_value=Path("/bin/bazel"),
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_build_configs", return_value=[]
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_startup_options", return_value=[]
          ),
          mock.patch.object(
              compile_install_task,
              "get_bazel_targets",
              return_value=["//:all"],
          ),
      ):
        with self.assertRaises(FileNotFoundError) as ctx:
          task.do_run()
        self.assertIn("No .proto files found in", str(ctx.exception))

      self.assertFalse(
          (dist_dir / "installed-files-linux-x86_64.json").exists()
      )

  def test_real_repo_proto_sources_stage_and_zip_end_to_end(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      sandbox_tmp.mkdir(parents=True, exist_ok=True)
      sandbox_netsim = sandbox_root / "netsim"
      sandbox_netsim_proto = sandbox_netsim / "proto" / "netsim"
      sandbox_netsim_proto.mkdir(parents=True, exist_ok=True)

      real_proto_dir = utils.NETSIM_ROOT / "proto" / "netsim"
      self.assertTrue(real_proto_dir.is_dir())
      expected_protos = [
          p for p in sorted(real_proto_dir.glob("*.proto")) if p.is_file()
      ]
      self.assertTrue(bool(expected_protos))
      for p in expected_protos:
        shutil.copy(p, sandbox_netsim_proto / p.name)

      sandbox_aosp = sandbox_root / "aosp"
      rc_proto = (
          sandbox_aosp
          / "tools"
          / "rootcanal"
          / "proto"
          / "rootcanal"
          / "configuration.proto"
      )
      rc_proto.parent.mkdir(parents=True, exist_ok=True)
      rc_proto.write_text("// dummy rootcanal configuration proto")

      out_dir = sandbox_root / "out"
      dist_dir = sandbox_root / "dist"
      args = _TestArgs(
          out_dir, dist_dir, build_id="99999", target="linux_x86_64"
      )
      compile_task = CompileInstallTask(args, {})

      bazel_bin = sandbox_netsim / "bazel-bin"
      for binary, src in compile_task.binaries.items():
        src_file = bazel_bin / compile_install_task.binary_extension(src)
        src_file.parent.mkdir(parents=True, exist_ok=True)
        src_file.write_text(f"dummy {binary}")

      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(
              compile_install_task, "NETSIM_ROOT", sandbox_netsim
          ),
          mock.patch.object(compile_install_task, "AOSP_ROOT", sandbox_aosp),
          mock.patch.object(compile_install_task, "run", return_value=0),
          mock.patch.object(
              compile_install_task,
              "get_bazel_path",
              return_value=Path("/bin/bazel"),
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_build_configs", return_value=[]
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_startup_options", return_value=[]
          ),
          mock.patch.object(
              compile_install_task,
              "get_bazel_targets",
              return_value=["//:all"],
          ),
      ):
        compile_res = compile_task.do_run()
      self.assertTrue(compile_res)

      zip_task = ZipArtifactTask(args)
      with mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}):
        zip_res = zip_task.do_run()
      self.assertTrue(zip_res)

      zip_path = dist_dir / "netsim-linux-x86_64-99999.zip"
      self.assertTrue(zip_path.is_file())

      with zipfile.ZipFile(zip_path, "r") as zf:
        names = zf.namelist()
        self.assertIn("proto/rootcanal/configuration.proto", names)
        for proto in expected_protos:
          self.assertIn(f"proto/netsim/{proto.name}", names)


class ZipArtifactTaskTest(unittest.TestCase):

  def test_zip_artifact_packages_and_verifies_protos(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      sandbox_tmp.mkdir(parents=True, exist_ok=True)
      sandbox_netsim = sandbox_root / "netsim"
      sandbox_aosp = sandbox_root / "aosp"
      out_dir = sandbox_root / "out"
      dist_dir = sandbox_root / "dist"

      args = _TestArgs(
          out_dir, dist_dir, build_id="12345", target="linux_x86_64"
      )
      compile_task = CompileInstallTask(args, {})

      bazel_bin = sandbox_netsim / "bazel-bin"
      for binary, src in compile_task.binaries.items():
        src_file = bazel_bin / compile_install_task.binary_extension(src)
        src_file.parent.mkdir(parents=True, exist_ok=True)
        src_file.write_text(f"dummy {binary}")

      src_netsim_proto = sandbox_netsim / "proto" / "netsim"
      src_netsim_proto.mkdir(parents=True, exist_ok=True)
      (src_netsim_proto / "common.proto").write_text("// common")
      (src_netsim_proto / "model.proto").write_text("// model")

      rc_proto = (
          sandbox_aosp
          / "tools"
          / "rootcanal"
          / "proto"
          / "rootcanal"
          / "configuration.proto"
      )
      rc_proto.parent.mkdir(parents=True, exist_ok=True)
      rc_proto.write_text("// rc")

      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(
              compile_install_task, "NETSIM_ROOT", sandbox_netsim
          ),
          mock.patch.object(compile_install_task, "AOSP_ROOT", sandbox_aosp),
          mock.patch.object(compile_install_task, "run", return_value=0),
          mock.patch.object(
              compile_install_task,
              "get_bazel_path",
              return_value=Path("/bin/bazel"),
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_build_configs", return_value=[]
          ),
          mock.patch.object(
              compile_install_task, "get_bazel_startup_options", return_value=[]
          ),
          mock.patch.object(
              compile_install_task,
              "get_bazel_targets",
              return_value=["//:all"],
          ),
      ):
        compile_res = compile_task.do_run()
      self.assertTrue(compile_res)

      zip_task = ZipArtifactTask(args)
      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(zip_artifact_task, "NETSIM_ROOT", sandbox_netsim),
      ):
        zip_res = zip_task.do_run()
      self.assertTrue(zip_res)

      zip_path = dist_dir / "netsim-linux-x86_64-12345.zip"
      self.assertTrue(zip_path.is_file())

      with zipfile.ZipFile(zip_path, "r") as zf:
        names = zf.namelist()
        self.assertIn("proto/netsim/common.proto", names)
        self.assertIn("proto/netsim/model.proto", names)
        self.assertIn("proto/rootcanal/configuration.proto", names)

        for binary in compile_install_task.get_netsim_binaries():
          self.assertIn(compile_install_task.binary_extension(binary), names)

        for name in names:
          self.assertFalse(name.endswith("/"))
          self.assertNotIn("\\", name)
          info = zf.getinfo(name)
          self.assertFalse(info.is_dir())

  def test_zip_artifact_raises_when_rootcanal_proto_missing(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      sandbox_tmp.mkdir(parents=True, exist_ok=True)
      sandbox_netsim = sandbox_root / "netsim"
      out_dir = sandbox_root / "out"
      dist_dir = sandbox_root / "dist"
      dest_dir = out_dir / "distribution" / "emulator"

      for binary in compile_install_task.get_netsim_binaries():
        bin_path = dest_dir / compile_install_task.binary_extension(binary)
        bin_path.parent.mkdir(parents=True, exist_ok=True)
        bin_path.write_text("binary")

      dest_netsim_proto = dest_dir / "proto" / "netsim"
      dest_netsim_proto.mkdir(parents=True, exist_ok=True)
      (dest_netsim_proto / "common.proto").write_text("// common")

      src_netsim_proto = sandbox_netsim / "proto" / "netsim"
      src_netsim_proto.mkdir(parents=True, exist_ok=True)
      (src_netsim_proto / "common.proto").write_text("// common")

      args = _TestArgs(
          out_dir, dist_dir, build_id="12345", target="linux_x86_64"
      )
      task = ZipArtifactTask(args)
      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(zip_artifact_task, "NETSIM_ROOT", sandbox_netsim),
      ):
        with self.assertRaises(FileNotFoundError) as ctx:
          task.do_run()
        self.assertIn("rootcanal/configuration.proto", str(ctx.exception))
      self.assertFalse((dist_dir / "netsim-linux-x86_64-12345.zip").exists())

  def test_zip_artifact_raises_when_netsim_proto_missing(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      sandbox_tmp.mkdir(parents=True, exist_ok=True)
      sandbox_netsim = sandbox_root / "netsim"
      out_dir = sandbox_root / "out"
      dist_dir = sandbox_root / "dist"
      dest_dir = out_dir / "distribution" / "emulator"

      for binary in compile_install_task.get_netsim_binaries():
        bin_path = dest_dir / compile_install_task.binary_extension(binary)
        bin_path.parent.mkdir(parents=True, exist_ok=True)
        bin_path.write_text("binary")

      dest_netsim_proto = dest_dir / "proto" / "netsim"
      dest_netsim_proto.mkdir(parents=True, exist_ok=True)
      (dest_netsim_proto / "common.proto").write_text("// common")

      dest_rc = dest_dir / "proto" / "rootcanal" / "configuration.proto"
      dest_rc.parent.mkdir(parents=True, exist_ok=True)
      dest_rc.write_text("// rc")

      src_netsim_proto = sandbox_netsim / "proto" / "netsim"
      src_netsim_proto.mkdir(parents=True, exist_ok=True)
      (src_netsim_proto / "common.proto").write_text("// common")
      (src_netsim_proto / "model.proto").write_text("// model")

      args = _TestArgs(
          out_dir, dist_dir, build_id="12345", target="linux_x86_64"
      )
      task = ZipArtifactTask(args)
      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(zip_artifact_task, "NETSIM_ROOT", sandbox_netsim),
      ):
        with self.assertRaises(FileNotFoundError) as ctx:
          task.do_run()
        self.assertIn("proto/netsim/model.proto", str(ctx.exception))
      self.assertFalse((dist_dir / "netsim-linux-x86_64-12345.zip").exists())

  def test_zip_artifact_raises_when_proto_dir_missing(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      sandbox_tmp.mkdir(parents=True, exist_ok=True)
      sandbox_netsim = sandbox_root / "netsim"
      out_dir = sandbox_root / "out"
      dist_dir = sandbox_root / "dist"
      dest_dir = out_dir / "distribution" / "emulator"

      for binary in compile_install_task.get_netsim_binaries():
        bin_path = dest_dir / compile_install_task.binary_extension(binary)
        bin_path.parent.mkdir(parents=True, exist_ok=True)
        bin_path.write_text("binary")

      src_netsim_proto = sandbox_netsim / "proto" / "netsim"
      src_netsim_proto.mkdir(parents=True, exist_ok=True)
      (src_netsim_proto / "common.proto").write_text("// common")

      args = _TestArgs(
          out_dir, dist_dir, build_id="12345", target="linux_x86_64"
      )
      task = ZipArtifactTask(args)
      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(zip_artifact_task, "NETSIM_ROOT", sandbox_netsim),
      ):
        with self.assertRaises(FileNotFoundError) as ctx:
          task.do_run()
        self.assertIn(
            "Missing required .proto files in release distribution",
            str(ctx.exception),
        )
      self.assertFalse((dist_dir / "netsim-linux-x86_64-12345.zip").exists())

  def test_zip_artifact_raises_when_source_netsim_protos_empty(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      sandbox_tmp.mkdir(parents=True, exist_ok=True)
      sandbox_netsim = sandbox_root / "netsim"
      out_dir = sandbox_root / "out"
      dist_dir = sandbox_root / "dist"
      dest_dir = out_dir / "distribution" / "emulator"

      for binary in compile_install_task.get_netsim_binaries():
        bin_path = dest_dir / compile_install_task.binary_extension(binary)
        bin_path.parent.mkdir(parents=True, exist_ok=True)
        bin_path.write_text("binary")

      dest_netsim_proto = dest_dir / "proto" / "netsim"
      dest_netsim_proto.mkdir(parents=True, exist_ok=True)
      (dest_netsim_proto / "common.proto").write_text("// common")

      dest_rc = dest_dir / "proto" / "rootcanal" / "configuration.proto"
      dest_rc.parent.mkdir(parents=True, exist_ok=True)
      dest_rc.write_text("// rc")

      src_netsim_proto = sandbox_netsim / "proto" / "netsim"
      src_netsim_proto.mkdir(parents=True, exist_ok=True)

      args = _TestArgs(
          out_dir, dist_dir, build_id="12345", target="linux_x86_64"
      )
      task = ZipArtifactTask(args)
      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(zip_artifact_task, "NETSIM_ROOT", sandbox_netsim),
      ):
        with self.assertRaises(FileNotFoundError) as ctx:
          task.do_run()
        self.assertIn("No source .proto files found in", str(ctx.exception))
      self.assertFalse((dist_dir / "netsim-linux-x86_64-12345.zip").exists())

  def test_zip_artifact_raises_when_unexpected_stale_proto_present(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      sandbox_tmp.mkdir(parents=True, exist_ok=True)
      sandbox_netsim = sandbox_root / "netsim"
      out_dir = sandbox_root / "out"
      dist_dir = sandbox_root / "dist"
      dest_dir = out_dir / "distribution" / "emulator"

      for binary in compile_install_task.get_netsim_binaries():
        bin_path = dest_dir / compile_install_task.binary_extension(binary)
        bin_path.parent.mkdir(parents=True, exist_ok=True)
        bin_path.write_text("binary")

      dest_netsim_proto = dest_dir / "proto" / "netsim"
      dest_netsim_proto.mkdir(parents=True, exist_ok=True)
      (dest_netsim_proto / "common.proto").write_text("// common")

      dest_rc = dest_dir / "proto" / "rootcanal" / "configuration.proto"
      dest_rc.parent.mkdir(parents=True, exist_ok=True)
      dest_rc.write_text("// rc")

      src_netsim_proto = sandbox_netsim / "proto" / "netsim"
      src_netsim_proto.mkdir(parents=True, exist_ok=True)
      (src_netsim_proto / "common.proto").write_text("// common")

      stale_proto = dest_netsim_proto / "stale.proto"
      stale_proto.write_text("// unexpected stale proto")

      args = _TestArgs(
          out_dir, dist_dir, build_id="12345", target="linux_x86_64"
      )
      task = ZipArtifactTask(args)
      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(zip_artifact_task, "NETSIM_ROOT", sandbox_netsim),
      ):
        with self.assertRaises(ValueError) as ctx:
          task.do_run()
        self.assertIn("proto/netsim/stale.proto", str(ctx.exception))
        self.assertIn(
            "Unexpected files under proto/ in release distribution",
            str(ctx.exception),
        )
      self.assertFalse((dist_dir / "netsim-linux-x86_64-12345.zip").exists())

  def test_zip_artifact_raises_when_stray_non_proto_file_under_proto(self):
    with tempfile.TemporaryDirectory() as tmp_dir_str:
      sandbox_root = Path(tmp_dir_str)
      sandbox_tmp = sandbox_root / "tmpdir"
      sandbox_tmp.mkdir(parents=True, exist_ok=True)
      sandbox_netsim = sandbox_root / "netsim"
      out_dir = sandbox_root / "out"
      dist_dir = sandbox_root / "dist"
      dest_dir = out_dir / "distribution" / "emulator"

      for binary in compile_install_task.get_netsim_binaries():
        bin_path = dest_dir / compile_install_task.binary_extension(binary)
        bin_path.parent.mkdir(parents=True, exist_ok=True)
        bin_path.write_text("binary")

      dest_netsim_proto = dest_dir / "proto" / "netsim"
      dest_netsim_proto.mkdir(parents=True, exist_ok=True)
      (dest_netsim_proto / "common.proto").write_text("// common")

      dest_rc = dest_dir / "proto" / "rootcanal" / "configuration.proto"
      dest_rc.parent.mkdir(parents=True, exist_ok=True)
      dest_rc.write_text("// rc")

      src_netsim_proto = sandbox_netsim / "proto" / "netsim"
      src_netsim_proto.mkdir(parents=True, exist_ok=True)
      (src_netsim_proto / "common.proto").write_text("// common")

      stray_txt = dest_netsim_proto / "README.txt"
      stray_txt.write_text("non-proto text file")

      args = _TestArgs(
          out_dir, dist_dir, build_id="12345", target="linux_x86_64"
      )
      task = ZipArtifactTask(args)
      with (
          mock.patch.dict(os.environ, {"TMPDIR": str(sandbox_tmp)}),
          mock.patch.object(zip_artifact_task, "NETSIM_ROOT", sandbox_netsim),
      ):
        with self.assertRaises(ValueError) as ctx:
          task.do_run()
        self.assertIn("proto/netsim/README.txt", str(ctx.exception))
        self.assertIn(
            "Unexpected files under proto/ in release distribution",
            str(ctx.exception),
        )
      self.assertFalse((dist_dir / "netsim-linux-x86_64-12345.zip").exists())


if __name__ == "__main__":
  unittest.main()
