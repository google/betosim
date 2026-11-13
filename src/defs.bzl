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

"""
Netsim Rust build rules.

This module provides the netsim_rust_library macro which wraps rust_library
to provide standard targets for testing, linting, and formatting.
"""

load("@bazel_tools//tools/cpp:toolchain_utils.bzl", "find_cpp_toolchain")
load("@rules_cc//cc:action_names.bzl", "ACTION_NAMES")
load("@rules_cc//cc/common:cc_common.bzl", "cc_common")
load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_clippy", "rust_common", "rust_doc", "rust_doc_test", "rust_library", "rust_test")

NETSIM_RUSTC_FLAGS = ["-Dwarnings", "-Dunused_crate_dependencies", "-Dunreachable_pub"]
NETSIM_CLIPPY_LINTS = {
    # Async / Concurrency
    "await_holding_lock": "deny",
    "future_not_send": "deny",
    "large_futures": "deny",
    "let_underscore_future": "deny",
    "mutex_atomic": "deny",
    "rc_mutex": "deny",
    "significant_drop_in_scrutinee": "deny",
    "unused_async": "deny",
    # Performance
    "large_stack_arrays": "deny",
    "large_types_passed_by_value": "deny",
    # FFI / Safety
    "borrow_as_ptr": "deny",
    "cast_possible_wrap": "deny",
    "cast_ptr_alignment": "deny",
    "ptr_as_ptr": "deny",
    "ptr_cast_constness": "deny",
    "ref_as_ptr": "deny",
    "undocumented_unsafe_blocks": "deny",
    # Code Hygiene
    "cloned_instead_of_copied": "deny",
    "dbg_macro": "deny",
    "format_push_string": "deny",
    "indexing_slicing": "deny",
    "map_err_ignore": "deny",
    "match_wild_err_arm": "deny",
    "panic": "deny",
    "panic_in_result_fn": "deny",
    "print_stderr": "deny",
    "print_stdout": "deny",
    "todo": "deny",
    "unimplemented": "deny",
    "unreachable": "deny",
    "unwrap_in_result": "deny",
    "unwrap_used": "deny",
}

NETSIM_TEST_CLIPPY_ALLOWS = {
    "cast_possible_wrap": "allow",
    "field_reassign_with_default": "allow",
    "indexing_slicing": "allow",
    "module_inception": "allow",
    "panic": "allow",
    "panic_in_result_fn": "allow",
    "print_stderr": "allow",
    "print_stdout": "allow",
    "todo": "allow",
    "unimplemented": "allow",
    "unreachable": "allow",
    "unused_async": "allow",
    "unwrap_in_result": "allow",
    "unwrap_used": "allow",
}

# Unfortunately, we can't use the rules_rust version because netsim is in external/.
def _netsim_rustfmt_test_impl(ctx):
    toolchain = ctx.toolchains["@rules_rust//rust:toolchain_type"]
    rustfmt, config = toolchain.rustfmt, ctx.file.config

    seen = {}
    srcs = []
    for t in ctx.attr.targets:
        info = t[rust_common.crate_info] if rust_common.crate_info in t else getattr(t, "crate_info", None)
        if info:
            for s in info.srcs.to_list():
                if s.is_source and s.short_path not in seen:
                    seen[s.short_path] = True
                    srcs.append(s)

    if not srcs:
        fail("No sources to format")

    runner = ctx.actions.declare_file(ctx.label.name + ".sh")
    ctx.actions.write(
        output = runner,
        is_executable = True,
        content = "#!/bin/bash\n%s --check --config-path %s %s" % (
            rustfmt.short_path,
            config.short_path,
            " ".join([s.short_path for s in srcs]),
        ),
    )

    return [DefaultInfo(
        executable = runner,
        runfiles = ctx.runfiles(files = srcs + [rustfmt, config] + toolchain.all_files.to_list()),
    )]

netsim_rustfmt_test = rule(
    implementation = _netsim_rustfmt_test_impl,
    attrs = {
        "targets": attr.label_list(providers = [[rust_common.crate_info]]),
        "config": attr.label(allow_single_file = True, default = Label("//src:rustfmt_stable.toml")),
    },
    test = True,
    toolchains = ["@rules_rust//rust:toolchain_type"],
)

def _define_common_targets(name, enable_clippy, enable_rustfmt, targets = [], target_compatible_with = []):
    # Clippy Linter
    if enable_clippy:
        rust_clippy(
            name = name + "_clippy",
            deps = targets,
            target_compatible_with = target_compatible_with,
            testonly = True,
        )

    # Rustfmt Check
    if enable_rustfmt:
        netsim_rustfmt_test(
            name = name + "_rustfmt",
            targets = targets,
            # Restrict to Linux for CI validation.
            target_compatible_with = ["@platforms//os:linux"] + target_compatible_with,
        )

def netsim_rust_library(
        name,
        srcs,
        deps = [],
        select_deps = [],
        select_testing_deps = None,
        testing_deps = [],
        test_deps = [],
        proc_macro_test_deps = [],
        crate_features = [],
        # Feature Flags (Default to True for safety)
        enable_clippy = True,
        enable_rustfmt = True,
        enable_unit_test = True,
        enable_integration_test = True,
        enable_doc = True,
        enable_doc_test = True,
        test_args = [],
        integration_test_args = [],
        **kwargs):
    """
    Defines a Netsim Rust library with standard targets.

    This macro wraps rust_library and automatically generates:
    - _test: Unit tests for the crate.
    - _integration_test: Integration tests for the crate if tests/ subdirectory has files.
    - _clippy: Clippy checks.
    - _fmt: Rustfmt checks.
    - _doc: Documentation generation.
    - _doc_test: Documentation tests.

    Args:
        name: The name of the library.
        srcs: The source files.
        deps: The dependencies.
        select_deps: The part of deps that uses select()
        select_testing_deps: The part of testing_deps that uses select(). Defaults to select_deps.
        testing_deps: Dependencies for the "testing" feature.
        test_deps: Dependencies for the unit test and integration test targets.
        proc_macro_test_deps: Proc-macro dependencies for the unit test and integration test targets.
        crate_features: the crate features
        enable_clippy: Whether to enable clippy checks.
        enable_rustfmt: Whether to enable rustfmt checks.
        enable_unit_test: Whether to generate a unit test target.
        enable_integration_test: Whether to generate an integration test target if files exist.
        enable_doc: Whether to generate documentation.
        enable_doc_test: Whether to generate a doc test target.
        test_args: Command-line arguments passed to the unit test binary.
        integration_test_args: Command-line arguments passed to the integration test binary.
        **kwargs: Additional arguments passed to rust_library.
    """

    default_lint_config = (
        Label("//src:test_clippy_lint_config") if kwargs.get("testonly", False) else Label("//src:clippy_lint_config")
    )
    lint_config = kwargs.pop("lint_config", default_lint_config if enable_clippy else None)
    test_lint_config = Label("//src:test_clippy_lint_config") if enable_clippy else None

    # 1. The Main Library (Always created)
    rustc_flags = NETSIM_RUSTC_FLAGS + kwargs.pop("rustc_flags", [])

    if select_testing_deps == None:
        select_testing_deps = select_deps

    rust_library(
        name = name,
        srcs = srcs,
        deps = deps + select_deps,
        rustc_flags = rustc_flags,
        crate_features = crate_features,
        lint_config = lint_config,
        **kwargs
    )

    # 2. The Testing Library with "testing" feature (always created)
    testing_deps = testing_deps + [
        d.split(":")[0] + ":testing" if d.startswith("//src") else d
        for d in deps
    ]
    test_deps = [
        d.split(":")[0] + ":testing" if d.startswith("//src") else d
        for d in test_deps
    ]
    rust_library(
        name = "testing",
        srcs = srcs,
        deps = testing_deps + select_testing_deps,
        rustc_flags = rustc_flags,
        crate_features = ["testing"] + crate_features,
        lint_config = test_lint_config,
        **kwargs
    )

    # 3. Automatic Unit Test
    if enable_unit_test:
        # There can be integ test deps that are not used in unit tests,
        # so disable unused_crate_dependencies and unreachable_pub.
        test_flags = [f for f in rustc_flags if f not in ["-Dunused_crate_dependencies", "-Dunreachable_pub"]]

        rust_test(
            name = "test",
            srcs = srcs,
            args = test_args,
            crate_root = kwargs.get("crate_root"),
            crate_features = ["testing"] + crate_features,
            crate_name = kwargs.get("crate_name", name),
            aliases = kwargs.get("aliases", {}),
            rustc_flags = test_flags,
            lint_config = test_lint_config,
            deps = testing_deps + select_testing_deps + test_deps,
            proc_macro_deps = kwargs.get("proc_macro_deps", []) + proc_macro_test_deps,
            edition = kwargs.get("edition", "2021"),
            env = kwargs.get("env", {}),
            rustc_env = kwargs.get("rustc_env", {}),
            stamp = kwargs.get("stamp", 0),
            compile_data = kwargs.get("compile_data", []),
            data = kwargs.get("data", []),
            tags = kwargs.get("tags", []),
            target_compatible_with = kwargs.get("target_compatible_with", []),
            visibility = kwargs.get("visibility", []),
            testonly = True,
        )

    # 4. Automatic Integration Test
    integration_test_srcs = native.glob(["tests/**/*.rs"], allow_empty = True)
    has_integration_test = enable_integration_test and len(integration_test_srcs) > 0

    if has_integration_test:
        # Integration tests receive all library dependencies for convenience,
        # so disable unused_crate_dependencies and unreachable_pub.
        test_flags = [f for f in rustc_flags if f not in ["-Dunused_crate_dependencies", "-Dunreachable_pub"]]

        # If there's a mod.rs, it's typically the root.
        # Else it's likely a single-file setup.
        crate_root_opts = [
            "tests/mod.rs",
            "tests/integration_test.rs",
            "tests/integration_tests.rs",
        ]
        crate_root = None
        for opt in crate_root_opts:
            if opt in integration_test_srcs:
                crate_root = opt
                break

        rust_test(
            name = "integration-test",
            srcs = integration_test_srcs,
            args = integration_test_args,
            crate_root = crate_root,
            crate_features = crate_features,
            aliases = kwargs.get("aliases", {}),
            rustc_flags = test_flags,
            lint_config = test_lint_config,
            compile_data = kwargs.get("compile_data", []),
            deps = [":testing"] + testing_deps + select_testing_deps + test_deps,
            proc_macro_deps = kwargs.get("proc_macro_deps", []) + proc_macro_test_deps,
            edition = kwargs.get("edition", "2021"),
            env = kwargs.get("env", {}),
            rustc_env = kwargs.get("rustc_env", {}),
            stamp = kwargs.get("stamp", 0),
            data = kwargs.get("data", []),
            tags = kwargs.get("tags", []),
            target_compatible_with = kwargs.get("target_compatible_with", []),
            visibility = kwargs.get("visibility", []),
            testonly = True,
        )

    # 5. Common Targets (Clippy, Rustfmt)
    tgs = [":" + name]
    if enable_unit_test:
        tgs.append(":test")
    if has_integration_test:
        tgs.append(":integration-test")

    target_compatible_with = kwargs.get("target_compatible_with", [])

    _define_common_targets(
        name,
        enable_clippy,
        enable_rustfmt,
        targets = tgs,
        target_compatible_with = target_compatible_with,
    )

    # 6. Documentation
    if enable_doc:
        rust_doc(
            name = "doc",
            crate = ":" + name,
            crate_features = crate_features,
            target_compatible_with = target_compatible_with,
            testonly = True,
        )

    # 7. Documentation Test
    if enable_doc_test:
        rust_doc_test(
            name = "doc-test",
            crate = ":" + name,
            crate_features = crate_features,
            target_compatible_with = ["@platforms//os:linux"] + target_compatible_with,
            testonly = True,
        )

def netsim_rust_binary(
        name,
        srcs,
        deps = [],
        select_deps = [],
        crate_features = [],
        # Feature Flags (Default to True for safety)
        enable_clippy = True,
        enable_rustfmt = True,
        **kwargs):
    """
    Defines a Netsim Rust binary with standard targets.

    This macro wraps rust_binary and automatically generates:
    - _clippy: Clippy checks.
    - _fmt: Rustfmt checks.

    Args:
        name: The name of the binary.
        srcs: The source files.
        deps: The dependencies.
        select_deps: The part of deps that uses select()
        crate_features: the crate features
        enable_clippy: Whether to enable clippy checks.
        enable_rustfmt: Whether to enable rustfmt checks.
        **kwargs: Additional arguments passed to rust_binary.
    """

    default_lint_config = (
        Label("//src:test_clippy_lint_config") if kwargs.get("testonly", False) else Label("//src:clippy_lint_config")
    )
    lint_config = kwargs.pop("lint_config", default_lint_config if enable_clippy else None)

    # 1. The Main Binary
    rustc_flags = NETSIM_RUSTC_FLAGS + kwargs.pop("rustc_flags", [])

    rust_binary(
        name = name,
        srcs = srcs,
        deps = deps + select_deps,
        rustc_flags = rustc_flags,
        crate_features = crate_features,
        lint_config = lint_config,
        **kwargs
    )

    # 2. Common Targets (Clippy, Rustfmt)
    _define_common_targets(
        name,
        enable_clippy,
        enable_rustfmt,
        targets = [":" + name],
        target_compatible_with = kwargs.get("target_compatible_with", []),
    )

def _stripped_binaries_impl(ctx):
    out_files = []
    is_windows = ctx.target_platform_has_constraint(ctx.attr._windows_constraint[platform_common.ConstraintValueInfo])
    cc_toolchain = find_cpp_toolchain(ctx)

    # `cc_toolchain.strip_executable` is the legacy `tool_path` entry, which
    # modern toolchains (including the hermetic goldfish one) leave as a
    # non-existent placeholder next to the toolchain's BUILD file. The real
    # binary has to be resolved through the action config instead.
    #
    # Only resolve it where a strip action actually exists. The clang-cl
    # toolchain used for Windows configures no strip action at all, and
    # get_tool_for_action() fails at analysis time rather than returning None,
    # so this must stay inside the guard even though the Windows branch below
    # never uses the tool.
    strip_executable = None
    if not is_windows:
        feature_configuration = cc_common.configure_features(
            ctx = ctx,
            cc_toolchain = cc_toolchain,
            requested_features = ctx.features,
            unsupported_features = ctx.disabled_features,
        )
        strip_executable = cc_common.get_tool_for_action(
            feature_configuration = feature_configuration,
            action_name = ACTION_NAMES.strip,
        )

    for src in ctx.files.srcs:
        out = ctx.actions.declare_file("_stripped/" + src.basename)
        if is_windows:
            # Windows binaries (PE/COFF) carry no strippable ELF symbol table and
            # the MSVC toolchain exposes no `strip` tool, so pass the binary
            # through unmodified. `ctx.actions.symlink` is the portable way to
            # re-materialize a file under a new path; Bazel degrades to a real
            # copy on filesystems that cannot create symlinks.
            ctx.actions.symlink(
                output = out,
                target_file = src,
                progress_message = "Copying %s (strip unsupported on Windows)" % src.short_path,
            )
        else:
            # `all_files` rather than `strip_files`: the tool resolved above may
            # live in a different filegroup than the one the legacy
            # `strip_files` attribute points at, and it must be staged in the
            # sandbox for the action to find it.
            ctx.actions.run_shell(
                outputs = [out],
                inputs = depset([src], transitive = [cc_toolchain.all_files]),
                command = "cp \"$1\" \"$2\" && chmod u+w \"$2\" && \"$3\" \"$2\"",
                arguments = [src.path, out.path, strip_executable],
                mnemonic = "NetsimStrip",
                progress_message = "Stripping %s with %s" % (src.short_path, strip_executable),
            )
        out_files.append(out)
    return [DefaultInfo(files = depset(out_files))]

stripped_binaries = rule(
    doc = """Strip binaries and place them into the _stripped directory using the C++ toolchain.""",
    implementation = _stripped_binaries_impl,
    attrs = {
        "srcs": attr.label_list(
            doc = "The executable binaries to strip.",
            mandatory = True,
            allow_files = True,
        ),
        "_cc_toolchain": attr.label(default = "@bazel_tools//tools/cpp:current_cc_toolchain"),
        "_windows_constraint": attr.label(default = "@platforms//os:windows"),
    },
    fragments = ["cpp"],
    toolchains = ["@bazel_tools//tools/cpp:toolchain_type"],
)
