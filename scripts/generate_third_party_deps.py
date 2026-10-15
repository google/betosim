#!/usr/bin/env python3
# Copyright 2025 The Android Open Source Project
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

"""A script to find all third-party Rust crate dependencies

and generate corresponding BUILD.bazel files.

This script is only for processing crates in
'external/qemu/android/third_party/rust/crates'
"""

import argparse
import os
import re
import sys
import toml

# Path to the third-party crates
THIRD_PARTY_CRATES_DIR = 'external/qemu/android/third_party/rust/crates'


def get_crate_name_and_version(dir_name):
  """Extracts crate name and version from directory name."""
  match = re.match(r'^(.*)-(\d+\.\d+\.\d+.*)$', dir_name)
  if match:
    return match.groups()
  return dir_name, None


def generate_build_file_content(crate_name, deps):
  """Generates a simple BUILD.bazel file content for a rust crate."""
  crate_name_underscores = crate_name.replace('-', '_')
  deps_str = ''
  if deps:
    deps_list_str = ',\n'.join(
        [f'        "@{dep}"' for dep in sorted(list(set(deps)))]
    )
    deps_str = f'\n    deps = [\n{deps_list_str}\n    ],'
  return f"""# Copyright 2025 The Android Open Source Project
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

\"\"\"{crate_name} bazel build rule\"\"\"

load("@rules_rust//rust:defs.bzl", "rust_library")

package(default_visibility = [\"//visibility:public\"])

rust_library(
    name = \"{crate_name}\",
    srcs = glob([\"**/*.rs\"]),
    crate_name = \"{crate_name_underscores}\",
    edition = \"2021\",{deps_str}
)
"""


def add_to_workspace(workspace_path, name, crate_path, build_file_name):
  """Appends a new_local_repository rule to the WORKSPACE file if not present."""
  build_file_label = f'//bazel_deps:{build_file_name}'
  workspace_entry = (
      '\nnew_local_repository(\n'
      f'    name = "{name}",\n'
      f'    path = "{crate_path}",\n'
      f'    build_file = "{build_file_label}",\n'
      ')\n'
  )
  try:
    with open(workspace_path, 'r+') as f:
      content = f.read()
      if f'name = "{name}"' in content:
        print(f'Workspace entry for {name} already exists in {workspace_path}')
        return
      f.write(workspace_entry)
    print(f'Successfully updated {workspace_path} with {name}')
  except IOError as e:
    print(f'Error updating WORKSPACE file: {e}', file=sys.stderr)


def find_crate_dir(crates_dir, crate_name):
  """Finds the directory for a given crate name."""
  for item in os.listdir(crates_dir):
    path = os.path.join(crates_dir, item)
    if os.path.isdir(path):
      name, _ = get_crate_name_and_version(item)
      if name == crate_name:
        return item
  return None


def get_dependencies(crates_dir, crate_dir_name):
  """Parses Cargo.toml to get non-optional dependencies."""
  cargo_toml_path = os.path.join(crates_dir, crate_dir_name, 'Cargo.toml')
  if not os.path.exists(cargo_toml_path):
    return []
  with open(cargo_toml_path, 'r') as f:
    cargo_toml = toml.load(f)
  deps = []
  if 'dependencies' in cargo_toml:
    for dep, value in cargo_toml['dependencies'].items():
      if isinstance(value, dict) and value.get('optional'):
        continue
      deps.append(dep)
  return deps


def get_all_dependencies(crates_dir, initial_crates):
  """Recursively finds all unique dependencies for a given list of crates."""
  queue = list(initial_crates)
  processed_crates = set(initial_crates)
  all_deps = set(initial_crates)

  while queue:
    crate = queue.pop(0)
    crate_dir_name = find_crate_dir(crates_dir, crate)
    if not crate_dir_name:
      print(f"Warning: Crate '{crate}' not found.", file=sys.stderr)
      continue
    deps = get_dependencies(crates_dir, crate_dir_name)
    for dep in deps:
      if dep not in processed_crates:
        processed_crates.add(dep)
        queue.append(dep)
        all_deps.add(dep)
  return sorted(list(all_deps))


def main():
  parser = argparse.ArgumentParser(
      description='Generate Bazel dependencies for third-party Rust crates.'
  )
  parser.add_argument(
      'crates',
      metavar='CRATE',
      type=str,
      nargs='+',
      help='One or more crate names to find dependencies for.',
  )
  parser.add_argument(
      '--android_build_top',
      default=os.environ.get('ANDROID_BUILD_TOP'),
      help='Path to the Android source root. Defaults to $ANDROID_BUILD_TOP.',
  )
  parser.add_argument(
      '--output-dir',
      default='tools/netsim/bazel_deps',
      help='Directory to save the generated BUILD.bazel file.',
  )
  parser.add_argument(
      '--workspace',
      default='tools/netsim/WORKSPACE',
      help='Path to the WORKSPACE file to update.',
  )
  args = parser.parse_args()

  if not args.android_build_top:
    print(
        'Error: ANDROID_BUILD_TOP environment variable is not set.',
        file=sys.stderr,
    )
    sys.exit(1)

  crates_dir = os.path.join(args.android_build_top, THIRD_PARTY_CRATES_DIR)
  if not os.path.isdir(crates_dir):
    print(f'Error: Crates directory not found at {crates_dir}', file=sys.stderr)
    sys.exit(1)

  all_crates = get_all_dependencies(crates_dir, args.crates)
  print(f"Processing crates and their dependencies: {', '.join(all_crates)}")

  for crate_name in all_crates:
    crate_dir_name = find_crate_dir(crates_dir, crate_name)
    if not crate_dir_name:
      continue

    print(f'Processing {crate_dir_name}...')
    deps = get_dependencies(crates_dir, crate_dir_name)

    # Generate BUILD.bazel file
    build_content = generate_build_file_content(crate_name, deps)
    build_filename = f'{crate_name}.BUILD.bazel'
    output_path = os.path.join(args.output_dir, build_filename)

    os.makedirs(args.output_dir, exist_ok=True)
    with open(output_path, 'w') as f:
      f.write(build_content)
    print(f'Generated {output_path}')

    # Add to WORKSPACE
    relative_crate_path = os.path.join(
        '../../', THIRD_PARTY_CRATES_DIR, crate_dir_name
    )
    add_to_workspace(
        args.workspace, crate_name, relative_crate_path, build_filename
    )


if __name__ == '__main__':
  main()
  os.system('buildifier tools/netsim/bazel_deps/* tools/netsim/WORKSPACE')
