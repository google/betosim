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

"""Reports how many error!/warn! call sites use each typed `err::Code` variant.

Standalone manual developer audit (`python3 scripts/err_code_sites.py --check`).
Bazel, Soong and presubmit builds do not run `--check`. Use it during rollout
and reviews to verify call-site uniqueness, `CrateDefault` non-use, per-crate
sub-band ranges, dense sub-band contiguity, and cross-crate numeric uniqueness.

Parses `pub(crate) enum Code { Variant = N, ... }` (or `pub enum Code`) from
each `src/*/src/err.rs` and scans `src/*/src/**/*.rs` (excluding comments,
string/char literals, and `#[cfg(test)]` modules and functions) for
`error!(..., code = err::Code::Variant as u32, ...)` and the matching `warn!`
form, printing one row per code with its site count.
With `--check`, exits non-zero if:
  * a code is used at more than one call site (outside ALLOWED_SHARED),
  * `CrateDefault` is defined in `err.rs` or used at a call site,
  * a tagged site names a variant missing from its crate's `err.rs`,
  * a crate with an `err.rs` has an untagged `error!` / `warn!` site,
  * an `err.rs` has no `pub(crate) enum Code` or `pub enum Code`, has an
    empty or unclosed one, or its crate lacks a CRATE_RANGES entry,
  * a code is outside its crate's sub-band in CRATE_RANGES,
  * codes within any 1000-code sub-band of a crate have gaps or wrong start,
  * two `err.rs` modules define the same numeric code,
  * a code defined in `err.rs` has no call site, or
  * a `#[deprecated]` code is still used at a call site.
Retired `#[deprecated]` variants stay in `err.rs` so their numbers keep
sub-bands contiguous and are never reused; they need no call site. In a
`pub(crate) enum Code`, also add `#[allow(dead_code)]`, or `-D warnings`
rejects the never-constructed variant.

Known static-scanner limits: out-of-line `#[cfg(test)] mod tests;`,
`#[cfg(all(test, ...))]`, `#[cfg(test)] impl`, nested `/* /* ... */ */` block
comments, nested brackets inside stacked attributes on `#[cfg(test)]` items,
`#[cfg(test)] fn` signatures containing `{` (e.g. const-generic `{N}`),
`#[cfg_attr(..., deprecated)]` variants, and brace or bracket macro forms
(`error!{...}`, `error![...]`) are not handled.

Usage: scripts/err_code_sites.py [--check] [--verbose] [--src-dir <dir>]
"""

import argparse
import collections
import pathlib
import re
import sys

DEFAULT_REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent

# Codes intentionally shared by multiple call sites, with justification.
ALLOWED_SHARED = {
    # cfg(unix) and cfg(windows) twins of the same lock-identity query; only
    # one of the two is compiled on any platform.
    ("daemon", "IniLockIdentityFailed"),
}

# (base_lo, base_hi, sub_lo, sub_hi) each crate's `err.rs` may use.
# `X0000` is reserved for `err_bases` untyped fallback (`val > base_lo` for
# primary subsystem crates); within each 1000-code band, `sub_lo <= val % 1000
# <= sub_hi` prevents companion crates from overlapping owner sub-bands.
CRATE_RANGES = {
    "daemon": (10000, 19999, 0, 499),
    "common": (10000, 19999, 500, 999),
    "device-actor": (20000, 29999, 0, 499),
    "actor-framework": (20000, 29999, 500, 799),
    "link-actor": (20000, 29999, 800, 999),
    "packet-stream": (30000, 39999, 0, 499),
    "model": (30000, 39999, 500, 999),
    "wifi-actor": (60000, 69999, 0, 999),
    "ap-actor": (70000, 79999, 0, 999),
}

MACRO_START_RE = re.compile(r"\b(?:error|warn)!\s*\(")
SITE_CODE_RE = re.compile(
    r"\bcode\s*=\s*(?:(?:(?:crate|super)::)?err::)?Code::(\w+)\s+as\s+u32\b"
)
ENUM_HEAD_RE = re.compile(r"\bpub(?:\(crate\))?\s+enum\s+Code\s*\{")
VARIANT_RE = re.compile(r"^\s*(\w+)\s*=\s*(\d[\d_]*)\s*,?\s*$")
ATTR_START_RE = re.compile(r"#\s*\[")
DEPRECATED_ATTR_RE = re.compile(r"^\s*deprecated(?:\s*$|\s*[=(])")
CFG_TEST_RE = re.compile(
    r"#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?"
    r"(?:mod\s+\w+|(?:(?:const|async|unsafe)\s+)*fn\s+\w+[^{;]*)\s*\{"
)
COMMENT_OR_STR_RE = re.compile(
    r'(?:b|c)?r(#*)".*?"\1'
    r'|(?:b|c)?"(?:\\.|[^"\\])*"'
    r"|b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,6}\}|.)|[^'\\\n])'"
    r"|(//[^\n]*)"
    r"|(/\*[\s\S]*?\*/)",
    re.S,
)


def mask_comments_and_strings(text: str) -> str:
  """Blanks comments and Rust string/char literals, preserving newlines."""
  return COMMENT_OR_STR_RE.sub(
      lambda m: re.sub(r"[^\n]", " ", m.group(0)),
      text,
  )


def find_closing(masked: str, start: int, open_ch: str, close_ch: str) -> int:
  """Returns the index just past the delimiter closing an open one at `start`.

  `masked` must already have comments and literals blanked. Returns -1 if the
  delimiter is never closed.
  """
  depth = 1
  i = start
  while i < len(masked):
    if masked[i] == open_ch:
      depth += 1
    elif masked[i] == close_ch:
      depth -= 1
      if not depth:
        return i + 1
    i += 1
  return -1


def strip_cfg_test(text: str) -> str:
  """Blanks out `#[cfg(test)]` modules and functions, preserving newlines."""
  masked = mask_comments_and_strings(text)
  out = []
  pos = 0
  for m in CFG_TEST_RE.finditer(masked):
    if m.start() < pos:
      continue
    i = find_closing(masked, m.end(), "{", "}")
    if i < 0:
      i = len(masked)
    out.append(text[pos : m.start()])
    out.append(re.sub(r"[^\n]", " ", text[m.start() : i]))
    pos = i
  out.append(text[pos:])
  return "".join(out)


def in_crate_range(crate: str, val: int) -> bool:
  """Returns whether `val` lies in `crate`'s allocated sub-band."""
  spec = CRATE_RANGES.get(crate)
  if spec is None:
    return False
  base_lo, base_hi, sub_lo, sub_hi = spec
  return base_lo < val <= base_hi and sub_lo <= (val % 1000) <= sub_hi


def iter_log_macro_calls(text: str):
  """Yields `(line, args_text)` for each `error!(...)` / `warn!(...)` call."""
  masked = mask_comments_and_strings(text)
  for m in MACRO_START_RE.finditer(masked):
    i = find_closing(masked, m.end(), "(", ")")
    end = len(masked) if i < 0 else i - 1
    line = masked.count("\n", 0, m.start()) + 1
    yield line, masked[m.end() : end]


def extract_code_enum_variants(
    text: str,
    problems: list[str] | None = None,
    crate: str = "",
    deprecated: set[str] | None = None,
) -> dict[str, int]:
  """Parses `pub(crate) enum Code` or `pub enum Code` variants from Rust source.

  Returns a dict mapping variant name to integer code value. Skips outer Rust
  attributes (`#[...]`). If `deprecated` is provided, adds each variant marked
  with a `#[deprecated]` attribute. If `problems` is provided, appends a
  problem for a missing, unclosed or empty `enum Code`, for unparseable lines,
  for duplicate variant names, and for attributes with no following variant.
  """
  prefix = f"{crate}: " if crate else ""
  report = problems.append if problems is not None else lambda _: None
  masked = mask_comments_and_strings(text)
  m = ENUM_HEAD_RE.search(masked)
  if not m:
    report(f"{prefix}err.rs has no `pub(crate) enum Code` or `pub enum Code`")
    return {}
  end = find_closing(masked, m.end(), "{", "}")
  if end < 0:
    report(f"{prefix}unclosed `enum Code` body")
    return {}
  raw_body = masked[m.end() : end - 1]

  chars = list(raw_body)
  events = []
  pos = 0
  for am in ATTR_START_RE.finditer(raw_body):
    if am.start() < pos:
      continue
    close = find_closing(raw_body, am.end(), "[", "]")
    if close < 0:
      continue
    pos = close
    for k in range(am.start(), close):
      if chars[k] != "\n":
        chars[k] = " "
    attr_body = raw_body[am.end() : close - 1]
    is_deprecated = bool(DEPRECATED_ATTR_RE.match(attr_body))
    events.append((am.start(), "attr", is_deprecated))
  body = "".join(chars)
  offset = 0
  for line in body.splitlines(keepends=True):
    stripped = line.strip()
    if stripped:
      events.append((offset + line.index(stripped[0]), "line", stripped))
    offset += len(line)
  events.sort(key=lambda e: e[0])

  variants = {}
  unparseable = 0
  pending_attrs = 0
  pending_deprecated = False
  trailing_msg = f"{prefix}trailing attribute without variant in `enum Code`"
  for _, kind, value in events:
    if kind == "attr":
      pending_attrs += 1
      pending_deprecated = pending_deprecated or value
      continue
    vm = VARIANT_RE.match(value)
    if not vm:
      unparseable += 1
      report(f"{prefix}unparseable enum Code line: {value}")
    else:
      name, val_str = vm.group(1), vm.group(2)
      if name in variants:
        report(f"{prefix}duplicate variant name in enum Code: {name}")
      variants[name] = int(val_str.replace("_", ""))
      if pending_deprecated and deprecated is not None:
        deprecated.add(name)
    pending_attrs = 0
    pending_deprecated = False
  if pending_attrs:
    report(trailing_msg)
  if not variants and not unparseable:
    report(f"{prefix}empty `enum Code`")
  return variants


def extract_error_call_sites(text: str) -> list[tuple[int, str | None]]:
  """Returns `(line, code_name)` for each error!/warn! call site.

  `code_name` is None for untagged sites. Skips `#[cfg(test)]` items.
  """
  stripped = strip_cfg_test(text)
  results = []
  for line, call_args in iter_log_macro_calls(stripped):
    cm = SITE_CODE_RE.search(call_args)
    results.append((line, cm.group(1) if cm else None))
  return results


def scan_tree(
    base_dir: pathlib.Path,
) -> tuple[
    list[str],
    dict[tuple[str, str], list[str]],
    dict[str, dict[str, int]],
]:
  """Scans a crate source tree and checks code uniqueness, density and ranges.

  Returns `(problems, sites, defined)`.
  """
  src_dir = base_dir / "src" if (base_dir / "src").is_dir() else base_dir
  defined = {}
  deprecated = collections.defaultdict(set)
  owners = collections.defaultdict(list)
  problems = []
  for err_rs in sorted(src_dir.glob("*/src/err.rs")):
    crate = err_rs.relative_to(src_dir).parts[0]
    variants = extract_code_enum_variants(
        err_rs.read_text(encoding="utf-8"),
        problems=problems,
        crate=crate,
        deprecated=deprecated[crate],
    )
    defined[crate] = variants
    if crate not in CRATE_RANGES:
      problems.append(f"{crate}: no CRATE_RANGES entry")
    for name, val in defined[crate].items():
      owners[val].append(f"{crate}::{name}")
      if name == "CrateDefault":
        problems.append(f"{crate}::CrateDefault is forbidden in err.rs")
      if crate in CRATE_RANGES and not in_crate_range(crate, val):
        base_lo, base_hi, sub_lo, sub_hi = CRATE_RANGES[crate]
        lo = base_lo + 1 if sub_lo == 0 else base_lo
        band = ""
        if (sub_lo, sub_hi) != (0, 999):
          band = f" sub-band +{sub_lo:03d}..+{sub_hi:03d}"
        problems.append(
            f"{crate}::{name} ({val}) outside {lo}..={base_hi}{band}"
        )
    if crate in CRATE_RANGES:
      base_lo, _, sub_lo, _ = CRATE_RANGES[crate]
      by_band = collections.defaultdict(list)
      for val in defined[crate].values():
        if in_crate_range(crate, val):
          by_band[(val // 1000) * 1000].append(val)
      for band, vals in sorted(by_band.items()):
        sorted_vals = sorted(vals)
        if sub_lo == 0:
          if band == base_lo or sorted_vals[0] == band + 1:
            expected_start = band + 1
          else:
            expected_start = band
        else:
          expected_start = band + sub_lo
        expected = list(
            range(expected_start, expected_start + len(sorted_vals))
        )
        if sorted_vals != expected:
          problems.append(
              f"{crate}: band {band} codes {sorted_vals} are not contiguous"
              f" from {expected_start} (expected"
              f" {expected[0]}..={expected[-1]})"
          )

  sites = collections.defaultdict(list)
  for path in sorted(src_dir.glob("*/src/**/*.rs")):
    crate = path.relative_to(src_dir).parts[0]
    rel = path.relative_to(src_dir).as_posix()
    text = path.read_text(encoding="utf-8")
    for line, code_name in extract_error_call_sites(text):
      if code_name:
        sites[(crate, code_name)].append(f"{rel}:{line}")
      elif crate in defined:
        problems.append(f"{rel}:{line}: untagged error!/warn! site")

  for (crate, name), locs in sorted(sites.items()):
    if name == "CrateDefault":
      problems.append(f"{crate}: tagged site uses CrateDefault: {locs}")
    elif name not in defined.get(crate, {}):
      problems.append(
          f"{crate}::{name} not defined in {crate}/src/err.rs: {locs}"
      )
    elif name in deprecated[crate]:
      problems.append(
          f"{crate}::{name} ({defined[crate][name]}) is #[deprecated] in"
          f" err.rs but still used at {', '.join(locs)}"
      )
    elif len(locs) > 1 and (crate, name) not in ALLOWED_SHARED:
      problems.append(f"{crate}::{name} shared by {len(locs)} sites: {locs}")
  for crate, variants in sorted(defined.items()):
    for name in variants:
      if (crate, name) not in sites and name not in deprecated[crate]:
        problems.append(f"{crate}::{name} defined but has no call site")
  for code, names in sorted(owners.items()):
    if len(names) > 1:
      problems.append(f"code {code} defined more than once: {names}")

  return problems, sites, defined


def check_tree(base_dir: pathlib.Path) -> list[str]:
  """Returns the `--check` problems for a crate source tree (empty if clean)."""
  return scan_tree(base_dir)[0]


def main(argv: list[str] | None = None) -> int:
  parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
  parser.add_argument("--check", action="store_true")
  parser.add_argument("--verbose", action="store_true")
  parser.add_argument(
      "--src-dir",
      type=pathlib.Path,
      default=DEFAULT_REPO_ROOT,
      help="Repository root or crates directory (defaults to repository root)",
  )
  args = parser.parse_args(argv)

  base_dir = args.src_dir.resolve()
  problems, sites, defined = scan_tree(base_dir)

  for (crate, name), locs in sorted(sites.items()):
    print(f"{len(locs):3d}  {crate}::{name}")
    if args.verbose:
      for loc in locs:
        print(f"       {loc}")

  total = sum(len(v) for v in sites.values())
  total_defined = sum(len(v) for v in defined.values())
  print(
      f"# {total} tagged sites, {len(sites)} codes in use, {total_defined}"
      " codes defined"
  )

  if args.check and problems:
    print("\n".join(problems), file=sys.stderr)
    return 1
  return 0


if __name__ == "__main__":
  sys.exit(main())
