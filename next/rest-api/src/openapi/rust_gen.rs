// Copyright 2026 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Rust `serde` type and route-table generator for OpenAPI 3.1 documents.

use std::fmt::Write as _;

use serde_json::Value;

use super::{SchemaKind, Spec, to_pascal_case};

/// Generates the `OPENAPI_JSON` constant, Rust `serde` types, the `Op` dispatch
/// enum, and the `ROUTES` table from an OpenAPI 3.1 document.
pub fn generate_rust(doc: &Value) -> String {
    let spec = Spec::from_doc(doc);
    let mut out = String::new();
    let json = serde_json::to_string_pretty(doc).unwrap_or_default();
    let _ = writeln!(
        out,
        r#"/// OpenAPI 3.1 JSON document (from `openapi.yaml`) served by `GET /v1/openapi.json`.
pub const OPENAPI_JSON: &str = {json:?};
"#
    );

    for schema in &spec.schemas {
        let name = schema.name;
        if let Some(ext) = schema.rust_type {
            if ext != "skip" {
                write_doc(&mut out, "", schema.description);
                let _ = writeln!(out, "pub type {name} = {ext};\n");
            }
            continue;
        }
        write_doc(&mut out, "", schema.description);
        match &schema.kind {
            SchemaKind::Enum(variants) => {
                let mut body = String::new();
                for &v in variants {
                    let ident = to_pascal_case(v);
                    let _ = writeln!(
                        body,
                        r#"    #[serde(rename = {v:?})]
    {ident},"#
                    );
                }
                let _ = writeln!(
                    out,
                    r#"#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum {name} {{
{body}}}
"#
                );
            }
            SchemaKind::Struct(fields) if fields.is_empty() => {
                let _ = writeln!(
                    out,
                    r#"#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct {name} {{}}
"#
                );
            }
            SchemaKind::Struct(fields) => {
                let mut body = String::new();
                for f in fields {
                    write_doc(&mut body, "    ", f.description);
                    let fname = f.name;
                    let base = f.ty.render(["String", "i64", "f64", "bool"], "Vec");
                    if f.required {
                        let _ = writeln!(body, "    pub {fname}: {base},");
                    } else {
                        let _ = writeln!(
                            body,
                            r#"    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub {fname}: Option<{base}>,"#
                        );
                    }
                }
                let _ = writeln!(
                    out,
                    r#"#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct {name} {{
{body}}}
"#
                );
            }
        }
    }

    let ops = spec.routes.iter().fold(String::new(), |mut s, &(_, _, op_id)| {
        let _ = writeln!(s, "    {op_id},");
        s
    });
    let routes = spec.routes.iter().fold(String::new(), |mut s, &(method, path, op_id)| {
        let method = method.to_ascii_uppercase();
        let _ = writeln!(s, "    Route::new(Method::{method}, {path:?}, Op::{op_id}),");
        s
    });
    let _ = write!(
        out,
        r#"#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {{
{ops}}}

pub const ROUTES: &[Route<Op>] = &[
{routes}];
"#
    );
    out
}

fn write_doc(out: &mut String, indent: &str, desc: Option<&str>) {
    for line in desc.into_iter().flat_map(str::lines) {
        let _ = writeln!(out, "{indent}/// {line}");
    }
}
