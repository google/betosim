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
load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_common", "rust_doc", "rust_doc_test", "rust_library", "rust_test")

NETSIM_RUSTC_FLAGS = ["-Dwarnings", "-Dunused_crate_dependencies", "-Dunreachable_pub"]
NETSIM_CLIPPY_FLAGS = [
    "-Dwarnings",
    # Async / Concurrency
    "-Dclippy::await_holding_lock",
    "-Dclippy::future_not_send",
    "-Dclippy::large_futures",
    "-Dclippy::let_underscore_future",
    "-Dclippy::mutex_atomic",
    "-Dclippy::rc_mutex",
    "-Dclippy::significant_drop_in_scrutinee",
    "-Dclippy::unused_async",
    # Performance
    "-Dclippy::large_stack_arrays",
    "-Dclippy::large_types_passed_by_value",
    # FFI / Safety
    "-Dclippy::borrow_as_ptr",
    "-Dclippy::cast_possible_wrap",
    "-Dclippy::cast_ptr_alignment",
    "-Dclippy::ptr_as_ptr",
    "-Dclippy::ptr_cast_constness",
    "-Dclippy::ref_as_ptr",
    "-Dclippy::undocumented_unsafe_blocks",
    # Code Hygiene
    "-Dclippy::cloned_instead_of_copied",
    "-Dclippy::dbg_macro",
    "-Dclippy::format_push_string",
    "-Dclippy::map_err_ignore",
    "-Dclippy::match_wild_err_arm",
    "-Dclippy::print_stderr",
    "-Dclippy::print_stdout",
    "-Dclippy::unwrap_in_result",
    "-Dclippy::unwrap_used",
    "-Dclippy::indexing_slicing",
    "-Dclippy::panic",
    "-Dclippy::todo",
    "-Dclippy::unimplemented",
    "-Dclippy::unreachable",
    "-Dclippy::panic_in_result_fn",
]

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
        "config": attr.label(allow_single_file = True, default = Label("//next:rustfmt_stable.toml")),
    },
    test = True,
    toolchains = ["@rules_rust//rust:toolchain_type"],
)

# NetsimDepInfo is used to transitively collect .rlib files and crate names
# across the dependency graph. We need this because standard rust_library
# providers don't easily expose the raw .rlib file paths in a way that
# can be uniformly consumed by our custom clippy rule.
NetsimDepInfo = provider(
    doc = "Provider for Netsim dependencies.",
    fields = {
        "crate_name": "Crate name",
        "rlib": "The .rlib file",
        "transitive_rlibs": "Depset of transitive .rlib files",
        "direct_externs": "Depset of direct crate_name=path strings",
        "transitive_externs": "Depset of transitive crate_name=path strings",
        "crate_features": "List of crate features",
    },
)

def _netsim_dep_aspect_impl(target, ctx):
    info = None
    if rust_common.crate_info in target:
        info = target[rust_common.crate_info]
    crate_name = info.name if info else ""

    lib_file = None

    # We hunt for the compiled artifact (.rlib, .so, .dylib, .dll) in DefaultInfo.
    # This is a heuristic to find the library file for both internal and external crates
    # without relying on internal implementation details of rules_rust.
    for f in target[DefaultInfo].files.to_list():
        if f.extension in ["rlib", "so", "dylib", "dll"]:
            lib_file = f
            break

    direct_rlibs = []
    direct_externs = []
    if crate_name and lib_file:
        direct_rlibs.append(lib_file)
        direct_externs.append("%s=%s" % (crate_name, lib_file.path))

    trans_rlibs = []
    trans_externs = []

    direct_externs_list = []

    if hasattr(ctx.rule.attr, "deps"):
        for d in ctx.rule.attr.deps:
            if NetsimDepInfo in d:
                trans_rlibs.append(d[NetsimDepInfo].transitive_rlibs)
                trans_externs.append(d[NetsimDepInfo].transitive_externs)
                dep_info = d[NetsimDepInfo]
                if dep_info.crate_name and dep_info.rlib:
                    direct_externs_list.append("%s=%s" % (dep_info.crate_name, dep_info.rlib.path))

    # Proc-macros are built for the host (execution) platform.
    # We still need to pass them to clippy via --extern so it can expand macros.
    if hasattr(ctx.rule.attr, "proc_macro_deps"):
        for d in ctx.rule.attr.proc_macro_deps:
            if NetsimDepInfo in d:
                lib = d[NetsimDepInfo].rlib
                name = d[NetsimDepInfo].crate_name
                if lib and name:
                    direct_rlibs.append(lib)
                    direct_externs_list.append("%s=%s" % (name, lib.path))

    target_features = []
    if hasattr(ctx.rule.attr, "crate_features"):
        target_features = ctx.rule.attr.crate_features

    return [NetsimDepInfo(
        crate_name = crate_name,
        rlib = lib_file,
        direct_externs = depset(direct_externs_list),
        transitive_rlibs = depset(direct_rlibs, transitive = trans_rlibs),
        transitive_externs = depset(direct_externs, transitive = trans_externs),
        crate_features = target_features,
    )]

netsim_dep_aspect = aspect(
    implementation = _netsim_dep_aspect_impl,
    attr_aspects = ["deps", "proc_macro_deps"],
)

# Why we use a manual clippy-driver rule instead of rules_rust defaults:
#
# rules_rust `rust_clippy` and `rust_clippy_aspect` do not seamlessly integrate
# with the custom multi-target test reporting and diagnostics formatting required
# across platform toolchains. We drive clippy directly using the resolved rust toolchain.
def _netsim_clippy_test_impl(ctx):
    toolchain = ctx.toolchains["@rules_rust//rust:toolchain_type"]
    rustfmt = toolchain.rustfmt
    clippy_driver_path = toolchain.clippy_driver.path
    sysroot = toolchain.sysroot

    is_windows = toolchain.target_triple.system == "windows"
    target_triple = toolchain.target_triple
    if hasattr(target_triple, "str"):
        target_triple = target_triple.str

    # Create a wrapper to run clippy and capture output/status.
    if is_windows:
        action_wrapper = ctx.actions.declare_file(ctx.label.name + "_clippy_action.bat")
        ctx.actions.write(
            output = action_wrapper,
            content = "@echo off\r\n" +
                      "set DRIVER=%~1\r\n" +
                      "\"%DRIVER:/=\\%\" %~5 > \"%~2\" 2>&1\r\n" +
                      "echo %errorlevel% > \"%~3\"\r\n" +
                      "echo. > \"%~4\"\r\n" +
                      "exit /b 0\r\n",
        )
    else:
        action_wrapper = ctx.actions.declare_file(ctx.label.name + "_clippy_action.sh")
        ctx.actions.write(
            output = action_wrapper,
            is_executable = True,
            content = "#!/bin/bash\n" +
                      "DRIVER=$1; LOG=$2; STATUS=$3; SUCCESS=$4; PARAMS=$5\n" +
                      "\"$DRIVER\" \"$PARAMS\" > \"$LOG\" 2>&1\n" +
                      "echo $? > \"$STATUS\"\n" +
                      "touch \"$SUCCESS\"\n" +
                      "exit 0\n",
        )

    success_files = []
    log_files = []
    status_files = []
    target_names = []

    idx = 0
    for t in ctx.attr.targets:
        if rust_common.crate_info not in t:
            continue
        info = t[rust_common.crate_info]
        edition = info.edition
        root_file = info.root
        segments = root_file.short_path.split("/")
        if len(segments) > 0 and segments[0] == "..":
            segments = segments[2:]
        depth = len(segments) - 1
        target_out_dir = "/".join([".."] * depth) if depth > 0 else "."
        srcs = info.srcs.to_list()
        pkg_dir = root_file.dirname.rsplit("/", 1)[0]
        if any([not s.is_source and s.dirname == pkg_dir for s in srcs]):
            target_out_dir = ".."

        # Collect dependencies for THIS target t!
        t_rlibs = []
        t_externs = []
        if NetsimDepInfo in t:
            t_rlibs = t[NetsimDepInfo].transitive_rlibs.to_list()
            t_externs = t[NetsimDepInfo].direct_externs.to_list()

        lib_dirs = {}
        for rlib in t_rlibs:
            dirname = rlib.path.rsplit("/", 1)[0]
            lib_dirs[dirname] = True

        success_file = ctx.actions.declare_file("%s_%d.success" % (ctx.label.name, idx))
        log_file = ctx.actions.declare_file("%s_%d.log" % (ctx.label.name, idx))
        status_file = ctx.actions.declare_file("%s_%d.status" % (ctx.label.name, idx))

        success_files.append(success_file)
        log_files.append(log_file)
        status_files.append(status_file)
        target_names.append(t.label.name)

        compile_data = []
        if hasattr(info, "compile_data"):
            compile_data = info.compile_data.to_list()

        t_args = ctx.actions.args()
        t_args.use_param_file("@%s", use_always = True)
        t_args.set_param_file_format("multiline")

        for ext in t_externs:
            # Skip host-built rlibs to avoid E0464 "multiple candidates" conflicts with target rlibs.
            # We keep .so/.dylib from -exec as they contain required proc-macros.
            if "-exec" in ext and ".rlib" in ext:
                continue
            t_args.add("--extern=%s" % ext)
        for lib_dir in lib_dirs.keys():
            t_args.add("-L", "dependency=%s" % lib_dir)
        t_args.add("--sysroot=%s" % sysroot)
        t_args.add("-L", "%s/lib/rustlib/%s/lib" % (sysroot, target_triple))
        t_args.add_all(ctx.attr.clippy_flags)
        if NetsimDepInfo in t:
            for feat in t[NetsimDepInfo].crate_features:
                if feat != "testing":
                    t_args.add("--cfg", 'feature="%s"' % feat)
        if info.type == "bin" and not "test" in t.label.name:
            t_args.add("--crate-type=bin")
        elif info.type == "test" or "test" in t.label.name:
            t_args.add("--test")

            # In test targets, allow idiomatic test patterns:
            # - unwrap_in_result: tests returning Result often assert via .unwrap()
            # - unwrap_used: tests and test fixtures commonly use .unwrap()
            # - indexing_slicing: tests commonly index directly into slices/vecs in assertions
            # - field_reassign_with_default: initializing mock structs in tests
            # - module_inception: tests/tests.rs having mod tests
            t_args.add("-Aclippy::unwrap_in_result")
            t_args.add("-Aclippy::unwrap_used")
            t_args.add("-Aclippy::indexing_slicing")
            t_args.add("-Aclippy::field_reassign_with_default")
            t_args.add("-Aclippy::module_inception")
            t_args.add("-Aclippy::panic")
            t_args.add("-Aclippy::unreachable")
            t_args.add("-Aclippy::unimplemented")
            t_args.add("-Aclippy::todo")
            t_args.add("-Aclippy::panic_in_result_fn")
            t_args.add("-Aclippy::cast_possible_wrap")
            t_args.add("-Aclippy::print_stdout")
            t_args.add("-Aclippy::print_stderr")
            t_args.add("-Aclippy::unused_async")
        else:
            t_args.add("--crate-type=rlib")
        t_args.add("--crate-name=%s" % info.name)
        t_args.add("--edition=%s" % edition)
        t_args.add("--emit=metadata")
        t_args.add("-Cembed-bitcode=no")
        if not is_windows:
            t_args.add("--remap-path-prefix=$(pwd)=.")
        t_args.add(root_file.path)

        # We run clippy-driver via a wrapper that always exits 0, even if clippy fails.
        # We save the status code to file to read later. This is because we want clippy to
        # act as a test (run always and show errors) rather than a build failure that stops the build.
        ctx.actions.run(
            outputs = [success_file, log_file, status_file],
            inputs = depset(srcs + t_rlibs + compile_data + [rustfmt, action_wrapper], transitive = [toolchain.all_files]),
            executable = action_wrapper,
            arguments = [clippy_driver_path, log_file.path, status_file.path, success_file.path, t_args],
            env = {
                "CARGO_MANIFEST_DIR": root_file.dirname,
                "OUT_DIR": target_out_dir,
            },
            mnemonic = "NetsimClippy",
            progress_message = "Running Netsim Clippy on %s target %d" % (ctx.label.name, idx),
        )

        idx += 1

    if not success_files:
        fail("No valid Rust targets to lint found")

    extension = ".bat" if is_windows else ".sh"
    runner = ctx.actions.declare_file(ctx.label.name + extension)

    # Generate runner script to cat all logs and exit with combined status.
    # This allows the bazel test action to report the failure without failing the build itself.
    if is_windows:
        bat_content = "@echo off\r\n"
        bat_content += "set failed=0\r\n"
        for i in range(len(log_files)):
            bat_content += "echo --- Output for target %s ---\r\n" % target_names[i]
            bat_content += "type \"%s\"\r\n" % log_files[i].short_path.replace("/", "\\")
            bat_content += "set /p st_raw=<\"%s\"\r\n" % status_files[i].short_path.replace("/", "\\")
            bat_content += "set /a st=%st_raw%\r\n"
            bat_content += "if NOT %st% == 0 set failed=1\r\n"
        bat_content += "exit /b %failed%\r\n"
        content = bat_content
    else:
        bash_content = "#!/bin/bash\n"
        bash_content += "failed=0\n"
        for i in range(len(log_files)):
            bash_content += "echo \"--- Output for target %s ---\"\n" % target_names[i]
            bash_content += "cat %s\n" % log_files[i].short_path
            bash_content += "st=$(cat %s)\n" % status_files[i].short_path
            bash_content += "if [ \"$st\" -ne 0 ]; then failed=1; fi\n"
        bash_content += "exit $failed\n"
        content = bash_content

    ctx.actions.write(
        output = runner,
        is_executable = True,
        content = content,
    )

    return [DefaultInfo(
        executable = runner,
        runfiles = ctx.runfiles(files = log_files + success_files + status_files),
    )]

netsim_clippy_test = rule(
    implementation = _netsim_clippy_test_impl,
    attrs = {
        "targets": attr.label_list(
            providers = [[rust_common.crate_info]],
            aspects = [netsim_dep_aspect],
        ),
        "clippy_flags": attr.string_list(),
    },
    test = True,
    toolchains = ["@rules_rust//rust:toolchain_type"],
)

def _define_common_targets(name, enable_clippy, enable_rustfmt, targets = [], target_compatible_with = []):
    # Clippy Linter
    if enable_clippy:
        netsim_clippy_test(
            name = name + "_clippy",
            targets = targets,
            clippy_flags = NETSIM_CLIPPY_FLAGS,
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
        **kwargs
    )

    # 2. The Testing Library with "testing" feature (always created)
    testing_deps = testing_deps + [
        d.split(":")[0] + ":testing" if d.startswith("//next") else d
        for d in deps
    ]
    test_deps = [
        d.split(":")[0] + ":testing" if d.startswith("//next") else d
        for d in test_deps
    ]
    rust_library(
        name = "testing",
        srcs = srcs,
        deps = testing_deps + select_testing_deps,
        rustc_flags = rustc_flags,
        crate_features = ["testing"] + crate_features,
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

    # 1. The Main Binary
    rustc_flags = NETSIM_RUSTC_FLAGS + kwargs.pop("rustc_flags", [])

    rust_binary(
        name = name,
        srcs = srcs,
        deps = deps + select_deps,
        rustc_flags = rustc_flags,
        crate_features = crate_features,
        **kwargs
    )

    # 2. Common Targets (Clippy, Rustfmt)
    _define_common_targets(
        name,
        enable_clippy,
        enable_rustfmt,
        targets = [":" + name],
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
