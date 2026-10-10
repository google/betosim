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

//! OpenAPI 3.1 route dispatch, validation, and multi-language stub generation
//! for the `/v1` REST API.

#[path = "openapi/cpp_gen.rs"]
mod cpp_gen;
#[path = "openapi/py_gen.rs"]
mod py_gen;
#[path = "openapi/rust_gen.rs"]
mod rust_gen;
#[path = "openapi/spec.rs"]
mod spec;

use std::{collections::BTreeSet, fmt::Debug};

pub use cpp_gen::generate_cpp;
use hyper::Method;
pub use py_gen::generate_python;
pub use rust_gen::generate_rust;
use serde_json::Value;
pub use spec::expand;
pub(crate) use spec::{SchemaKind, Spec};

/// OpenAPI version required by [`validate`].
pub const OPENAPI_VERSION: &str = "3.1.0";
/// Prefix of schema `$ref`s in the document.
pub const SCHEMA_REF_PREFIX: &str = "#/components/schemas/";
/// Prefix of shared response `$ref`s in the document.
pub const RESPONSE_REF_PREFIX: &str = "#/components/responses/";
/// Prefix of shared parameter `$ref`s in the document.
pub const PARAM_REF_PREFIX: &str = "#/components/parameters/";
/// Only media type used for request and response bodies.
pub const JSON: &str = "application/json";
const ALLOWED_METHODS: &[&str] = &["get", "post", "patch", "put", "delete"];
const RESERVED_IDENTS: &[&str] = include!("openapi/reserved_idents.inc");

macro_rules! check {
    ($errs:expr, $ok:expr, $($arg:tt)+) => {
        if !($ok) {
            $errs.push(format!($($arg)+));
        }
    };
}

/// One row of the generated route table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route<Op> {
    pub method: Method,
    /// Path template from the server root, for example `/v1/widgets/{name}:reset`.
    pub path: &'static str,
    /// Dispatch key. Its `Debug` name is the OpenAPI `operationId`.
    pub op: Op,
}

impl<Op> Route<Op> {
    /// Creates a [`Route`] entry.
    pub const fn new(method: Method, path: &'static str, op: Op) -> Self {
        Self { method, path, op }
    }

    /// The operation's `operationId`.
    pub fn operation_id(&self) -> String
    where
        Op: Debug,
    {
        format!("{:?}", self.op)
    }
}

/// Names of the parameters in a path template, in order.
pub fn path_params(template: &str) -> impl Iterator<Item = &str> {
    template.split('/').filter_map(|s| split_param(s).map(|(_, name, _)| name))
}

fn split_param(segment: &str) -> Option<(&str, &str, &str)> {
    let (prefix, rest) = segment.split_once('{')?;
    let (name, suffix) = rest.split_once('}')?;
    Some((prefix, name, suffix))
}

/// Extracted `(param_name, value)` pairs from a matched route path.
pub type PathParams<'r, 'p> = Vec<(&'r str, &'p str)>;

/// Finds the route for a request and extracts its path parameters.
pub fn find_route<'r, 'p, Op>(
    routes: &'r [Route<Op>],
    method: &Method,
    path: &'p str,
) -> Option<(&'r Route<Op>, PathParams<'r, 'p>)> {
    routes
        .iter()
        .filter(|r| r.method == *method)
        .filter_map(|r| match_path(r.path, path).map(|params| (r, params)))
        .min_by_key(|(_, p)| (p.len(), p.iter().map(|(_, v)| v.len()).sum::<usize>()))
}

fn match_path<'t, 'p>(template: &'t str, path: &'p str) -> Option<PathParams<'t, 'p>> {
    let (mut ts, mut ps, mut params) = (template.split('/'), path.split('/'), Vec::new());
    loop {
        match (ts.next(), ps.next()) {
            (Some(t), Some(p)) => {
                if let Some((pre, name, suf)) = split_param(t) {
                    let val = p.strip_prefix(pre)?.strip_suffix(suf).filter(|v| !v.is_empty())?;
                    params.push((name, val));
                } else if t != p {
                    return None;
                }
            }
            (None, None) => return Some(params),
            _ => return None,
        }
    }
}

fn obj(v: &Value) -> impl Iterator<Item = (&str, &Value)> {
    v.as_object().into_iter().flatten().map(|(k, v)| (k.as_str(), v))
}

fn arr(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

fn str_of(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

fn is_upper_camel(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_uppercase())
        && s.chars().all(|c: char| c.is_ascii_alphanumeric())
}

fn is_ident(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c: char| c.is_ascii_alphanumeric() || c == '_')
        && !RESERVED_IDENTS.contains(&s)
}

fn is_valid_path_param(p: &Value) -> bool {
    is_ident(str_of(&p["name"]))
        && p["in"] == "path"
        && p["required"] == true
        && p["schema"]["type"] == "string"
}

fn resolve<'a>(r: &str, prefix: &str, map: &'a Value) -> Option<&'a Value> {
    r.strip_prefix(prefix).and_then(|name| map.get(name))
}

fn deref<'a>(v: &'a Value, prefix: &str, map: &'a Value) -> &'a Value {
    v["$ref"].as_str().and_then(|r| resolve(r, prefix, map)).unwrap_or(v)
}

/// Returns every way `doc` departs from the supported OpenAPI 3.1 subset.
pub fn validate(doc: &Value) -> Vec<String> {
    let mut errs = Vec::new();
    let comp = &doc["components"];
    let (schemas, responses, params_comp) =
        (&comp["schemas"], &comp["responses"], &comp["parameters"]);

    check!(errs, doc["openapi"] == OPENAPI_VERSION, "openapi must be {OPENAPI_VERSION}");

    let mut services = BTreeSet::new();
    for tag in arr(&doc["tags"]) {
        let name = str_of(&tag["name"]);
        check!(
            errs,
            is_upper_camel(name) && name.ends_with("Service"),
            "service {name:?} must be UpperCamelCase ending in Service"
        );
        check!(errs, services.insert(name), "service {name:?} is declared twice");
    }

    let mut refs = Vec::new();
    collect_refs(doc, &mut refs);
    for r in refs {
        let ok = resolve(r, SCHEMA_REF_PREFIX, schemas).is_some()
            || resolve(r, RESPONSE_REF_PREFIX, responses).is_some()
            || resolve(r, PARAM_REF_PREFIX, params_comp).is_some();
        check!(errs, ok, "$ref {r:?} does not resolve in components");
    }
    if let Some((name, _)) = obj(schemas).find(|&(n, _)| has_cycle(n, schemas, &mut Vec::new())) {
        errs.push(format!("schema {name:?} is recursive"));
    }

    for (name, resp) in obj(responses) {
        if let Err(e) = body_schema(&resp["content"], schemas) {
            errs.push(format!("components.responses.{name}: {e}"));
        }
    }
    for (name, p) in obj(params_comp) {
        check!(
            errs,
            is_valid_path_param(p),
            "components.parameters.{name}: must be a required string path param"
        );
    }
    for (name, schema) in obj(schemas) {
        check_schema(name, schema, &mut errs);
    }

    let mut operation_ids = BTreeSet::new();
    for (path, item) in obj(&doc["paths"]) {
        let tparams: BTreeSet<&str> = path_params(path).collect();
        for (method, op) in obj(item).filter(|&(m, _)| m != "parameters") {
            let at = format!("{} {path}", method.to_uppercase());
            if !ALLOWED_METHODS.contains(&method) {
                errs.push(format!("{at}: method is not allowed"));
                continue;
            }
            let id = str_of(&op["operationId"]);
            check!(errs, is_upper_camel(id), "{at}: operationId {id:?} must be UpperCamelCase");
            check!(errs, operation_ids.insert(id), "{at}: operationId {id:?} is not unique");
            check!(
                errs,
                matches!(arr(&op["tags"]), [t] if services.contains(str_of(t))),
                "{at}: needs exactly one declared service tag"
            );

            let mut declared = BTreeSet::new();
            for raw_p in arr(&item["parameters"]).iter().chain(arr(&op["parameters"])) {
                let p = deref(raw_p, PARAM_REF_PREFIX, params_comp);
                let name = str_of(&p["name"]);
                check!(
                    errs,
                    is_valid_path_param(p),
                    "{at}: parameter {name:?} must be a required string path param"
                );
                declared.insert(name);
            }
            check!(errs, declared == tparams, "{at}: parameters do not match the path template");

            if let Some(req) = op.get("requestBody") {
                check_request(&at, req, &tparams, schemas, &mut errs);
            }
            check!(
                errs,
                op["responses"].get("200").is_some(),
                "{at}: responses must include \"200\""
            );
            for (status, resp) in obj(&op["responses"]) {
                let resolved = deref(resp, RESPONSE_REF_PREFIX, responses);
                if let Err(e) = body_schema(&resolved["content"], schemas) {
                    errs.push(format!("{at}: response {status} {e}"));
                }
            }
        }
    }
    errs
}

fn check_schema(name: &str, schema: &Value, errs: &mut Vec<String>) {
    check!(
        errs,
        is_upper_camel(name) && !RESERVED_IDENTS.contains(&name),
        "schema {name:?} must be an UpperCamelCase identifier"
    );
    check_no_default_noise(&format!("schema {name:?}"), schema, errs);
    if let Some(values) = schema.get("enum") {
        let items = arr(values);
        let pascal: BTreeSet<_> =
            items.iter().filter_map(Value::as_str).map(to_pascal_case).collect();
        check!(
            errs,
            !items.is_empty()
                && items.iter().all(|v| v.as_str().is_some_and(is_ident))
                && pascal.len() == items.len()
                && pascal.iter().all(|p| !p.is_empty()),
            "schema {name:?}: enum values must be distinct valid identifiers"
        );
    } else if schema["type"] != "object" {
        errs.push(format!("schema {name:?}: must be an enum or object"));
    } else {
        for (field, prop) in obj(&schema["properties"]) {
            check!(errs, is_ident(field), "schema {name:?}.{field}: invalid field identifier");
            check_no_default_noise(&format!("schema {name:?}.{field}"), prop, errs);
            check!(
                errs,
                parse_type(prop).is_some(),
                "schema {name:?}.{field}: unsupported property type"
            );
        }
    }
}

fn check_request(
    at: &str,
    request: &Value,
    tparams: &BTreeSet<&str>,
    schemas: &Value,
    errs: &mut Vec<String>,
) {
    let schema = match body_schema(&request["content"], schemas) {
        Ok(s) => s,
        Err(e) => return errs.push(format!("{at}: requestBody {e}")),
    };
    let props = &schema["properties"];
    for &p in tparams {
        check!(errs, props.get(p).is_none(), "{at}: path param {p:?} is also a body field");
    }
    match request["content"][JSON].get("example").filter(|e| !e.is_null()) {
        None => errs.push(format!("{at}: requestBody needs an example")),
        Some(Value::Object(ex)) => {
            check!(
                errs,
                schema["type"] == "object",
                "{at}: requestBody example must not be an object"
            );
            for k in ex.keys() {
                check!(errs, props.get(k).is_some(), "{at}: example has unknown {k:?}");
            }
            for k in arr(&schema["required"]) {
                check!(errs, ex.contains_key(str_of(k)), "{at}: example lacks {k}");
            }
        }
        Some(_) => {
            check!(errs, schema["type"] != "object", "{at}: requestBody example must be an object");
        }
    }
}

fn check_no_default_noise(ctx: &str, val: &Value, errs: &mut Vec<String>) {
    for (ok, msg) in [
        (val.get("nullable").is_none(), "use OpenAPI 3.1 type array instead of nullable"),
        (!val["required"].as_array().is_some_and(Vec::is_empty), "omit empty required array"),
        (val["deprecated"] != false, "omit deprecated: false"),
        (val["description"] != "", "omit empty description"),
    ] {
        check!(errs, ok, "{ctx}: {msg}");
    }
}

fn body_schema<'a>(content: &Value, schemas: &'a Value) -> Result<&'a Value, String> {
    let map = content.as_object().ok_or("has no content")?;
    if !map.keys().eq([JSON]) {
        return Err(format!("content must be exactly {JSON}"));
    }
    let r = content[JSON]["schema"]["$ref"].as_str().ok_or("schema must be a $ref")?;
    resolve(r, SCHEMA_REF_PREFIX, schemas).ok_or_else(|| format!("{r:?} does not resolve"))
}

fn collect_refs<'a>(value: &'a Value, refs: &mut Vec<&'a str>) {
    match value {
        Value::Object(map) => map.iter().for_each(|(k, v)| match (k.as_str(), v.as_str()) {
            ("$ref", Some(r)) => refs.push(r),
            _ => collect_refs(v, refs),
        }),
        Value::Array(items) => items.iter().for_each(|v| collect_refs(v, refs)),
        _ => {}
    }
}

fn has_cycle<'a>(name: &'a str, schemas: &'a Value, path: &mut Vec<&'a str>) -> bool {
    if path.contains(&name) {
        return true;
    }
    path.push(name);
    let mut refs = Vec::new();
    schemas.get(name).into_iter().for_each(|s| collect_refs(s, &mut refs));
    let cycle = refs
        .into_iter()
        .filter_map(|r| r.strip_prefix(SCHEMA_REF_PREFIX))
        .any(|dep| has_cycle(dep, schemas, path));
    path.pop();
    cycle
}

#[derive(Clone, Copy)]
pub(crate) enum Scalar<'a> {
    String,
    Int,
    Float,
    Bool,
    Named(&'a str),
}

#[derive(Clone, Copy)]
pub(crate) enum TypeRef<'a> {
    Scalar(Scalar<'a>),
    Array(Scalar<'a>),
}

pub(crate) fn parse_type(prop: &Value) -> Option<(TypeRef<'_>, bool)> {
    if let Some(r) = prop["$ref"].as_str() {
        return Some((TypeRef::Scalar(Scalar::Named(r.strip_prefix(SCHEMA_REF_PREFIX)?)), false));
    }
    let (ty, nullable) = match &prop["type"] {
        Value::String(s) => (s.as_str(), false),
        Value::Array(items) => (
            items.iter().filter_map(Value::as_str).find(|&t| t != "null")?,
            items.iter().any(|t| t == "null"),
        ),
        _ => return None,
    };
    if ty == "array" {
        let (TypeRef::Scalar(item), _) = parse_type(&prop["items"])? else { return None };
        return Some((TypeRef::Array(item), nullable));
    }
    let scalar = match ty {
        "string" => Scalar::String,
        "integer" => Scalar::Int,
        "number" => Scalar::Float,
        "boolean" => Scalar::Bool,
        _ => return None,
    };
    Some((TypeRef::Scalar(scalar), nullable))
}

pub(crate) fn to_pascal_case(s: &str) -> String {
    s.split(['_', '-'])
        .filter_map(|p| {
            let mut c = p.chars();
            Some(format!("{}{}", c.next()?.to_ascii_uppercase(), c.as_str().to_ascii_lowercase()))
        })
        .collect()
}
