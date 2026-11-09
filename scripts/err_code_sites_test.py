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

"""Unit tests for scripts/err_code_sites.py."""

import contextlib
import io
import pathlib
import tempfile
import unittest

import err_code_sites


def _sites(src: str) -> list[tuple[int, str | None]]:
  return err_code_sites.extract_error_call_sites(src)


def _variants_and_problems(src: str) -> tuple[dict[str, int], list[str]]:
  problems = []
  variants = err_code_sites.extract_code_enum_variants(
      src, problems=problems, crate="daemon"
  )
  return variants, problems


class TestMaskCommentsAndStrings(unittest.TestCase):

  def test_mask_line_comments(self):
    src = "let x = 1; // comment here\nlet y = 2;"
    masked = err_code_sites.mask_comments_and_strings(src)
    self.assertNotIn("comment here", masked)
    self.assertEqual(len(src), len(masked))
    self.assertEqual(src.splitlines()[1], masked.splitlines()[1])

  def test_mask_block_comments(self):
    src = "/* block\ncomment */let x = 1;"
    masked = err_code_sites.mask_comments_and_strings(src)
    self.assertNotIn("block", masked)
    self.assertNotIn("comment", masked)
    self.assertEqual(src.count("\n"), masked.count("\n"))
    self.assertTrue(masked.endswith("let x = 1;"))

  def test_mask_string_literals(self):
    src = 'let s = "string with // comment";'
    masked = err_code_sites.mask_comments_and_strings(src)
    self.assertNotIn("string with // comment", masked)
    self.assertEqual(len(src), len(masked))

  def test_mask_raw_strings(self):
    src = 'let s = r#"raw string with "quotes" and // comments"#;'
    masked = err_code_sites.mask_comments_and_strings(src)
    self.assertNotIn("raw string", masked)
    self.assertEqual(len(src), len(masked))


class TestStripCfgTest(unittest.TestCase):

  def test_standard_cfg_test_mod(self):
    src = (
        "fn real() {}\n"
        "#[cfg(test)]\n"
        "mod tests {\n"
        '    error!(code = Code::TestErr as u32, "test");\n'
        "}\n"
    )
    stripped = err_code_sites.strip_cfg_test(src)
    self.assertIn("fn real() {}", stripped)
    self.assertNotIn("TestErr", stripped)
    self.assertEqual(src.count("\n"), stripped.count("\n"))

  def test_cfg_test_with_allow_attr(self):
    src = (
        "#[cfg(test)]\n"
        "#[allow(clippy::unwrap_used)]\n"
        "mod tests {\n"
        '    warn!(code = Code::TestErr as u32, "test");\n'
        "}\n"
    )
    stripped = err_code_sites.strip_cfg_test(src)
    self.assertNotIn("TestErr", stripped)
    self.assertEqual(src.count("\n"), stripped.count("\n"))

  def test_cfg_test_pub_mod(self):
    src = (
        "#[cfg(test)]\n"
        "pub mod tests {\n"
        '    error!(code = Code::TestErr as u32, "test");\n'
        "}\n"
    )
    stripped = err_code_sites.strip_cfg_test(src)
    self.assertNotIn("TestErr", stripped)
    self.assertEqual(src.count("\n"), stripped.count("\n"))

  def test_cfg_test_standalone_fn(self):
    src = (
        "#[cfg(test)]\n"
        "fn helper() {\n"
        '    error!(code = Code::TestErr as u32, "test");\n'
        "}\n"
        "fn production() {}\n"
    )
    stripped = err_code_sites.strip_cfg_test(src)
    self.assertNotIn("TestErr", stripped)
    self.assertIn("fn production() {}", stripped)
    self.assertEqual(src.count("\n"), stripped.count("\n"))

  def test_cfg_test_standalone_fn_with_attributes(self):
    src = (
        "#[cfg(test)]\n"
        "#[allow(unused)]\n"
        "pub fn helper() {\n"
        '    error!(code = Code::TestErr as u32, "test");\n'
        "}\n"
    )
    stripped = err_code_sites.strip_cfg_test(src)
    self.assertNotIn("TestErr", stripped)
    self.assertEqual(src.count("\n"), stripped.count("\n"))


class TestExtractCodeEnumVariants(unittest.TestCase):

  def test_pub_crate_enum(self):
    src = "pub(crate) enum Code {\n    Alpha = 10001,\n    Beta = 10002,\n}\n"
    self.assertEqual(
        err_code_sites.extract_code_enum_variants(src),
        {"Alpha": 10001, "Beta": 10002},
    )

  def test_pub_enum(self):
    src = "pub enum Code {\n    WifiInit = 60001,\n}\n"
    self.assertEqual(
        err_code_sites.extract_code_enum_variants(src), {"WifiInit": 60001}
    )

  def test_digit_separators(self):
    src = "pub(crate) enum Code {\n    Alpha = 10_001,\n}\n"
    self.assertEqual(
        err_code_sites.extract_code_enum_variants(src), {"Alpha": 10001}
    )

  def test_final_variant_without_trailing_comma(self):
    src = "pub(crate) enum Code {\n    Alpha = 10001,\n    Beta = 10002\n}\n"
    self.assertEqual(
        err_code_sites.extract_code_enum_variants(src),
        {"Alpha": 10001, "Beta": 10002},
    )

  def test_ignores_commented_out_variant(self):
    src = (
        "pub(crate) enum Code {\n"
        "    // Ignored = 99999,\n"
        "    Alpha = 10_001,\n"
        "}\n"
    )
    self.assertEqual(
        err_code_sites.extract_code_enum_variants(src), {"Alpha": 10001}
    )

  def test_other_enum_yields_no_variants(self):
    src = "pub(crate) enum Other { Foo = 1 }"
    self.assertEqual(err_code_sites.extract_code_enum_variants(src), {})

  def test_missing_enum_flagged(self):
    _, problems = _variants_and_problems("pub(crate) enum Other { Foo = 1 }")
    self.assertEqual(
        problems,
        ["daemon: err.rs has no `pub(crate) enum Code` or `pub enum Code`"],
    )

  def test_empty_enum_flagged(self):
    variants, problems = _variants_and_problems("pub(crate) enum Code {}\n")
    self.assertEqual(variants, {})
    self.assertEqual(problems, ["daemon: empty `enum Code`"])

  def test_unclosed_enum_flagged(self):
    _, problems = _variants_and_problems(
        "pub(crate) enum Code {\n    Alpha = 10001,\n"
    )
    self.assertEqual(problems, ["daemon: unclosed `enum Code` body"])

  def test_missing_discriminant_flagged(self):
    variants, problems = _variants_and_problems(
        "pub enum Code {\n    Valid = 10001,\n    MissingValue,\n}\n"
    )
    self.assertEqual(variants, {"Valid": 10001})
    self.assertEqual(
        problems, ["daemon: unparseable enum Code line: MissingValue,"]
    )

  def test_inline_attribute_missing_discriminant_flagged(self):
    _, problems = _variants_and_problems(
        "pub(crate) enum Code {\n    #[cfg(unix)] MissingInline,\n}\n"
    )
    self.assertEqual(
        problems, ["daemon: unparseable enum Code line: MissingInline,"]
    )

  def test_non_integer_discriminant_flagged(self):
    _, problems = _variants_and_problems(
        "pub(crate) enum Code {\n    NonInteger = BAR,\n}\n"
    )
    self.assertEqual(
        problems, ["daemon: unparseable enum Code line: NonInteger = BAR,"]
    )

  def test_underscore_only_discriminant_flagged(self):
    variants, problems = _variants_and_problems(
        "pub(crate) enum Code {\n    Foo = _,\n}\n"
    )
    self.assertEqual(variants, {})
    self.assertEqual(problems, ["daemon: unparseable enum Code line: Foo = _,"])

  def test_leading_underscore_discriminant_flagged(self):
    variants, problems = _variants_and_problems(
        "pub(crate) enum Code {\n    Foo = _10001,\n}\n"
    )
    self.assertEqual(variants, {})
    self.assertEqual(
        problems, ["daemon: unparseable enum Code line: Foo = _10001,"]
    )

  def test_inner_brace_variant_keeps_following_variants(self):
    variants, _ = _variants_and_problems(
        "pub(crate) enum Code {\n"
        "    Bad { x: u32 } = 10001,\n"
        "    Good = 10002,\n"
        "}\n"
    )
    self.assertEqual(variants, {"Good": 10002})

  def test_inner_brace_variant_flagged(self):
    _, problems = _variants_and_problems(
        "pub(crate) enum Code {\n"
        "    Bad { x: u32 } = 10001,\n"
        "    Good = 10002,\n"
        "}\n"
    )
    self.assertEqual(
        problems,
        ["daemon: unparseable enum Code line: Bad { x: u32 } = 10001,"],
    )

  def test_duplicate_name_flagged(self):
    variants, problems = _variants_and_problems(
        "pub enum Code {\n    Duplicate = 10001,\n    Duplicate = 10002,\n}\n"
    )
    self.assertEqual(variants, {"Duplicate": 10002})
    self.assertEqual(
        problems, ["daemon: duplicate variant name in enum Code: Duplicate"]
    )

  def test_skips_cfg_and_allow_attributes(self):
    variants, problems = _variants_and_problems(
        "#![allow(non_camel_case_types)]\n"
        "pub(crate) enum Code {\n"
        "    #[cfg(unix)]\n"
        "    UnixOnly = 10001,\n"
        "    #[cfg(windows)]\n"
        "    WindowsOnly = 10002,\n"
        '    #[cfg(all(target_os = "linux", feature = "cuttlefish"))]\n'
        "    LinuxCuttlefish = 10003,\n"
        "    #[allow(dead_code)]\n"
        "    AllowedVariant = 10004,\n"
        "}\n"
    )
    self.assertEqual(problems, [])
    self.assertEqual(
        variants,
        {
            "UnixOnly": 10001,
            "WindowsOnly": 10002,
            "LinuxCuttlefish": 10003,
            "AllowedVariant": 10004,
        },
    )

  def test_skips_inline_and_multiline_attributes(self):
    variants, problems = _variants_and_problems(
        "pub(crate) enum Code {\n"
        "    #[cfg(unix)] InlineUnix = 10001,\n"
        "    #[cfg(windows)] #[allow(unused)] InlineWin = 10002,\n"
        "    #[cfg(all(\n"
        '        target_os = "linux",\n'
        '        feature = "cuttlefish",\n'
        "    ))]\n"
        "    MultilineCfg = 10003,\n"
        '    #[doc(alias = ["a", "b"])]\n'
        "    NestedBrackets = 10004,\n"
        "}\n"
    )
    self.assertEqual(problems, [])
    self.assertEqual(
        variants,
        {
            "InlineUnix": 10001,
            "InlineWin": 10002,
            "MultilineCfg": 10003,
            "NestedBrackets": 10004,
        },
    )

  def test_deprecated_note_form_recorded(self):
    deprecated = set()
    err_code_sites.extract_code_enum_variants(
        "pub(crate) enum Code {\n"
        '    #[deprecated(note = "retired in b/123")]\n'
        "    Old = 10002,\n"
        "    Live = 10001,\n"
        "}\n",
        deprecated=deprecated,
    )
    self.assertEqual(deprecated, {"Old"})

  def test_stacked_cfg_before_deprecated_recorded(self):
    deprecated = set()
    err_code_sites.extract_code_enum_variants(
        "pub(crate) enum Code {\n"
        "    #[cfg(unix)]\n"
        "    #[deprecated]\n"
        "    Old = 10002,\n"
        "    Live = 10001,\n"
        "}\n",
        deprecated=deprecated,
    )
    self.assertEqual(deprecated, {"Old"})

  def test_inner_attribute_inside_enum_flagged(self):
    _, problems = _variants_and_problems(
        "pub(crate) enum Code {\n    #![allow(unused)]\n    Alpha = 10001,\n}\n"
    )
    self.assertEqual(
        problems, ["daemon: unparseable enum Code line: #![allow(unused)]"]
    )

  def test_trailing_attribute_without_variant_rejected(self):
    _, problems = _variants_and_problems(
        "pub(crate) enum Code {\n    Foo = 10001,\n    #[deprecated]\n}\n"
    )
    self.assertEqual(
        problems,
        ["daemon: trailing attribute without variant in `enum Code`"],
    )

  def test_deprecated_key_value_form_recorded(self):
    deprecated = set()
    err_code_sites.extract_code_enum_variants(
        "pub(crate) enum Code {\n"
        '    #[deprecated = "retired"]\n'
        "    Old = 10001,\n"
        "}\n",
        deprecated=deprecated,
    )
    self.assertEqual(deprecated, {"Old"})

  def test_deprecated_followed_by_allow_dead_code_recorded(self):
    deprecated = set()
    err_code_sites.extract_code_enum_variants(
        "pub(crate) enum Code {\n"
        "    #[deprecated]\n"
        "    #[allow(dead_code)]\n"
        "    Old = 10001,\n"
        "}\n",
        deprecated=deprecated,
    )
    self.assertEqual(deprecated, {"Old"})

  def test_inline_deprecated_marks_only_its_variant(self):
    deprecated = set()
    err_code_sites.extract_code_enum_variants(
        "pub(crate) enum Code {\n"
        "    #[deprecated] Old = 10002,\n"
        "    Live = 10001,\n"
        "}\n",
        deprecated=deprecated,
    )
    self.assertEqual(deprecated, {"Old"})


class TestExtractErrorCallSites(unittest.TestCase):

  def test_tagged_site(self):
    src = 'fn run() {\n    error!(code = Code::MyErr as u32, "an error");\n}\n'
    self.assertEqual(_sites(src), [(2, "MyErr")])

  def test_untagged_site(self):
    src = 'fn run() {\n    warn!("untagged warning");\n}\n'
    self.assertEqual(_sites(src), [(2, None)])

  def test_cfg_test_site_excluded(self):
    src = (
        "#[cfg(test)]\n"
        "mod tests {\n"
        '    error!(code = Code::TestOnly as u32, "test");\n'
        "}\n"
        'fn run() { warn!(code = Code::Prod as u32, "p"); }\n'
    )
    self.assertEqual(_sites(src), [(5, "Prod")])

  def test_err_module_prefix(self):
    self.assertEqual(
        _sites('error!(code = err::Code::Variant as u32, "m");'),
        [(1, "Variant")],
    )

  def test_crate_err_module_prefix(self):
    self.assertEqual(
        _sites('error!(code = crate::err::Code::Variant as u32, "m");'),
        [(1, "Variant")],
    )

  def test_super_err_module_prefix(self):
    self.assertEqual(
        _sites('error!(code = super::err::Code::Variant as u32, "m");'),
        [(1, "Variant")],
    )

  def test_other_crate_err_path_not_attributed(self):
    self.assertEqual(
        _sites('error!(code = netsim_common::err::Code::X as u32, "m");'),
        [(1, None)],
    )

  def test_missing_as_u32_not_attributed(self):
    self.assertEqual(_sites('error!(code = Code::Variant, "m");'), [(1, None)])

  def test_close_paren_char_in_args(self):
    self.assertEqual(
        _sites("error!(delim = ')', code = Code::Paren as u32, \"m\");"),
        [(1, "Paren")],
    )

  def test_close_paren_string_in_args(self):
    self.assertEqual(
        _sites('error!(d = "has ) paren", code = Code::Paren as u32, "m");'),
        [(1, "Paren")],
    )

  def test_code_tag_inside_string_not_attributed(self):
    self.assertEqual(_sites('warn!("code = Code::Fake as u32");'), [(1, None)])

  def test_open_brace_char_in_cfg_test_body(self):
    src = (
        "#[cfg(test)]\n"
        "mod tests {\n"
        "    fn t() { error!(open = '{', code = Code::T as u32, \"m\"); }\n"
        "}\n"
        'fn run() { warn!(code = Code::Prod as u32, "p"); }\n'
    )
    self.assertEqual(_sites(src), [(5, "Prod")])

  def test_open_brace_string_in_cfg_test_body(self):
    src = (
        "#[cfg(test)]\n"
        "mod tests {\n"
        '    fn t() { error!(s = "has { brace", code = Code::T as u32); }\n'
        "}\n"
        'fn run() { warn!(code = Code::Prod as u32, "p"); }\n'
    )
    self.assertEqual(_sites(src), [(5, "Prod")])

  def test_cfg_test_trait_method_declaration_preserves_following_fn(self):
    src = (
        "trait T {\n"
        "    #[cfg(test)]\n"
        "    fn helper(&self);\n"
        "    fn real(&self) {\n"
        '        error!(code = Code::Real as u32, "boom");\n'
        "    }\n"
        "}\n"
    )
    self.assertEqual(_sites(src), [(5, "Real")])

  def test_cfg_test_async_fn_excluded(self):
    src = '#[cfg(test)]\nasync fn t() {\n    error!("ignored");\n}\n'
    self.assertEqual(_sites(src), [])

  def test_cfg_test_unsafe_fn_excluded(self):
    src = '#[cfg(test)]\nunsafe fn t() { error!("ignored"); }\n'
    self.assertEqual(_sites(src), [])

  def test_lifetimes_and_loop_labels_preserve_char_masking(self):
    src = (
        "fn f<'a>(x: &'a u8) { 'l: loop { break 'l; } let c = ')';"
        ' error!(code = Code::Y as u32, "ok"); }'
    )
    self.assertEqual(_sites(src), [(1, "Y")])


class TestCheckTree(unittest.TestCase):

  def setUp(self):
    super().setUp()
    tmp = tempfile.TemporaryDirectory()
    self.addCleanup(tmp.cleanup)
    self.root = pathlib.Path(tmp.name)

  def _write(self, crate: str, name: str, text: str):
    crate_dir = self.root / "src" / crate / "src"
    crate_dir.mkdir(parents=True, exist_ok=True)
    (crate_dir / name).write_text(text, encoding="utf-8")

  def _populate_crate(
      self,
      crate: str,
      variants: dict[str, int],
      sites: list[str],
      visibility: str = "pub(crate)",
  ):
    lines = "".join(f"    {k} = {v},\n" for k, v in variants.items())
    self._write(crate, "err.rs", f"{visibility} enum Code {{\n{lines}}}\n")
    calls = "".join(
        f'    error!(code = Code::{name} as u32, "msg");\n' for name in sites
    )
    self._write(crate, "lib.rs", f"fn foo() {{\n{calls}}}\n")

  def _main(self, *args: str) -> tuple[int, str, str]:
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
      rc = err_code_sites.main(["--src-dir", str(self.root), *args])
    return rc, out.getvalue(), err.getvalue()

  def test_clean_tree(self):
    self._populate_crate(
        "daemon", {"First": 10001, "Second": 10002}, ["First", "Second"]
    )
    self.assertEqual(err_code_sites.check_tree(self.root), [])

  def test_main_check_clean_tree_exits_zero(self):
    self._populate_crate("daemon", {"First": 10001}, ["First"])
    rc, _, err = self._main("--check")
    self.assertEqual(rc, 0)
    self.assertEqual(err, "")

  def test_main_check_reports_problems_on_stderr(self):
    self._populate_crate("daemon", {"First": 10001, "Unused": 10002}, ["First"])
    rc, _, err = self._main("--check")
    self.assertEqual(rc, 1)
    self.assertEqual(err, "daemon::Unused defined but has no call site\n")

  def test_main_without_check_exits_zero_despite_problems(self):
    self._populate_crate("daemon", {"First": 10001, "Unused": 10002}, ["First"])
    rc, _, err = self._main()
    self.assertEqual(rc, 0)
    self.assertEqual(err, "")

  def test_main_verbose_lists_site_locations(self):
    self._populate_crate("daemon", {"First": 10001}, ["First"])
    _, out, _ = self._main("--verbose")
    self.assertIn("  1  daemon::First\n       daemon/src/lib.rs:2\n", out)

  def test_band_hole_detected(self):
    self._populate_crate(
        "daemon", {"First": 10001, "Third": 10003}, ["First", "Third"]
    )
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        [
            "daemon: band 10000 codes [10001, 10003] are not contiguous from"
            " 10001 (expected 10001..=10002)"
        ],
    )

  def test_out_of_range_code_rejected(self):
    self._populate_crate("daemon", {"InvalidRange": 25000}, ["InvalidRange"])
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        [
            "daemon::InvalidRange (25000) outside 10001..=19999 sub-band"
            " +000..+499"
        ],
    )

  def test_non_base_band_may_start_at_xn001(self):
    self._populate_crate(
        "packet-stream",
        {"Base1": 30001, "Base2": 30002, "Cat1": 32001, "Cat2": 32002},
        ["Base1", "Base2", "Cat1", "Cat2"],
    )
    self.assertEqual(err_code_sites.check_tree(self.root), [])

  def test_non_base_band_hole_detected(self):
    self._populate_crate(
        "packet-stream",
        {"Base1": 30001, "Cat1": 32001, "Cat3": 32003},
        ["Base1", "Cat1", "Cat3"],
    )
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        [
            "packet-stream: band 32000 codes [32001, 32003] are not"
            " contiguous from 32001 (expected 32001..=32002)"
        ],
    )

  def test_non_base_band_starting_at_xn000_accepted(self):
    self._populate_crate(
        "packet-stream",
        {"Base1": 30001, "Cat0": 32000, "Cat1": 32001},
        ["Base1", "Cat0", "Cat1"],
    )
    self.assertEqual(err_code_sites.check_tree(self.root), [])

  def test_non_base_band_starting_above_xn001_rejected(self):
    self._populate_crate(
        "packet-stream",
        {"Base1": 30001, "Cat2": 32002, "Cat3": 32003},
        ["Base1", "Cat2", "Cat3"],
    )
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        [
            "packet-stream: band 32000 codes [32002, 32003] are not"
            " contiguous from 32000 (expected 32000..=32001)"
        ],
    )

  def test_non_crate_default_at_base_lo_rejected(self):
    self._populate_crate("daemon", {"ZeroBase": 10000}, ["ZeroBase"])
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        ["daemon::ZeroBase (10000) outside 10001..=19999 sub-band +000..+499"],
    )

  def test_cfg_attributes_in_enum_code_accepted(self):
    self._write(
        "daemon",
        "err.rs",
        "pub(crate) enum Code {\n"
        "    #[cfg(unix)]\n"
        "    UnixCode = 10001,\n"
        "    #[cfg(windows)]\n"
        "    WinCode = 10002,\n"
        '    #[cfg(all(target_os = "linux", feature = "cuttlefish"))]\n'
        "    LinuxCfCode = 10003,\n"
        "    #[cfg(unix)] InlineCode = 10004,\n"
        "    #[cfg(all(\n"
        '        target_os = "linux",\n'
        '        feature = "cuttlefish",\n'
        "    ))]\n"
        "    MultilineCode = 10005,\n"
        "}\n",
    )
    self._write(
        "daemon",
        "lib.rs",
        "fn run() {\n"
        '    error!(code = Code::UnixCode as u32, "unix");\n'
        '    error!(code = Code::WinCode as u32, "win");\n'
        '    error!(code = Code::LinuxCfCode as u32, "cf");\n'
        '    warn!(code = Code::InlineCode as u32, "inline");\n'
        '    warn!(code = Code::MultilineCode as u32, "multiline");\n'
        "}\n",
    )
    self.assertEqual(err_code_sites.check_tree(self.root), [])

  def test_companion_crate_rejects_owner_offset(self):
    self._populate_crate("common", {"WrongSubBand": 10001}, ["WrongSubBand"])
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        [
            "common::WrongSubBand (10001) outside 10000..=19999 sub-band"
            " +500..+999"
        ],
    )

  def test_companion_crate_rejects_above_sub_hi(self):
    self._populate_crate(
        "actor-framework", {"AboveSubHi": 20800}, ["AboveSubHi"]
    )
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        [
            "actor-framework::AboveSubHi (20800) outside 20000..=29999"
            " sub-band +500..+799"
        ],
    )

  def test_companion_crate_sub_band_rejects_non_sub_lo_start(self):
    self._populate_crate("common", {"SkippedStart": 10501}, ["SkippedStart"])
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        [
            "common: band 10000 codes [10501] are not contiguous from 10500"
            " (expected 10500..=10500)"
        ],
    )

  def test_companion_crate_sub_band_accepts_sub_lo_start(self):
    self._populate_crate(
        "common", {"First": 10500, "Second": 10501}, ["First", "Second"]
    )
    self.assertEqual(err_code_sites.check_tree(self.root), [])

  def test_model_accepts_sub_lo_start(self):
    self._populate_crate("model", {"ChipMismatch": 36500}, ["ChipMismatch"])
    self.assertEqual(err_code_sites.check_tree(self.root), [])

  def test_model_rejects_below_sub_lo(self):
    self._populate_crate("model", {"Below": 36499}, ["Below"])
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        ["model::Below (36499) outside 30000..=39999 sub-band +500..+999"],
    )

  def test_wifi_actor_pub_enum_accepted(self):
    self._populate_crate(
        "wifi-actor", {"WifiInit": 60001}, ["WifiInit"], visibility="pub"
    )
    self.assertEqual(err_code_sites.check_tree(self.root), [])

  def test_full_band_crate_out_of_range_omits_sub_band(self):
    self._populate_crate(
        "wifi-actor", {"Stray": 70001}, ["Stray"], visibility="pub"
    )
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        ["wifi-actor::Stray (70001) outside 60001..=69999"],
    )

  def test_ap_actor_pub_enum_accepted(self):
    self._populate_crate(
        "ap-actor", {"ApInit": 70001}, ["ApInit"], visibility="pub"
    )
    self.assertEqual(err_code_sites.check_tree(self.root), [])

  def test_crate_default_defined_in_err_rs_rejected(self):
    self._populate_crate(
        "daemon", {"CrateDefault": 10000, "Valid": 10001}, ["Valid"]
    )
    self.assertIn(
        "daemon::CrateDefault is forbidden in err.rs",
        err_code_sites.check_tree(self.root),
    )

  def test_crate_default_used_at_call_site_rejected(self):
    self._write(
        "daemon", "err.rs", "pub(crate) enum Code {\n    Valid = 10001,\n}\n"
    )
    self._write(
        "daemon",
        "lib.rs",
        "fn run() {\n"
        '    error!(code = Code::Valid as u32, "ok");\n'
        '    warn!(code = Code::CrateDefault as u32, "bad");\n'
        "}\n",
    )
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        ["daemon: tagged site uses CrateDefault: ['daemon/src/lib.rs:3']"],
    )

  def test_untagged_site_in_crate_with_err_rs_rejected(self):
    self._write(
        "daemon", "err.rs", "pub(crate) enum Code {\n    Valid = 10001,\n}\n"
    )
    self._write(
        "daemon",
        "lib.rs",
        "fn run() {\n"
        '    error!(code = Code::Valid as u32, "ok");\n'
        '    warn!("untagged");\n'
        "}\n",
    )
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        ["daemon/src/lib.rs:3: untagged error!/warn! site"],
    )

  def test_untagged_site_in_crate_without_err_rs_allowed(self):
    self._write("tools", "lib.rs", 'fn run() {\n    warn!("untagged");\n}\n')
    self.assertEqual(err_code_sites.check_tree(self.root), [])

  def test_site_naming_undefined_variant_rejected(self):
    self._write(
        "daemon", "err.rs", "pub(crate) enum Code {\n    Valid = 10001,\n}\n"
    )
    self._write(
        "daemon",
        "lib.rs",
        "fn run() {\n"
        '    error!(code = Code::Valid as u32, "ok");\n'
        '    error!(code = Code::Missing as u32, "bad");\n'
        "}\n",
    )
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        [
            "daemon::Missing not defined in daemon/src/err.rs:"
            " ['daemon/src/lib.rs:3']"
        ],
    )

  def test_code_shared_by_two_sites_rejected(self):
    self._populate_crate("daemon", {"First": 10001}, ["First", "First"])
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        [
            "daemon::First shared by 2 sites:"
            " ['daemon/src/lib.rs:2', 'daemon/src/lib.rs:3']"
        ],
    )

  def test_allowed_shared_code_accepted(self):
    self._populate_crate(
        "daemon",
        {"IniLockIdentityFailed": 10001},
        ["IniLockIdentityFailed", "IniLockIdentityFailed"],
    )
    self.assertEqual(err_code_sites.check_tree(self.root), [])

  def test_unused_code_rejected(self):
    self._populate_crate("daemon", {"First": 10001, "Unused": 10002}, ["First"])
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        ["daemon::Unused defined but has no call site"],
    )

  def test_allow_deprecated_does_not_exempt_unused_variant(self):
    self._write(
        "daemon",
        "err.rs",
        "pub(crate) enum Code {\n"
        "    First = 10001,\n"
        "    #[allow(deprecated)]\n"
        "    Unused = 10002,\n"
        "}\n",
    )
    self._write(
        "daemon",
        "lib.rs",
        'fn run() {\n    error!(code = Code::First as u32, "a");\n}\n',
    )
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        ["daemon::Unused defined but has no call site"],
    )

  def test_deprecated_mid_band_variant_without_call_site_accepted(self):
    self._write(
        "daemon",
        "err.rs",
        "pub(crate) enum Code {\n"
        "    First = 10001,\n"
        "    #[deprecated]\n"
        "    #[allow(dead_code)]\n"
        "    RetiredMid = 10002,\n"
        "    Third = 10003,\n"
        "}\n",
    )
    self._write(
        "daemon",
        "lib.rs",
        "fn run() {\n"
        '    error!(code = Code::First as u32, "a");\n'
        '    error!(code = Code::Third as u32, "c");\n'
        "}\n",
    )
    self.assertEqual(err_code_sites.check_tree(self.root), [])

  def test_deprecated_variant_used_at_call_site_rejected(self):
    self._write(
        "daemon",
        "err.rs",
        "pub(crate) enum Code {\n"
        "    First = 10001,\n"
        "    #[deprecated]\n"
        "    RetiredMid = 10002,\n"
        "}\n",
    )
    self._write(
        "daemon",
        "lib.rs",
        "fn run() {\n"
        '    error!(code = Code::First as u32, "a");\n'
        '    error!(code = Code::RetiredMid as u32, "x");\n'
        "}\n",
    )
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        [
            "daemon::RetiredMid (10002) is #[deprecated] in err.rs but still"
            " used at daemon/src/lib.rs:3"
        ],
    )

  def test_duplicate_numeric_code_across_crates_rejected(self):
    self._populate_crate("daemon", {"First": 10001}, ["First"])
    self._populate_crate("common", {"Clash": 10001}, ["Clash"])
    self.assertIn(
        "code 10001 defined more than once: ['common::Clash', 'daemon::First']",
        err_code_sites.check_tree(self.root),
    )

  def test_err_rs_without_enum_code_rejected(self):
    self._write("daemon", "err.rs", "pub(crate) enum Other {}\n")
    self._write("daemon", "lib.rs", "fn run() {}\n")
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        ["daemon: err.rs has no `pub(crate) enum Code` or `pub enum Code`"],
    )

  def test_crate_missing_from_crate_ranges_rejected(self):
    self._populate_crate("unknown-crate", {"First": 10001}, ["First"])
    self.assertEqual(
        err_code_sites.check_tree(self.root),
        ["unknown-crate: no CRATE_RANGES entry"],
    )


if __name__ == "__main__":
  unittest.main()
