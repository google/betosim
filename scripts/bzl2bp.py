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

import argparse
import glob as pyglob
import os
import sys

soong_targets = []
CURRENT_REL_PATH = ""

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
NEXT_DIR = os.path.join(os.path.dirname(SCRIPT_DIR), "next")

SKIPPED_TEST_PACKAGES = {
    "libslirp-rs",
    "verify",
}


def is_test_package_allowed(package_name):
  if not package_name:
    return True
  for skipped in SKIPPED_TEST_PACKAGES:
    if package_name == skipped or package_name.startswith(skipped + "/"):
      return False
  return True


EXACT_DEP_MAPPING = {
    "@protobuf-json-mapping": "libprotobuf_json_mapping",
    "@protobuf-rust": "libprotobuf",
    "//proto": "libnetsim_proto",
    "//rootcanal": "librootcanal",
    "@base64": "libbase64_rust",
    "@log": "liblog_rust",
    "@rootcanal//:lib_rootcanal_ffi": "lib_rootcanal_ffi",
    "@cxx.rs//:cxx": "libcxx",
    "@casimir//:casimir_lib": "libcasimir",
    "//next/testing": "libnetsim_next_netsim_testing_testing",
    "//next/daemon": "netsim_next_daemon",
    "//next/daemon:testing": "libnetsim_next_daemon_lib_testing",
    "//next/testing:netsim-testing": "libnetsim_next_netsim_testing",
    "//next/testing:testing": "libnetsim_next_netsim_testing_testing",
    "//:netsim_link_layer_packets_rust_gen": (
        "rootcanal_link_layer_packets_rust_gen"
    ),
    ":ap_actor_cxx": "libnetsim_next_ap_actor_cxx",
    ":hostap_crypto_shim": "libnetsim_next_ap_actor_cxx",
    "@boringssl//:crypto": "libcrypto",
}

IGNORED_DEPS = {
    "libslirp-rs",
    "@libslirp",
    "@libglib",
    "@aes",
    "@ccm",
    "aes",
    "ccm",
}


def translate_dep(dep):
  for ignored in IGNORED_DEPS:
    if ignored in dep:
      return None

  if dep in EXACT_DEP_MAPPING:
    return EXACT_DEP_MAPPING[dep]

  # Resolve relative labels starting with ":" to absolute
  if dep.startswith(":"):
    rel = CURRENT_REL_PATH.strip("/")
    dep = f"//next/{rel}{dep}" if rel else f"//next{dep}"

  if dep in EXACT_DEP_MAPPING:
    return EXACT_DEP_MAPPING[dep]

  if dep.startswith("@"):
    return "lib" + dep[1:].replace("-", "_")

  if dep.startswith("//"):
    if ":" in dep:
      parts = dep.split(":")
      pkg = parts[0].split("/")[-1]
      target = parts[1]
      if target == "testing" and pkg:
        base = f"{pkg}_testing"
      else:
        base = target
    else:
      base = dep.split("/")[-1]
    if base.startswith("netsim_next_"):
      return "lib" + base.replace("-", "_")
    return "libnetsim_next_" + base.replace("-", "_")

  return dep.replace("-", "_")


def transform_deps(deps):
  out = set()
  for d in deps:
    t = translate_dep(d)
    if t:
      out.add(t)
  return sorted(list(out))


def glob(include, exclude=None):
  out = []
  for pattern in include:
    if pattern.count("**") > 1:
      if pattern.endswith("**"):
        pattern = pattern + "/*"
      base_dir = os.path.join(NEXT_DIR, CURRENT_REL_PATH)
      matched = pyglob.glob(os.path.join(base_dir, pattern), recursive=True)
      for p in matched:
        if os.path.isfile(p):
          rel = os.path.relpath(p, base_dir)
          out.append(rel)
    else:
      out.append(pattern)
  return sorted(out)


def select(items):
  for key in items:
    if "linux" in key or "android" in key:
      return items[key]
  if "//conditions:default" in items:
    return items["//conditions:default"]
  return list(items.values())[0]


def load(label, *args, **kwargs):
  pass


def package(*args, **kwargs):
  pass


def alias(*args, **kwargs):
  pass


def filegroup(*args, **kwargs):
  pass


def netsim_cc_library_static(*args, **kwargs):
  pass


def genrule(name=None, srcs=None, outs=None, cmd=None, tools=None, **kwargs):
  if not name or name in IGNORED_TARGETS:
    return
  soong_name = f"netsim_next_{name.replace('-', '_')}"
  soong_tools = []
  soong_cmd = cmd or ""
  for t in tools or []:
    if t.startswith(":"):
      t_soong = f"netsim_next_{t[1:].replace('-', '_')}"
      soong_tools.append(t_soong)
      soong_cmd = soong_cmd.replace(
          f"$(execpath {t})", f"$(location {t_soong})"
      )
    else:
      soong_tools.append(t)
  soong_cmd = soong_cmd.replace("$(SRCS)", "$(in)").replace("$(OUTS)", "$(out)")
  soong_targets.append({
      "type": "genrule",
      "name": soong_name,
      "tools": soong_tools,
      "cmd": soong_cmd,
      "srcs": list(srcs or []),
      "out": list(outs or []),
  })


def rust_library(*args, **kwargs):
  pass


def exports_files(*args, **kwargs):
  pass


def cc_binary(*args, **kwargs):
  pass


def kt_android_library(*args, **kwargs):
  pass


def sh_test(*args, **kwargs):
  pass


def android_binary(*args, **kwargs):
  pass


def rust_clippy(*args, **kwargs):
  pass


def rust_doc(*args, **kwargs):
  pass


def rust_doc_test(*args, **kwargs):
  pass


def stripped_binaries(*args, **kwargs):
  pass


def redirect_dep_to_testing(d):
  if d.startswith("//next"):
    if d.endswith(":testing"):
      return d
    elif ":" in d:
      return d + "_testing"
    else:
      return d + ":testing"
  return d


STATIC_LIB_OVERRIDES = {"libnetsim_next_ap_actor_cxx"}
SHARED_LIB_OVERRIDES = {"lib_rootcanal_ffi", "libcrypto"}
PLATFORM_RUSTLIBS = {
    "daemon": ["libcommand_fds"],
    "daemon-lib": [
        "libcommand_fds",
        "libbytes",
        "libserde_json",
        "libtokio_util",
        "libnetsim_next_rootcanal_server",
    ],
    "packet-stream": ["libcommand_fds", "libtracing"],
}
IGNORED_TARGETS = {
    "libslirp-rs",
    "cxx-bridge-header",
}
ALLOWED_TESTING_TARGETS = {
    "rest-api",
    "nfc-actor",
    "device-actor",
    "actor-framework",
    "capture-api",
    "common",
    "device-api",
    "link-api",
    "model",
    "types",
    "netsim-testing",
    "daemon-lib",
    "packets",
    "rootcanal",
    "rootcanal-server",
    "modem-rs",
    "websocket-server",
    "bluetooth-actor",
    "capture-actor",
    "cell-actor",
    "grpc-server",
    "hci-server",
    "link-actor",
    "packet-stream",
    "ethernet-actor",
    "uwb-actor",
    "wifi-actor",
    "ap-actor",
    "slirp",
    "slirp-actor",
    "slirp-host",
    "http-proxy",
}


def rust_test(*args, **kwargs):
  name = kwargs.get("name")
  srcs = kwargs.get("srcs")
  if not name or not srcs or name in IGNORED_TARGETS:
    return

  deps = kwargs.get("deps", [])
  redirected_deps = [redirect_dep_to_testing(d) for d in deps]
  proc_macro_deps = kwargs.get("proc_macro_deps", [])

  pkg_name = CURRENT_REL_PATH.strip("/")
  if not pkg_name:
    pkg_name = name

  package_name = CURRENT_REL_PATH.strip("/")
  if not is_test_package_allowed(package_name):
    return

  module_name = pkg_name.replace("-", "_")
  module_prefix = f"{module_name}_" if module_name else ""

  crate_root = kwargs.get("crate_root")
  if not crate_root:
    # Keep in sync with crate_root_opts in next/defs.bzl.
    for opt in [
        "tests/mod.rs",
        "tests/integration_test.rs",
        "tests/integration_tests.rs",
    ]:
      if opt in srcs:
        crate_root = opt
        break
    if not crate_root:
      if srcs:
        crate_root = srcs[0]
      else:
        print(
            f"Warning: rust_test {name} in {CURRENT_REL_PATH} lacks a valid "
            "crate root and has no sources. Skipping.",
            file=sys.stderr,
        )
        return

  if name in ("tests", "test", "integration_tests", "integration-test"):
    if crate_root and "tests/" in crate_root:
      test_suffix = "integration_tests"
    else:
      test_suffix = "tests"
  else:
    test_suffix = name.replace("-", "_")

  target_name = f"libnetsim_next_{module_prefix}{test_suffix}"

  rustlibs = []
  shared_libs = []
  static_libs = []
  for lib in transform_deps(redirected_deps):
    if lib in STATIC_LIB_OVERRIDES:
      static_libs.append(lib)
    elif lib in SHARED_LIB_OVERRIDES:
      shared_libs.append(lib)
    else:
      rustlibs.append(lib)
  proc_macros = transform_deps(proc_macro_deps)

  crate_name = kwargs.get("crate_name")
  if not crate_name:
    if crate_root == "src/lib.rs" or crate_root == "src/main.rs":
      crate_name = module_name
    else:
      crate_name = f"{module_name}_tests"

  default_file = os.path.basename(crate_root) if crate_root else "lib.rs"
  all_inputs = (
      list(srcs)
      + list(kwargs.get("data", []))
      + list(kwargs.get("compile_data", []))
  )
  srcs_content = resolve_rust_srcs(srcs, default_file)
  data_content = resolve_rust_data(all_inputs)
  all_srcs = sorted(list(set(srcs_content + data_content)))
  crate_root_mapped = crate_root if crate_root else resolve_crate_root(all_srcs)

  # Deduplicate: if soong_targets already has a test with matching crate_root or srcs, skip
  for existing in soong_targets:
    if existing.get("type") == "rust_test_host" and (
        existing.get("crate_root") == crate_root_mapped
        or existing.get("srcs") == sorted(list(set(srcs_content)))
    ):
      return

  if (
      "libnetsim_next_ap_actor_cxx" in static_libs
      and "libcrypto" not in shared_libs
  ):
    shared_libs.append("libcrypto")

  tgt_dict = {
      "type": "rust_test_host",
      "name": target_name,
      "crate_name": crate_name,
      "srcs": sorted(list(set(srcs_content + data_content))),
      "crate_root": crate_root_mapped,
      "rustlibs": sorted(list(set(rustlibs))),
      "shared_libs": sorted(list(set(shared_libs))),
      "proc_macros": sorted(list(set(proc_macros))),
      "features": ["cuttlefish", "testing"],
      "edition": kwargs.get("edition", "2024"),
      "test_suites": ["general_tests"],
  }
  if data_content:
    tgt_dict["data"] = data_content
  if static_libs:
    tgt_dict["static_libs"] = sorted(list(set(static_libs)))
  if name == "integration-test" and os.path.exists(
      os.path.join(NEXT_DIR, package_name, "AndroidTest.xml")
  ):
    tgt_dict["test_config"] = "AndroidTest.xml"

  if not any(t["name"] == target_name for t in soong_targets):
    soong_targets.append(tgt_dict)


def rust_test_suite(*args, **kwargs):
  pass


def test_suite(*args, **kwargs):
  pass


def rust_protobuf(*args, **kwargs):
  name = kwargs.get("name")
  srcs = kwargs.get("srcs", [])
  if not name or not srcs or name in IGNORED_TARGETS:
    return

  package_name = CURRENT_REL_PATH.strip("/")
  module_name = package_name.replace("-", "_")
  module_prefix = f"{module_name}_" if module_name else ""
  target_name = f"libnetsim_next_{module_prefix}{name.replace('-', '_')}"

  proto_file = srcs[0] if srcs else "proto/packet.proto"

  soong_targets.append({
      "type": "rust_protobuf",
      "name": target_name,
      "crate_name": name.replace("-", "_"),
      "protos": [proto_file],
      "source_stem": f"{name.replace('-', '_')}_proto",
      "host_supported": True,
      "features": ["cuttlefish"],
  })


def netsim_rust_library(
    name,
    srcs=None,
    deps=None,
    proc_macro_deps=None,
    compile_data=None,
    crate_name=None,
    **kwargs,
):
  prefix = "libnetsim_next_"
  if name in IGNORED_TARGETS:
    return
  if deps is None:
    deps = []
  else:
    deps = list(deps)
  select_deps = kwargs.get("select_deps")
  if select_deps:
    deps.extend(select_deps)
  if proc_macro_deps is None:
    proc_macro_deps = []
  if compile_data is None:
    compile_data = []
  if srcs is None or not isinstance(srcs, list):
    srcs = ["src/**/*.rs"]
  if crate_name is None:
    crate_name = name.replace("-", "_")

  edition = kwargs.get("edition")

  rustlibs = []
  shared_libs = []
  static_libs = []
  for lib in transform_deps(deps):
    if lib in STATIC_LIB_OVERRIDES:
      static_libs.append(lib)
    elif lib in SHARED_LIB_OVERRIDES:
      shared_libs.append(lib)
    else:
      rustlibs.append(lib)

  if name in PLATFORM_RUSTLIBS:
    rustlibs.extend(PLATFORM_RUSTLIBS[name])
  rustlibs = sorted(list(set(rustlibs)))
  proc_macros = transform_deps(proc_macro_deps)
  all_inputs = list(srcs) + list(compile_data) + list(kwargs.get("data", []))
  srcs_content = resolve_rust_srcs(srcs, "lib.rs")
  data_content = resolve_rust_data(all_inputs)
  crate_root_mapped = resolve_crate_root(
      srcs_content, ["src/lib.rs", "src/main.rs"]
  )

  if not srcs_content:
    raise ValueError(
        f"Target '{name}' has an empty source list, cannot resolve crate_root."
    )

  features = ["cuttlefish"]
  crate_features = kwargs.get("crate_features", [])
  if crate_features:
    features.extend(crate_features)
  features = sorted(list(set(features)))

  if (
      "libnetsim_next_ap_actor_cxx" in static_libs
      and "libcrypto" not in shared_libs
  ):
    shared_libs.append("libcrypto")

  # 1. Main Library Target
  main_tgt = {
      "type": "rust_library_host",
      "name": f"{prefix}{name.replace('-', '_')}",
      "crate_name": crate_name,
      "stem": f"lib{crate_name}",
      "crate_root": crate_root_mapped,
      "srcs": srcs_content + data_content,
      "rustlibs": rustlibs,
      "shared_libs": sorted(list(set(shared_libs))),
      "proc_macros": proc_macros,
      "features": features,
      "edition": edition or "2021",
  }
  if static_libs:
    main_tgt["static_libs"] = static_libs
  soong_targets.append(main_tgt)

  # 1.5. Testing Library Target (mimicking defs.bzl)
  testing_deps = kwargs.get("testing_deps", [])
  redirected_deps = [redirect_dep_to_testing(d) for d in deps]

  combined_testing_deps = testing_deps + redirected_deps
  testing_rustlibs = []
  testing_shared_libs = []
  testing_static_libs = []
  for lib in transform_deps(combined_testing_deps):
    if lib in STATIC_LIB_OVERRIDES:
      testing_static_libs.append(lib)
    elif lib in SHARED_LIB_OVERRIDES:
      testing_shared_libs.append(lib)
    else:
      testing_rustlibs.append(lib)

  if (
      "libnetsim_next_ap_actor_cxx" in testing_static_libs
      and "libcrypto" not in testing_shared_libs
  ):
    testing_shared_libs.append("libcrypto")

  if name in PLATFORM_RUSTLIBS:
    for lib in PLATFORM_RUSTLIBS[name]:
      if lib.startswith("libnetsim_next_") and not lib.endswith("_testing"):
        pkg_base = lib.replace("libnetsim_next_", "").replace("_", "-")
        target_lib = lib + "_testing"
        if target_lib in EXACT_DEP_MAPPING:
          testing_rustlibs.append(EXACT_DEP_MAPPING[target_lib])
        elif pkg_base in ALLOWED_TESTING_TARGETS:
          testing_rustlibs.append(target_lib)
        else:
          testing_rustlibs.append(lib)
      else:
        if lib in EXACT_DEP_MAPPING:
          testing_rustlibs.append(EXACT_DEP_MAPPING[lib])
        else:
          testing_rustlibs.append(lib)
  testing_rustlibs = sorted(list(set(testing_rustlibs)))
  testing_shared_libs = sorted(list(set(testing_shared_libs)))
  testing_static_libs = sorted(list(set(testing_static_libs)))

  if name in ALLOWED_TESTING_TARGETS:
    testing_tgt = {
        "type": "rust_library_host",
        "name": f"{prefix}{name.replace('-', '_')}_testing",
        "crate_name": crate_name,
        "stem": f"lib{crate_name}_testing",
        "crate_root": crate_root_mapped,
        "srcs": srcs_content + data_content,
        "rustlibs": testing_rustlibs,
        "shared_libs": testing_shared_libs,
        "proc_macros": proc_macros,
        "features": ["testing", "cuttlefish"],
        "edition": edition,
    }
    if testing_static_libs:
      testing_tgt["static_libs"] = testing_static_libs
    soong_targets.append(testing_tgt)

  # 2. Automatic Test Targets (mimicking defs.bzl)
  enable_unit_test = kwargs.get("enable_unit_test", True)
  enable_integration_test = kwargs.get("enable_integration_test", True)
  test_deps = kwargs.get("test_deps", [])
  redirected_test_deps = [redirect_dep_to_testing(d) for d in test_deps]

  test_rustlibs = []
  test_shared_libs = []
  test_static_libs = []
  for lib in transform_deps(redirected_test_deps):
    if lib in STATIC_LIB_OVERRIDES:
      test_static_libs.append(lib)
    elif lib in SHARED_LIB_OVERRIDES:
      test_shared_libs.append(lib)
    else:
      test_rustlibs.append(lib)
  proc_macro_test_deps = kwargs.get("proc_macro_test_deps", [])

  # 2.1. Inline/Unit Test Target
  if enable_unit_test:
    inline_test_rustlibs = sorted(list(set(testing_rustlibs + test_rustlibs)))
    inline_test_shared_libs = sorted(
        list(set(testing_shared_libs + test_shared_libs))
    )
    inline_test_static_libs = sorted(
        list(set(testing_static_libs + test_static_libs))
    )

    if (
        "libnetsim_next_ap_actor_cxx" in inline_test_static_libs
        and "libcrypto" not in inline_test_shared_libs
    ):
      inline_test_shared_libs.append("libcrypto")

    inline_test_proc_macros = list(proc_macros)
    inline_test_proc_macros.extend(transform_deps(proc_macro_test_deps))
    inline_test_proc_macros = sorted(list(set(inline_test_proc_macros)))

    package_name = CURRENT_REL_PATH.strip("/")
    if is_test_package_allowed(package_name):
      unit_tgt = {
          "type": "rust_test_host",
          "name": f"{prefix}{name.replace('-', '_')}_tests",
          "crate_name": crate_name,
          "crate_root": crate_root_mapped,
          "srcs": srcs_content + data_content,
          "rustlibs": inline_test_rustlibs,
          "shared_libs": inline_test_shared_libs,
          "proc_macros": inline_test_proc_macros,
          "features": ["testing", "cuttlefish"],
          "edition": edition or "2024",
          "test_suites": ["general_tests"],
      }
      if data_content:
        unit_tgt["data"] = data_content
      if inline_test_static_libs:
        unit_tgt["static_libs"] = inline_test_static_libs
      soong_targets.append(unit_tgt)

  # 2.2. Integration Test Target
  current_dir = os.path.join(NEXT_DIR, CURRENT_REL_PATH)
  tests_dir = os.path.join(current_dir, "tests")
  has_integration_tests = False
  integration_test_srcs = []
  if os.path.exists(tests_dir):
    for t_root, _, t_files in os.walk(tests_dir):
      for f in t_files:
        if f.endswith(".rs"):
          abs_path = os.path.join(t_root, f)
          rel_to_current = os.path.relpath(abs_path, current_dir).replace(
              os.sep, "/"
          )
          integration_test_srcs.append(rel_to_current)
          has_integration_tests = True

  integration_test_srcs.sort()

  if enable_integration_test and has_integration_tests:
    crate_root = None
    for opt in [
        "tests/mod.rs",
        "tests/integration_tests.rs",
        "tests/integration_test.rs",
    ]:
      if opt in integration_test_srcs:
        crate_root = opt
        break
    if (
        not crate_root
        and len(integration_test_srcs) == 1
        and integration_test_srcs[0].startswith("tests/")
    ):
      crate_root = integration_test_srcs[0]

    if not crate_root:
      print(
          f"Warning: Integration test in {CURRENT_REL_PATH} lacks a valid crate"
          " root (tests/mod.rs, tests/integration_tests.rs, or"
          " tests/integration_test.rs). Skipping.",
          file=sys.stderr,
      )

    if crate_root:
      integration_test_rustlibs = sorted(
          list(
              set(
                  testing_rustlibs
                  + test_rustlibs
                  + [f"{prefix}{name.replace('-', '_')}_testing"]
              )
          )
      )
      integration_test_shared_libs = sorted(
          list(set(testing_shared_libs + test_shared_libs))
      )
      integration_test_static_libs = sorted(
          list(set(testing_static_libs + test_static_libs))
      )

      if (
          "libnetsim_next_ap_actor_cxx" in integration_test_static_libs
          and "libcrypto" not in integration_test_shared_libs
      ):
        integration_test_shared_libs.append("libcrypto")

      integration_test_proc_macros = list(proc_macros)
      integration_test_proc_macros.extend(transform_deps(proc_macro_test_deps))
      integration_test_proc_macros = sorted(
          list(set(integration_test_proc_macros))
      )

      package_name = CURRENT_REL_PATH.strip("/")
      if is_test_package_allowed(package_name):
        integ_dict = {
            "type": "rust_test_host",
            "name": f"{prefix}{name.replace('-', '_')}_integration_tests",
            "crate_name": f"{crate_name}_tests",
            "crate_root": crate_root,
            "srcs": sorted(
                list(
                    set(integration_test_srcs + resolve_rust_data(compile_data))
                )
            ),
            "rustlibs": integration_test_rustlibs,
            "shared_libs": integration_test_shared_libs,
            "proc_macros": integration_test_proc_macros,
            "features": ["testing", "cuttlefish"],
            "edition": edition or "2024",
            "test_suites": ["general_tests"],
        }
        if data_content:
          integ_dict["data"] = data_content
        if integration_test_static_libs:
          integ_dict["static_libs"] = integration_test_static_libs
        soong_targets.append(integ_dict)


def resolve_crate_root(srcs_content, preferred_candidates=None):
  if preferred_candidates is None:
    preferred_candidates = [
        "src/lib.rs",
        "src/main.rs",
        "tests/integration_tests.rs",
        "tests/integration_test.rs",
        "tests/mod.rs",
    ]
  for cand in preferred_candidates:
    if cand in srcs_content:
      return cand
  rs_files = [s for s in srcs_content if s.endswith(".rs")]
  if rs_files:
    return rs_files[0]
  return "src/lib.rs"


def resolve_rust_srcs(srcs, default_file):
  mapped_srcs = []
  for src in srcs:
    if src in EXACT_DEP_MAPPING:
      mapped_srcs.append(":" + EXACT_DEP_MAPPING[src])
    elif src.startswith(":"):
      mapped_srcs.append(f":netsim_next_{src[1:].replace('-', '_')}")
    elif "**/*.rs" in src:
      mapped_srcs.append(src.replace("**/*.rs", default_file))
    elif "*.rs" in src:
      mapped_srcs.append(src.replace("*.rs", default_file))
    elif src.endswith(".rs"):
      mapped_srcs.append(src)
  return sorted(list(set(mapped_srcs)))


def resolve_rust_data(items):
  mapped_data = []
  for item in items:
    if (
        not item.endswith(".rs")
        and not item.startswith(":")
        and item not in EXACT_DEP_MAPPING
    ):
      mapped_data.append(item)
  return sorted(list(set(mapped_data)))


def netsim_rust_binary(name, srcs=None, deps=None, **kwargs):
  if deps is None:
    deps = []
  else:
    deps = list(deps)
  select_deps = kwargs.get("select_deps")
  if select_deps:
    deps.extend(select_deps)
  if srcs is None or not isinstance(srcs, list):
    srcs = ["src/**/*.rs"]

  rustlibs = transform_deps(deps)
  if name in PLATFORM_RUSTLIBS:
    rustlibs.extend(PLATFORM_RUSTLIBS[name])
  rustlibs = sorted(list(set(rustlibs)))
  all_inputs = (
      list(srcs)
      + list(kwargs.get("compile_data", []))
      + list(kwargs.get("data", []))
  )
  srcs_content = resolve_rust_srcs(srcs, "main.rs")
  data_content = resolve_rust_data(all_inputs)
  crate_root_mapped = kwargs.get("crate_root") or resolve_crate_root(
      srcs_content, ["src/main.rs", "src/lib.rs"]
  )

  if not srcs_content:
    raise ValueError(
        f"Target '{name}' has an empty source list, cannot resolve crate_root."
    )

  soong_name = (
      "netsimd"
      if name == "daemon"
      else (
          "netsim"
          if name == "netsim"
          else f"netsim_next_{name.replace('-', '_')}"
      )
  )

  bin_dict = {
      "type": "rust_binary_host",
      "name": soong_name,
      "stem": soong_name,
      "crate_root": crate_root_mapped,
      "srcs": sorted(list(set(srcs_content + data_content))),
      "rustlibs": rustlibs,
      "features": ["cuttlefish"],
      "edition": kwargs.get("edition", "2021"),
  }
  soong_targets.append(bin_dict)


def rust_proc_macro(name, srcs=None, deps=None, **kwargs):
  if deps is None:
    deps = []
  if srcs is None or not isinstance(srcs, list):
    srcs = ["src/**/*.rs"]

  rustlibs = transform_deps(deps)
  all_inputs = (
      list(srcs)
      + list(kwargs.get("compile_data", []))
      + list(kwargs.get("data", []))
  )
  srcs_content = resolve_rust_srcs(srcs, "lib.rs")
  data_content = resolve_rust_data(all_inputs)
  crate_root_mapped = resolve_crate_root(
      srcs_content, ["src/lib.rs", "src/main.rs"]
  )

  if not srcs_content:
    raise ValueError(
        f"Target '{name}' has an empty source list, cannot resolve crate_root."
    )

  macro_dict = {
      "type": "rust_proc_macro",
      "name": f"libnetsim_next_{name.replace('-', '_')}",
      "crate_name": name.replace("-", "_"),
      "stem": f"lib{name.replace('-', '_')}",
      "edition": kwargs.get("edition"),
      "crate_root": crate_root_mapped,
      "srcs": sorted(list(set(srcs_content + data_content))),
      "rustlibs": rustlibs,
      "features": ["cuttlefish"],
  }
  soong_targets.append(macro_dict)


# Map rust_binary exactly to netsim_rust_binary
def rust_binary(*args, **kwargs):
  netsim_rust_binary(*args, **kwargs)


def rust_cxx_bridge(name, src=None, deps=None, **kwargs):
  if not src:
    return
  stem = os.path.splitext(os.path.basename(src))[0]
  package_name = CURRENT_REL_PATH.strip("/")
  module_name = package_name.replace("-", "_")

  soong_targets.append({
      "type": "genrule",
      "name": f"libnetsim_next_{module_name}_bridge_header",
      "tools": ["cxxbridge"],
      "cmd": "$(location cxxbridge) $(in) --header > $(out)",
      "srcs": [src],
      "out": [f"{module_name}/{stem}.rs.h"],
  })

  soong_targets.append({
      "type": "genrule",
      "name": f"libnetsim_next_{module_name}_bridge_code",
      "tools": ["cxxbridge"],
      "cmd": "$(location cxxbridge) $(in) > $(out)",
      "srcs": [src],
      "out": [f"{module_name}/{stem}.rs.cc"],
  })


def cc_library_static(name, srcs=None, hdrs=None, deps=None, **kwargs):
  package_name = CURRENT_REL_PATH.strip("/")
  module_name = package_name.replace("-", "_")
  if name.startswith(module_name) or name == "ap_actor_cxx":
    target_name = f"libnetsim_next_{name.replace('-', '_')}"
  else:
    target_name = f"libnetsim_next_{module_name}_{name.replace('-', '_')}"

  # Deduplicate: if target already exists in soong_targets, skip or merge
  for existing in soong_targets:
    if existing["name"] == target_name:
      return

  srcs_content = []
  if srcs:
    for s in srcs:
      if s.startswith(":") or "bridge" in s:
        srcs_content.append(f":libnetsim_next_{module_name}_bridge_code")
      else:
        srcs_content.append(s)

  if package_name == "ap-actor":
    target_name = "libnetsim_next_ap_actor_cxx"
    srcs_content = [
        "src/crypto_ffi.cc",
        ":libnetsim_next_ap_actor_bridge_code",
    ]

  for existing in soong_targets:
    if existing["name"] == target_name:
      return

  generated_headers = [
      f"libnetsim_next_{module_name}_bridge_header",
      "cxx-bridge-header",
  ]

  shared_libs = []
  if deps:
    for d in deps:
      if "crypto" in d or "boringssl" in d:
        shared_libs.append("libcrypto")

  soong_targets.append({
      "type": "cc_library_static",
      "name": target_name,
      "host_supported": True,
      "device_supported": False,
      "cflags": [
          "-DRUST_CXX_NO_EXCEPTIONS",
          "-DCONFIG_OPENSSL_INTERNAL_AES_WRAP",
      ],
      "srcs": srcs_content,
      "generated_headers": generated_headers,
      "local_include_dirs": ["src"],
      "shared_libs": sorted(list(set(shared_libs))),
  })


SANDBOX = {
    "glob": glob,
    "select": select,
    "load": load,
    "package": package,
    "alias": alias,
    "filegroup": filegroup,
    "netsim_cc_library_static": netsim_cc_library_static,
    "cc_library_static": cc_library_static,
    "cc_library": cc_library_static,
    "exports_files": exports_files,
    "rust_test": rust_test,
    "rust_test_suite": rust_test_suite,
    "test_suite": test_suite,
    "rust_protobuf": rust_protobuf,
    "netsim_rust_library": netsim_rust_library,
    "netsim_rust_binary": netsim_rust_binary,
    "rust_binary": rust_binary,
    "rust_proc_macro": rust_proc_macro,
    "genrule": genrule,
    "cc_binary": cc_binary,
    "stripped_binaries": stripped_binaries,
    "kt_android_library": kt_android_library,
    "rust_cxx_bridge": rust_cxx_bridge,
    "sh_test": sh_test,
    "android_binary": android_binary,
    "rust_clippy": rust_clippy,
    "rust_doc": rust_doc,
    "rust_doc_test": rust_doc_test,
    "True": True,
    "False": False,
    "None": None,
}


def main():
  global CURRENT_REL_PATH

  parser = argparse.ArgumentParser(
      description="Convert Bazel BUILD files to Soong Android.bp"
  )
  parser.add_argument(
      "--package",
      help="Only process this specific package directory (e.g., 'nfc-actor')",
  )
  args = parser.parse_args()

  repo_dir = NEXT_DIR
  search_dir = repo_dir
  if args.package:
    search_dir = os.path.join(repo_dir, args.package.strip("/"))

  if not os.path.exists(search_dir):
    return

  for root, dirs, files in os.walk(search_dir):
    if "BUILD" in files:
      path = os.path.join(root, "BUILD")
      rel_path = os.path.relpath(root, repo_dir)
      if rel_path == ".":
        rel_path = ""
      else:
        rel_path += "/"

      if "verify/" in rel_path:
        continue

      global soong_targets
      soong_targets = []
      CURRENT_REL_PATH = rel_path
      with open(path, "r") as f:
        code = f.read()

      exec(code, SANDBOX)

      if soong_targets:
        with open(os.path.join(root, "Android.bp"), "w") as f:
          f.write("// This file is generated by bzl2bp.py. DO NOT EDIT.\n\n")
          f.write(
              "package {\n    default_applicable_licenses:"
              ' ["tools_netsim_license"],\n}\n\n'
          )

          for i, tgt in enumerate(soong_targets):
            f.write(f'{tgt["type"]} {{\n')
            f.write(f'    name: "{tgt["name"]}",\n')

            if tgt.get("host_supported") is not None:
              f.write(
                  f'    host_supported: {str(tgt["host_supported"]).lower()},\n'
              )
            if tgt.get("device_supported") is not None:
              f.write(
                  "    device_supported:"
                  f' {str(tgt["device_supported"]).lower()},\n'
              )
            if "crate_name" in tgt:
              f.write(f'    crate_name: "{tgt["crate_name"]}",\n')

            if "edition" in tgt and tgt["edition"]:
              f.write(f'    edition: "{tgt["edition"]}",\n')

            if "stem" in tgt:
              f.write(f'    stem: "{tgt["stem"]}",\n')

            if "crate_root" in tgt:
              f.write(f'    crate_root: "{tgt["crate_root"]}",\n')

            if "test_config" in tgt:
              f.write(f'    test_config: "{tgt["test_config"]}",\n')

            if "test_suites" in tgt:
              f.write("    test_suites: [\n")
              for suite in tgt["test_suites"]:
                f.write(f'        "{suite}",\n')
              f.write("    ],\n")

            if tgt.get("cflags"):
              f.write("    cflags: [\n")
              for cf in tgt["cflags"]:
                f.write(f'        "{cf}",\n')
              f.write("    ],\n")

            if tgt.get("protos"):
              f.write("    protos: [\n")
              for pr in tgt["protos"]:
                f.write(f'        "{pr}",\n')
              f.write("    ],\n")

            if tgt.get("source_stem"):
              f.write(f'    source_stem: "{tgt["source_stem"]}",\n')

            if tgt.get("tools"):
              f.write("    tools: [\n")
              for tl in tgt["tools"]:
                f.write(f'        "{tl}",\n')
              f.write("    ],\n")

            if tgt.get("cmd"):
              f.write(f'    cmd: "{tgt["cmd"]}",\n')

            if tgt.get("out"):
              f.write("    out: [\n")
              for o in tgt["out"]:
                f.write(f'        "{o}",\n')
              f.write("    ],\n")

            if tgt.get("generated_headers"):
              f.write("    generated_headers: [\n")
              for gh in tgt["generated_headers"]:
                f.write(f'        "{gh}",\n')
              f.write("    ],\n")

            if tgt.get("local_include_dirs"):
              f.write("    local_include_dirs: [\n")
              for lid in tgt["local_include_dirs"]:
                f.write(f'        "{lid}",\n')
              f.write("    ],\n")

            f.write("    srcs: [\n")
            for src in tgt["srcs"]:
              f.write(f'        "{src}",\n')
            f.write("    ],\n")

            if tgt.get("data"):
              f.write("    data: [\n")
              for d in sorted(list(set(tgt["data"]))):
                f.write(f'        "{d}",\n')
              f.write("    ],\n")

            if tgt["type"].startswith("rust_"):
              features = tgt.get("features", ["cuttlefish"])
              if len(features) == 1:
                f.write(f'    features: ["{features[0]}"],\n')
              else:
                f.write("    features: [\n")
                for feat in sorted(list(set(features))):
                  f.write(f'        "{feat}",\n')
                f.write("    ],\n")

              f.write('    lints: "none",\n')

              if tgt.get("rustlibs"):
                f.write("    rustlibs: [\n")
                for lib in sorted(list(set(tgt["rustlibs"]))):
                  f.write(f'        "{lib}",\n')
                f.write("    ],\n")
              if tgt.get("static_libs"):
                f.write("    static_libs: [\n")
                for lib in sorted(list(set(tgt["static_libs"]))):
                  f.write(f'        "{lib}",\n')
                f.write("    ],\n")
              if tgt.get("shared_libs"):
                f.write("    shared_libs: [\n")
                for lib in sorted(list(set(tgt["shared_libs"]))):
                  f.write(f'        "{lib}",\n')
                f.write("    ],\n")

              if tgt.get("proc_macros"):
                f.write("    proc_macros: [\n")
                for mac in sorted(list(set(tgt["proc_macros"]))):
                  f.write(f'        "{mac}",\n')
                f.write("    ],\n")
            elif tgt["type"] == "cc_library_static":
              if tgt.get("static_libs"):
                f.write("    static_libs: [\n")
                for lib in sorted(list(set(tgt["static_libs"]))):
                  f.write(f'        "{lib}",\n')
                f.write("    ],\n")
              if tgt.get("shared_libs"):
                f.write("    shared_libs: [\n")
                for lib in sorted(list(set(tgt["shared_libs"]))):
                  f.write(f'        "{lib}",\n')
                f.write("    ],\n")

            if i == len(soong_targets) - 1:
              f.write("}\n")
            else:
              f.write("}\n\n")


if __name__ == "__main__":
  main()
