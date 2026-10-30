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

"""Repository rules for local rust crates."""

def _patched_new_local_repository_impl(ctx):
    netsim_dir = ctx.path(Label("@netsim//:MODULE.bazel")).dirname
    workspace_root = str(netsim_dir.dirname.dirname)

    if ctx.attr.path.startswith("/") or ctx.attr.path.startswith("\\") or (len(ctx.attr.path) > 1 and ctx.attr.path[1] == ":"):
        src_path = ctx.attr.path
    else:
        src_path = workspace_root + "/" + ctx.attr.path

    os_name = ctx.os.name.lower()
    if os_name.startswith("windows"):
        prebuilt_py = workspace_root + "/prebuilts/python/windows-x86/python.exe"
    elif os_name.startswith("mac") or "darwin" in os_name:
        prebuilt_py = workspace_root + "/prebuilts/python/darwin-x86/bin/python3"
    else:
        prebuilt_py = workspace_root + "/prebuilts/python/linux-x86/bin/python3"

    python_bin = "python3"
    if ctx.path(prebuilt_py).exists:
        python_bin = prebuilt_py
    elif ctx.which("python3"):
        python_bin = str(ctx.which("python3"))
    elif ctx.which("python"):
        python_bin = str(ctx.which("python"))

    # Copy source to root using python
    res = ctx.execute([
        python_bin,
        "-c",
        "import shutil, sys, os; shutil.copytree(sys.argv[1], sys.argv[2], dirs_exist_ok=True)",
        str(src_path),
        ".",
    ])

    if res.return_code != 0:
        fail("Failed to copy source: " + res.stderr)

    # Symlink build file
    ctx.symlink(ctx.attr.build_file, "BUILD.bazel")

    # Apply patches
    for patch in ctx.attr.patches:
        ctx.patch(patch, strip = 1)

patched_new_local_repository = repository_rule(
    implementation = _patched_new_local_repository_impl,
    attrs = {
        "build_file": attr.label(mandatory = True),
        "patches": attr.label_list(default = []),
        "path": attr.string(mandatory = True),
    },
)
