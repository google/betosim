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

"""A script to find all Rust crate dependencies from Android.bp files

and generate corresponding BUILD.bazel files.
"""

import argparse
import glob
import os
import re
import sys

# --- Functions from get_crate_deps.py ---


def build_dependency_graph(android_build_top):
  """Builds a dependency graph from all Android.bp files in the rust crates directory."""
  graph = {}
  rule_to_path = {}
  crate_to_rule = {}
  crate_roots = [
      'external/rust/android-crates-io/crates',
  ]

  crate_dirs = [os.path.join(android_build_top, root) for root in crate_roots]

  for i, directory in enumerate(crate_dirs):
    if not os.path.exists(directory):
      continue
    for crate_dir in os.listdir(directory):
      android_bp_path = os.path.join(directory, crate_dir, 'Android.bp')
      if not os.path.exists(android_bp_path):
        continue

      try:
        with open(android_bp_path, 'r') as f:
          content = f.read()
      except IOError as e:
        print(f'Warning: Could not read {android_bp_path}: {e}')
        continue

      rule_matches = re.finditer(r'rust_\w+\s*\{(.*?)\}', content, re.DOTALL)

      for match in rule_matches:
        rule_content = match.group(1)
        name_match = re.search(r'name:\s*"(\w+)"', rule_content)
        crate_name_match = re.search(r'crate_name:\s*"([\w-]+)"', rule_content)

        if not name_match:
          continue

        rule_name = name_match.group(1)
        rule_to_path[rule_name] = os.path.join(
            crate_roots[i], crate_dir, 'Android.bp'
        )

        if crate_name_match:
          crate_name = crate_name_match.group(1)
          crate_to_rule[crate_name] = rule_name
        else:
          crate_to_rule[rule_name] = rule_name

        deps = []
        rustlibs_matches = re.findall(
            r'rustlibs:\s*\[(.*?)\]', rule_content, re.DOTALL
        )
        proc_macro_matches = re.findall(
            r'proc_macros:\s*\[(.*?)\]', rule_content, re.DOTALL
        )

        for dep_match in rustlibs_matches + proc_macro_matches:
          dep_list = [
              dep.strip().strip('"')
              for dep in re.split(r'[,\s\n]+', dep_match)
              if dep.strip()
          ]
          deps.extend(dep_list)

        graph[rule_name] = deps

  return graph, rule_to_path, crate_to_rule


def get_all_dependencies(crates, graph, crate_to_rule):
  """Recursively finds all unique dependencies for a given list of crates."""
  initial_rules = []
  for crate in crates:
    if crate in crate_to_rule:
      initial_rules.append(crate_to_rule[crate])
    else:
      initial_rules.append(crate)

  queue = list(initial_rules)
  processed_rules = set(initial_rules)
  all_deps = set(initial_rules)

  while queue:
    rule = queue.pop(0)
    if rule in graph:
      for dep in graph[rule]:
        if dep not in processed_rules:
          processed_rules.add(dep)
          queue.append(dep)
          all_deps.add(dep)

  return sorted(list(all_deps))


# --- Functions from bp_to_bazel.py ---


def extract_block_content(lines, start_index):
  """Extracts content of a {} block, handling nested blocks."""
  content = []
  brace_level = 0
  i = start_index

  if '{' in lines[i]:
    brace_level = lines[i].count('{')
  else:
    return [], start_index + 1

  i += 1
  while i < len(lines):
    line = lines[i]
    brace_level += line.count('{')
    brace_level -= line.count('}')

    if brace_level == 0:
      return content, i + 1

    content.append(line)
    i += 1

  return content, i


def parse_properties(content_lines):
  """Parses key-value pairs from a block's content lines."""
  properties = {}
  in_list_key = None

  full_content = '\n'.join(content_lines)
  # Remove block comments /* ... */
  full_content = re.sub(r'/\*.*?\*/', '', full_content, flags=re.DOTALL)
  # Remove line comments //
  full_content = re.sub(r'//.*', '', full_content)
  # Remove arch blocks
  full_content = re.sub(r'arch: \{.*?\s*\}', '', full_content, flags=re.DOTALL)

  lines = full_content.splitlines()
  i = 0
  while i < len(lines):
    line = lines[i].strip()
    if not line:
      i += 1
      continue

    if in_list_key:
      if ']' in line:
        line_content = line.split(']')[0]
        items_str = line_content.split(',')
        items = [item.strip().strip('"') for item in items_str if item.strip()]
        properties[in_list_key].extend(items)
        in_list_key = None
      else:
        items_str = line.split(',')
        items = [item.strip().strip('"') for item in items_str if item.strip()]
        properties[in_list_key].extend(items)
      i += 1
      continue

    match = re.match(r'(\w+):\s*"([^"]*)",?', line)
    if match:
      key, value = match.groups()
      properties[key] = value
      i += 1
      continue

    match = re.match(r'(\w+):\s*\[', line)
    if match:
      key = match.group(1)
      properties[key] = []
      in_list_key = key

      line_content = line.split('[', 1)[1]
      if ']' in line_content:
        line_content = line_content.split(']')[0]
        items_str = line_content.split(',')
        items = [item.strip().strip('"') for item in items_str if item.strip()]
        properties[key].extend(items)
        in_list_key = None
      else:
        items_str = line_content.split(',')
        items = [item.strip().strip('"') for item in items_str if item.strip()]
        properties[key].extend(items)
      i += 1
      continue
    i += 1
  return properties


def parse_bp_blocks(bp_content):
  """Parses all blocks from an Android.bp file content."""
  bp_content = re.sub(r'//.*', '', bp_content)
  bp_content = re.sub(r'/\*.*?\*/', '', bp_content, flags=re.DOTALL)

  lines = bp_content.splitlines()
  blocks = []
  i = 0
  while i < len(lines):
    line = lines[i].strip()
    if line.endswith('{'):
      block_type = line[:-1].strip()
      content, end_i = extract_block_content(lines, i)
      if block_type not in ['arch', 'target', 'windows']:
        properties = parse_properties(content)
        properties['block_type'] = block_type
        blocks.append(properties)
      i = end_i
    else:
      i += 1
  return blocks


def generate_genrule(block):
  """Generates a Bazel genrule from a parsed block."""
  name = block.get('name')
  srcs = block.get('srcs', [])
  outs = block.get('out', [])
  cmd = block.get('cmd', '')

  srcs_str = ',\n'.join([f'        "{s}"' for s in srcs])
  outs_str = ',\n'.join([f'        "{o}"' for o in outs])

  if len(srcs) > 0:
    cmd = cmd.replace('$(in)', ' '.join([f'$(location {s})' for s in srcs]))
  cmd = cmd.replace('$(genDir)', '$(@D)')

  return f"""
genrule(
    name = "{name}",
    srcs = [
{srcs_str}
    ],
    outs = [
{outs_str}
    ],
    cmd = "{cmd}",
)
"""


def generate_rust_rule(block, crate_info):
  """Generates a Bazel rust_library or rust_proc_macro rule."""
  is_proc_macro = block.get('block_type') == 'rust_proc_macro'
  rule_type = 'rust_proc_macro' if is_proc_macro else 'rust_library'

  name = block.get('name')
  if name.startswith('lib'):
    name = name[3:]
  crate_name = block.get('crate_name', crate_info['crate_name'])
  edition = block.get('edition', '2021')

  deps = block.get('rustlibs', [])
  proc_macro_deps = block.get('proc_macros', [])
  features = block.get('features', [])
  cfgs = []
  if name == 'serde_json' or name == 'serde_json_arbitrary_precision':
    cfgs = ['fast_arithmetic=\\"64\\"']

  deps = [dep[3:] if dep.startswith('lib') else dep for dep in deps]
  proc_macro_deps = [
      dep[3:] if dep.startswith('lib') else dep for dep in proc_macro_deps
  ]

  deps = ['protobuf-rust' if dep == 'protobuf' else dep for dep in deps]

  srcs = block.get('srcs', [])
  # Handle srcs that are references to other rules
  for i, src in enumerate(srcs):
    if src.startswith(':'):
      if 'android_logger_srcs' not in src:
        deps.append(src[1:])
      srcs.pop(i)

  deps_str = ',\n'.join(
      [f'        "@{dep}"' for dep in sorted(list(set(deps)))]
  )
  proc_macro_deps_str = ',\n'.join(
      [f'        "@{dep}"' for dep in sorted(list(set(proc_macro_deps)))]
  )
  flags_str = ',\n'.join(
      [f'        "--cfg=feature=\\"{f}\\""' for f in features]
      + [f'        "--cfg={c}"' for c in cfgs]
  )

  srcs_attr = ''
  if crate_name == 'log' or crate_name == 'zlib_rs':
    srcs_attr = '    srcs = glob(["**/*.rs"]),'
  elif srcs:
    srcs_str = ',\n'.join([f'        "{s}"' for s in srcs])
    srcs_attr = f'    srcs = [\n{srcs_str}\n    ],'
  else:
    srcs_attr = '    srcs = glob(["**/*.rs"]),'

  compile_data_attr = ''
  if crate_name in [
      'getrandom',
      'libz_rs_sys',
      'zlib_rs',
      'clap_derive',
      'clap_builder',
  ]:
    compile_data_attr = '    compile_data = glob(["README.md"]),'
  elif crate_name == 'clap':
    compile_data_attr = '    compile_data = glob(["examples/demo.md"]),'
  crate_root_attr = '    crate_root = "src/lib.rs",'

  content = f"""
{rule_type}(
    name = "{name}",
{srcs_attr}
    crate_name = "{crate_name}",
    edition = "{edition}",
{compile_data_attr}
{crate_root_attr}
"""
  if deps_str:
    content += f'    deps = [\n{deps_str}\n    ],\n'
  if proc_macro_deps_str:
    content += f'    proc_macro_deps = [\n{proc_macro_deps_str}\n    ],\n'
  if flags_str:
    content += f'    rustc_flags = [\n{flags_str}\n    ],\n'

  content += ')\n'
  return content


def generate_bazel_file_content(blocks, crate_info):
  """Generates the content for the BUILD.bazel file."""
  copyright_header = f"""# Copyright 2025 The Android Open Source Project
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
"""
  if crate_info['workspace_name'] == 'grpcio-sys':
    return f"""{copyright_header}
\"\"\"grpcio-sys bazel build rule\"\"\"

load("@rules_rust//rust:defs.bzl", "rust_library")

package(default_visibility = ["//visibility:public"])

rust_library(
    name = "grpcio_sys",
    srcs = glob(["**/*.rs"]),
    crate_name = "grpcio_sys",
    edition = "2018",
    crate_root = "src/lib.rs",
    rustc_env = {{
        "BINDING_PATH": "../bindings/bindings.rs",
    }},
    deps = [
        "@libc",
        "@libz_sys",
    ],
    rustc_flags = [
        "--cfg=feature=\\"_libz-sys\\"",
        "--cfg=feature=\\"_secure\\"",
        "--cfg=feature=\\"boringssl\\"",
        "--cfg=feature=\\"boringssl-src\\"",
        "--cfg=feature=\\"libz-sys\\"",
    ],
)
"""
  if crate_info['workspace_name'] == 'grpcio':
    return f"""{copyright_header}
\"\"\"grpcio bazel build rule\"\"\"

load("@rules_rust//rust:defs.bzl", "rust_library")

package(default_visibility = ["//visibility:public"])

rust_library(
    name = "grpcio",
    srcs = glob(["**/*.rs"]),
    crate_name = "grpcio",
    edition = "2018",
    crate_root = "src/lib.rs",
    deps = [
        "@futures_executor",
        "@futures_util",
        "@grpcio_sys",
        "@libc",
        "@log_rust",
        "@parking_lot",
        "@protobuf-rust",
    ],
    rustc_flags = [
        "--cfg=feature=\\"_secure\\"",
        "--cfg=feature=\\"boringssl\\"",
        "--cfg=feature=\\"protobufv3\\"",
        "--cfg=feature=\\"protobufv3-codec\\"",
    ],
    aliases = {{
        "@protobuf-rust": "protobufv3",
    }},
)
"""
  if crate_info['workspace_name'] == 'protobuf-rust':
    return f"""{copyright_header}
\"\"\"protobuf-rust bazel build rule\"\"\"

load("@rules_rust//rust:defs.bzl", "rust_library")

package(default_visibility = ["//visibility:public"])

genrule(
    name = "copy_protobuf_build_out",
    srcs = ["out/version.rs"],
    outs = ["version.rs"],
    cmd = "cp $(location out/version.rs) $(@D)",
)

rust_library(
    name = "protobuf-rust",
    srcs = glob(["src/**/*.rs"]) + [":copy_protobuf_build_out"],
    crate_name = "protobuf",
    edition = "2021",
    rustc_env = {{"OUT_DIR": ".."}},
    deps = [
        "@bytes",
        "@once_cell",
        "@protobuf_support",
        "@thiserror",
    ],
    rustc_flags = [
        "--cfg=feature=\\"bytes\\"",
    ],
)
"""

  content = f"""{copyright_header}
\"\"\"{crate_info['crate_name']} bazel build rule\"\"\"

"""
  rust_blocks = [
      b
      for b in blocks
      if b.get('block_type')
      in ['rust_library', 'rust_proc_macro', 'rust_library_host']
  ]
  genrule_blocks = [b for b in blocks if b.get('block_type') == 'genrule']

  if rust_blocks:
    primary_block = get_primary_rust_block(rust_blocks)
    rule_type = (
        'rust_proc_macro'
        if primary_block.get('block_type') == 'rust_proc_macro'
        else 'rust_library'
    )
    content += f'load("@rules_rust//rust:defs.bzl", "{rule_type}")\n'

  content += '\npackage(default_visibility = ["//visibility:public"])\n'

  for block in genrule_blocks:
    content += generate_genrule(block)

  for block in rust_blocks:
    content += generate_rust_rule(block, crate_info)

  return content


def get_primary_rust_block(blocks):
  """Selects the most appropriate rust block, prioritizing library rules."""
  rust_lib_blocks = [
      b
      for b in blocks
      if b.get('block_type') in ['rust_library', 'rust_library_host']
  ]
  if not rust_lib_blocks:
    rust_lib_blocks = [b for b in blocks if 'rust' in b.get('block_type', '')]
  if not rust_lib_blocks:
    return None

  rust_lib_blocks.sort(key=lambda b: len(b.get('features', [])))
  return rust_lib_blocks[0]


def main():
  parser = argparse.ArgumentParser(
      description='Find Rust crate dependencies and generate BUILD.bazel files.'
  )
  parser.add_argument(
      'crates',
      metavar='Android.bp_rule_name',
      type=str,
      nargs='+',
      help=(
          'One or more crate names to find dependencies for. Ex) liblog_rust,'
          ' libfutures, etc'
      ),
  )
  parser.add_argument(
      '--android_build_top',
      default=os.path.abspath(
          os.path.join(os.path.dirname(__file__), '../../../')
      ),
      help='Path to the Android source root. Defaults to $ANDROID_BUILD_TOP.',
  )
  parser.add_argument(
      '--output-dir',
      default=os.path.abspath(
          os.path.join(os.path.dirname(__file__), '../bazel_deps/')
      ),
      help='Directory to save the generated BUILD.bazel file.',
  )
  args = parser.parse_args()

  if not args.android_build_top:
    print(
        'Error: ANDROID_BUILD_TOP environment variable is not set.'
        ' Please specify it with --android_build_top.',
        file=sys.stderr,
    )
    sys.exit(1)

  print('Building dependency graph...')
  dependency_graph, rule_to_path_map, crate_to_rule_map = (
      build_dependency_graph(args.android_build_top)
  )

  print(f"Finding all dependencies for: {', '.join(args.crates)}")
  dependencies = get_all_dependencies(
      args.crates, dependency_graph, crate_to_rule_map
  )
  print(f'Found {len(dependencies)} total dependencies.')

  bp_files_to_process = {
      rule: rule_to_path_map[rule]
      for rule in dependencies
      if rule in rule_to_path_map
  }

  for rule, bp_file_rel_path in bp_files_to_process.items():
    bp_file_abs_path = os.path.join(args.android_build_top, bp_file_rel_path)
    if not os.path.exists(bp_file_abs_path):
      print(f'Error: File not found at {bp_file_abs_path}', file=sys.stderr)
      continue

    print(f'Processing {bp_file_rel_path}...')

    try:
      with open(bp_file_abs_path, 'r') as f:
        bp_content = f.read()

      blocks = parse_bp_blocks(bp_content)
      if not blocks:
        print(
            f'Warning: No blocks found in {bp_file_rel_path}', file=sys.stderr
        )
        continue

      rust_lib_block = get_primary_rust_block(blocks)
      if not rust_lib_block:
        print(
            f'Warning: No rust library/macro found in {bp_file_rel_path}.',
            file=sys.stderr,
        )
        continue

      folder_name = os.path.basename(os.path.dirname(bp_file_rel_path))

      raw_name = rust_lib_block.get('name', folder_name)
      bazel_rule_name = raw_name[3:] if raw_name.startswith('lib') else raw_name
      crate_name = rust_lib_block.get(
          'crate_name', bazel_rule_name.replace('-', '_')
      )

      workspace_name = folder_name
      if bazel_rule_name == 'protobuf':
        bazel_rule_name = 'protobuf-rust'
      if workspace_name == 'protobuf':
        workspace_name = 'protobuf-rust'

      crate_info = {
          'workspace_name': workspace_name,
          'bazel_rule_name': bazel_rule_name,
          'crate_name': crate_name,
      }

      bazel_content = generate_bazel_file_content(blocks, crate_info)

      output_filename = f"{crate_info['workspace_name']}.BUILD.bazel"
      output_path = os.path.join(args.output_dir, output_filename)

      os.makedirs(args.output_dir, exist_ok=True)
      with open(output_path, 'w') as f:
        f.write(bazel_content)

      print(f'Successfully generated {output_path}')

    except (ValueError, IOError) as e:
      print(f'Error processing {bp_file_rel_path}: {e}', file=sys.stderr)
      continue
  print("""
        ************************************************************
        IMPORTANT: Make sure to update the crate list in MODULE.bazel
        ************************************************************
        """)


def fix_rule_name():
  search_pattern = os.path.join(
      os.path.dirname(__file__), '..', 'bazel_deps', '*.BUILD.bazel'
  )
  file_paths = glob.glob(search_pattern)

  renames = {}
  # Pass 1: Collect all renames by comparing rule name with filename
  for file_path in file_paths:
    try:
      with open(file_path, 'r') as f:
        content = f.read()
    except IOError:
      continue

    pattern = re.compile(
        r'(?:rust_library|rust_proc_macro)\s*\(.*?name\s*=\s*"([^"]+)"',
        re.DOTALL,
    )
    match = pattern.search(content)
    if not match:
      continue

    old_name = match.group(1)
    filename = os.path.basename(file_path)
    new_name = filename.removesuffix('.BUILD.bazel')

    if old_name != new_name:
      renames[old_name] = new_name

  # Pass 2: Apply renames to rule names and dependencies
  for file_path in file_paths:
    try:
      with open(file_path, 'r') as f:
        content = f.read()
    except IOError:
      continue

    original_content = content

    # Update the rule name itself to match the filename
    filename = os.path.basename(file_path)
    correct_name = filename.removesuffix('.BUILD.bazel')
    pattern = re.compile(
        r'((?:rust_library|rust_proc_macro)\s*\(.*?name\s*=\s*)"([^"]+)"',
        re.DOTALL,
    )
    content = pattern.sub(r'\1"' + correct_name + '"', content, count=1)

    # Update dependencies using the collected renames
    for old, new in renames.items():
      content = content.replace(f'"@{old}"', f'"@{new}"')

      # Replace strings in a list of deps attribute for rules
      deps_pattern = re.compile(r'(deps\s*=\s*\[.*?\])', re.DOTALL)
      for match in deps_pattern.finditer(content):
        deps_block = match.group(1)
        if f'"@{old}"' in deps_block:
          updated_deps_block = deps_block.replace(f'"@{old}"', f'"@{new}"')
          content = content.replace(deps_block, updated_deps_block)

    if content != original_content:
      with open(file_path, 'w') as f:
        f.write(content)


if __name__ == '__main__':
  main()
  os.system(
      'buildifier'
      f' {os.path.abspath(os.path.join(os.path.dirname(__file__), "../bazel_deps/*"))}'
  )
  fix_rule_name()
