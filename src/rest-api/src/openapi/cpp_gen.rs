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

//! C++ `nlohmann::json` type generator for OpenAPI 3.1 documents.

use std::fmt::Write as _;

use serde_json::Value;

use super::{SchemaKind, Spec, to_pascal_case};

const CPP_HEADER: &str = r#"#include <cstdint>
#include <optional>
#include <string>
#include <vector>
#include <nlohmann/json.hpp>
"#;

/// Generates C++ `nlohmann::json` types from an OpenAPI 3.1 document.
pub fn generate_cpp(doc: &Value) -> String {
    let spec = Spec::from_doc(doc);
    let mut out = String::from(CPP_HEADER);
    for schema in &spec.schemas {
        let name = schema.name;
        out.push('\n');
        match &schema.kind {
            SchemaKind::Enum(variants) => {
                let decls = variants
                    .iter()
                    .map(|v| format!("  k{},\n", to_pascal_case(v)))
                    .collect::<String>();
                let pairs = variants
                    .iter()
                    .map(|v| format!("  {{{name}::k{}, {v:?}}},\n", to_pascal_case(v)))
                    .collect::<String>();
                let _ = write!(
                    out,
                    r#"enum class {name} {{
{decls}}};
NLOHMANN_JSON_SERIALIZE_ENUM({name}, {{
{pairs}}})
"#
                );
            }
            SchemaKind::Struct(fields) if fields.is_empty() => {
                let _ = writeln!(out, "struct {name} {{}};");
            }
            SchemaKind::Struct(fields) => {
                let members = fields
                    .iter()
                    .map(|f| {
                        let base = f
                            .ty
                            .render(["std::string", "int64_t", "double", "bool"], "std::vector");
                        let ty = if f.required { base } else { format!("std::optional<{base}>") };
                        format!("  {ty} {};\n", f.name)
                    })
                    .collect::<String>();
                let names = fields.iter().map(|f| f.name).collect::<Vec<_>>().join(", ");
                let _ = write!(
                    out,
                    r#"struct {name} {{
{members}}};
NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE_WITH_DEFAULT({name}, {names})
"#
                );
            }
        }
    }
    out
}
