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

//! Intermediate representation of a validated OpenAPI 3.1 document for the
//! Rust, C++, and Python stub generators.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::{ALLOWED_METHODS, JSON, SCHEMA_REF_PREFIX, Scalar, TypeRef, arr, obj, parse_type};

/// Parsed view of an OpenAPI document used by the stub generators.
pub(crate) struct Spec<'a> {
    pub(crate) schemas: Vec<SchemaDef<'a>>,
    pub(crate) routes: Vec<(&'a str, &'a str, &'a str)>,
}

#[derive(Clone)]
pub(crate) struct SchemaDef<'a> {
    pub(crate) name: &'a str,
    pub(crate) description: Option<&'a str>,
    pub(crate) rust_type: Option<&'a str>,
    pub(crate) kind: SchemaKind<'a>,
}

#[derive(Clone)]
pub(crate) enum SchemaKind<'a> {
    Enum(Vec<&'a str>),
    Struct(Vec<FieldDef<'a>>),
}

#[derive(Clone)]
pub(crate) struct FieldDef<'a> {
    pub(crate) name: &'a str,
    pub(crate) description: Option<&'a str>,
    pub(crate) ty: TypeRef<'a>,
    pub(crate) required: bool,
}

impl<'a> TypeRef<'a> {
    pub(crate) fn render(self, prims: [&'a str; 4], list: &str) -> String {
        let s = |x: Scalar<'a>| match x {
            Scalar::String => prims[0],
            Scalar::Int => prims[1],
            Scalar::Float => prims[2],
            Scalar::Bool => prims[3],
            Scalar::Named(n) => n,
        };
        match self {
            Self::Scalar(x) => s(x).to_owned(),
            Self::Array(x) if list == "list" => format!("list[{}]", s(x)),
            Self::Array(x) => format!("{list}<{}>", s(x)),
        }
    }
}

impl<'a> Spec<'a> {
    pub(crate) fn from_doc(doc: &'a Value) -> Self {
        let by_name: BTreeMap<&'a str, SchemaDef<'a>> =
            obj(&doc["components"]["schemas"]).map(|(n, s)| (n, parse_schema(n, s))).collect();
        let (mut visited, mut schemas) = (BTreeSet::new(), Vec::with_capacity(by_name.len()));
        for &name in by_name.keys() {
            visit_schema(name, &by_name, &mut visited, &mut schemas);
        }
        let mut skip = BTreeSet::new();
        for s in schemas.iter_mut().rev() {
            if skip.contains(s.name) {
                s.rust_type.get_or_insert("skip");
            }
            if s.rust_type.is_some()
                && let SchemaKind::Struct(fields) = &s.kind
            {
                for f in fields {
                    if let TypeRef::Scalar(Scalar::Named(d)) | TypeRef::Array(Scalar::Named(d)) =
                        f.ty
                    {
                        skip.insert(d);
                    }
                }
            }
        }
        let paths: BTreeMap<&'a str, &'a Value> = obj(&doc["paths"]).collect();
        let mut routes = Vec::new();
        for (path, item) in paths {
            for &m in ALLOWED_METHODS {
                if let Some(op_id) = item[m]["operationId"].as_str() {
                    routes.push((m, path, op_id));
                }
            }
        }
        Self { schemas, routes }
    }
}

/// Expands concise YAML conventions (schema aliases, implicit `type`, default
/// `required` lists, and top-level service `tags`) into canonical OpenAPI 3.1
/// JSON.
pub fn expand(doc: &mut Value) {
    let raw: Vec<(String, Value)> =
        obj(&doc["components"]["schemas"]).map(|(k, v)| (k.to_owned(), v.clone())).collect();
    let find_schema = |v: &Value| raw.iter().find_map(|(k, s)| (s == v).then_some(k.as_str()));

    if doc.get("tags").is_none() {
        let mut tags = BTreeSet::new();
        for (_, item) in obj(&doc["paths"]) {
            for &m in ALLOWED_METHODS {
                for t in arr(&item[m]["tags"]).iter().filter_map(Value::as_str) {
                    tags.insert(t.to_owned());
                }
            }
        }
        doc["tags"] = Value::Array(tags.into_iter().map(|n| json!({ "name": n })).collect());
    }
    if let Some(schemas) = doc.pointer_mut("/components/schemas").and_then(Value::as_object_mut) {
        for schema in schemas.values_mut() {
            if schema.get("enum").is_some() && schema.get("type").is_none() {
                schema["type"] = Value::from("string");
            }
            let Some(props) = schema.get_mut("properties").and_then(Value::as_object_mut) else {
                continue;
            };
            for prop in props.values_mut() {
                if let Some(target) = find_schema(prop) {
                    *prop = json!({ "$ref": format!("{SCHEMA_REF_PREFIX}{target}") });
                } else if let [item] = arr(prop) {
                    let items = match find_schema(item) {
                        Some(t) => json!({ "$ref": format!("{SCHEMA_REF_PREFIX}{t}") }),
                        None => item.clone(),
                    };
                    *prop = json!({ "type": "array", "items": items });
                } else if let Some(target) = prop.get("items").and_then(find_schema) {
                    prop["items"] = json!({ "$ref": format!("{SCHEMA_REF_PREFIX}{target}") });
                }
            }
            if schema.get("type").is_none() {
                schema["type"] = Value::from("object");
            }
            if schema.get("required").is_none() {
                let req: Vec<Value> = obj(&schema["properties"])
                    .filter(|&(_, p)| !parse_type(p).is_some_and(|(_, nullable)| nullable))
                    .map(|(k, _)| Value::from(k))
                    .collect();
                if !req.is_empty() {
                    schema["required"] = Value::Array(req);
                }
            }
        }
    }
    if let Some(paths) = doc.get_mut("paths").and_then(Value::as_object_mut) {
        for item in paths.values_mut() {
            for &m in ALLOWED_METHODS {
                let Some(resps) = item
                    .get_mut(m)
                    .and_then(|op| op.get_mut("responses"))
                    .and_then(Value::as_object_mut)
                else {
                    continue;
                };
                for (status, resp) in resps.iter_mut() {
                    let Some(target) = find_schema(resp) else { continue };
                    let desc = if status == "200" { "OK" } else { "Error" };
                    *resp = json!({
                        "description": desc,
                        "content": {
                            JSON: { "schema": { "$ref": format!("{SCHEMA_REF_PREFIX}{target}") } }
                        }
                    });
                }
            }
        }
    }
}

fn parse_schema<'a>(name: &'a str, s: &'a Value) -> SchemaDef<'a> {
    let kind = if let Some(values) = s["enum"].as_array() {
        SchemaKind::Enum(values.iter().filter_map(Value::as_str).collect())
    } else {
        let req: BTreeSet<&str> = arr(&s["required"]).iter().filter_map(Value::as_str).collect();
        let mut fields: Vec<FieldDef<'a>> = obj(&s["properties"])
            .filter_map(|(name, prop)| {
                let (ty, nullable) = parse_type(prop)?;
                let required = (req.is_empty() || req.contains(name)) && !nullable;
                Some(FieldDef { name, description: prop["description"].as_str(), ty, required })
            })
            .collect();
        fields.sort_by_key(|f| (!f.required, f.name));
        SchemaKind::Struct(fields)
    };
    let (description, rust_type) = (s["description"].as_str(), s["x-rust-type"].as_str());
    SchemaDef { name, description, rust_type, kind }
}

fn visit_schema<'a>(
    name: &'a str,
    by_name: &BTreeMap<&'a str, SchemaDef<'a>>,
    visited: &mut BTreeSet<&'a str>,
    out: &mut Vec<SchemaDef<'a>>,
) {
    if !visited.insert(name) {
        return;
    }
    let Some(schema) = by_name.get(name) else { return };
    if let SchemaKind::Struct(fields) = &schema.kind {
        for f in fields {
            if let TypeRef::Scalar(Scalar::Named(dep)) | TypeRef::Array(Scalar::Named(dep)) = f.ty {
                visit_schema(dep, by_name, visited, out);
            }
        }
    }
    out.push(schema.clone());
}
