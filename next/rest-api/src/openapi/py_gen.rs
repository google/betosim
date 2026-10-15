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

//! Python `StrEnum` and `@dataclass` type generator for OpenAPI 3.1 documents.

use std::fmt::Write as _;

use serde_json::Value;

use super::{SchemaKind, Spec};

const PY_HEADER: &str = r#"from dataclasses import dataclass
from enum import StrEnum
"#;

/// Generates Python `StrEnum` and `@dataclass` types from an OpenAPI 3.1
/// document.
pub fn generate_python(doc: &Value) -> String {
    let spec = Spec::from_doc(doc);
    let mut out = String::from(PY_HEADER);
    for schema in &spec.schemas {
        let name = schema.name;
        out.push_str("\n\n");
        match &schema.kind {
            SchemaKind::Enum(variants) => {
                let _ = writeln!(out, "class {name}(StrEnum):");
                for &v in variants {
                    let _ = writeln!(out, "    {} = {v:?}", v.to_ascii_uppercase());
                }
            }
            SchemaKind::Struct(fields) => {
                let _ = writeln!(
                    out,
                    r#"@dataclass
class {name}:"#
                );
                if fields.is_empty() {
                    let _ = writeln!(out, "    pass");
                } else {
                    for f in fields {
                        let base = f.ty.render(["str", "int", "float", "bool"], "list");
                        let suffix = if f.required { "" } else { " | None = None" };
                        let _ = writeln!(out, "    {}: {base}{suffix}", f.name);
                    }
                }
            }
        }
    }
    out
}
