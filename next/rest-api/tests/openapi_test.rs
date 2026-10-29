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

//! Prototype of the `openapi.yaml`-first pipeline on a small Widget test API.
//!
//! One OpenAPI 3.1 YAML document ([`spec`]) is the source of truth:
//!
//! * [`validate`] checks that it stays within the subset Cuttlefish's
//!   `cvd env` and the stub generators consume.
//! * [`generate_rust`], [`generate_cpp`], and [`generate_python`] emit the
//!   Rust `serde` types + [`ROUTES`] table, C++ `nlohmann::json` structs, and
//!   Python `@dataclass` types directly from [`spec`].
//! * `GET /v1/openapi.json` serves [`spec`] serialized as JSON, and [`find_route`]
//!   dispatches requests through the generated [`ROUTES`] table.

use hyper::Method;
use netsim_rest_api::openapi::{
    JSON, Route, find_route, generate_cpp, generate_python, generate_rust, validate,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Rust types and route table generated from [`spec`] by [`generate_rust`].
/// `test_generate_rust` asserts that [`generate_rust`] produces this exact
/// code.
mod generated {
    use super::*;

    /// Widget color.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Color {
        #[serde(rename = "RED")]
        Red,
        #[serde(rename = "GREEN")]
        Green,
    }

    /// Empty response body.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Empty {}

    /// A widget.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Widget {
        pub color: Color,
        pub name: String,
        /// Optional display label.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub label: Option<String>,
    }

    /// Response body for `ListWidgets`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct ListWidgetsResponse {
        pub widgets: Vec<Widget>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Op {
        ListWidgets,
        CreateWidget,
        GetWidget,
        DeleteWidget,
        ResetWidget,
    }

    pub const ROUTES: &[Route<Op>] = &[
        Route::new(Method::GET, "/v1/widgets", Op::ListWidgets),
        Route::new(Method::POST, "/v1/widgets", Op::CreateWidget),
        Route::new(Method::GET, "/v1/widgets/{name}", Op::GetWidget),
        Route::new(Method::DELETE, "/v1/widgets/{name}", Op::DeleteWidget),
        Route::new(Method::POST, "/v1/widgets/{name}:reset", Op::ResetWidget),
    ];
}

use generated::{Color, Empty, ListWidgetsResponse, Op, ROUTES, Widget};

/// Checked-in `openapi.yaml` for the Widget test API.
const OPENAPI_YAML: &str = include_str!("openapi.yaml");

/// Parses [`OPENAPI_YAML`] into a JSON [`Value`].
fn spec() -> Value {
    let mut v: serde_yaml::Value = serde_yaml::from_str(OPENAPI_YAML).unwrap_or_default();
    let _ = v.apply_merge();
    serde_yaml::from_value(v).unwrap_or_default()
}

/// Serves one request the way the REST server would: `GET /v1/openapi.json`
/// returns [`spec`], and everything else is dispatched through [`ROUTES`].
fn handle(method: &Method, path: &str, body: &[u8]) -> (u16, Value) {
    if method == Method::GET && path == "/v1/openapi.json" {
        return (200, spec());
    }
    let Some((route, params)) = find_route(ROUTES, method, path) else {
        return (404, json!({}));
    };
    let name = params.iter().find_map(|&(k, v)| (k == "name").then_some(v));
    let widget = |name: &str| Widget { color: Color::Red, name: name.into(), label: None };
    let result = match route.op {
        Op::ListWidgets => {
            serde_json::to_value(ListWidgetsResponse { widgets: vec![widget("w1")] })
        }
        Op::CreateWidget => match serde_json::from_slice::<Widget>(body) {
            Ok(created) => serde_json::to_value(created),
            Err(e) => return (400, json!({ "error": e.to_string() })),
        },
        Op::GetWidget => serde_json::to_value(widget(name.unwrap_or_default())),
        Op::DeleteWidget | Op::ResetWidget => serde_json::to_value(Empty {}),
    };
    match result {
        Ok(value) => (200, value),
        Err(e) => (500, json!({ "error": e.to_string() })),
    }
}

#[test]
fn test_spec_is_cvd_env_friendly() {
    assert_eq!(validate(&spec()), Vec::<String>::new());
}

#[test]
fn test_validate_rejects_unfriendly_documents() {
    type Case = (&'static str, fn(&mut Value));
    let cases: &[Case] = &[
        ("required string path param", |d| {
            d["paths"]["/v1/widgets"]["get"]["parameters"] =
                json!([{ "name": "filter", "in": "query", "required": false }]);
        }),
        ("must be exactly application/json", |d| {
            d["paths"]["/v1/widgets"]["post"]["requestBody"]["content"]["text/plain"] = json!({});
        }),
        ("not unique", |d| {
            d["paths"]["/v1/widgets"]["get"]["operationId"] = json!("CreateWidget");
        }),
        ("does not resolve", |d| {
            d["paths"]["/v1/widgets"]["get"]["responses"]["200"]["content"][JSON]["schema"] =
                json!({ "$ref": "#/components/schemas/Missing" });
        }),
        ("also a body field", |d| {
            let post = d["paths"]["/v1/widgets"]["post"].clone();
            d["paths"]["/v1/widgets/{name}"]["post"] = post;
            d["paths"]["/v1/widgets/{name}"]["post"]["operationId"] = json!("RenameWidget");
            d["paths"]["/v1/widgets/{name}"]["post"]["parameters"] = json!([{
                "name": "name",
                "in": "path",
                "required": true,
                "schema": { "type": "string" }
            }]);
        }),
        ("needs an example", |d| {
            if let Some(media) =
                d["paths"]["/v1/widgets"]["post"]["requestBody"]["content"][JSON].as_object_mut()
            {
                media.remove("example");
            }
        }),
        ("is recursive", |d| {
            d["components"]["schemas"]["Empty"]["properties"] =
                json!({ "self_ref": { "$ref": "#/components/schemas/Empty" } });
        }),
        ("unsupported property type", |d| {
            d["components"]["schemas"]["Empty"]["properties"] =
                json!({ "nested": { "type": "object" } });
        }),
        ("invalid field identifier", |d| {
            d["components"]["schemas"]["Empty"]["properties"] =
                json!({ "explicit": { "type": "string" } });
        }),
        ("enum values must be distinct valid identifiers", |d| {
            d["components"]["schemas"]["Color"]["enum"] = json!(["in-progress"]);
        }),
        ("enum values must be distinct valid identifiers", |d| {
            d["components"]["schemas"]["Color"]["enum"] = json!(["FOO_BAR", "foo_bar"]);
        }),
        ("enum values must be distinct valid identifiers", |d| {
            d["components"]["schemas"]["Color"]["enum"] = json!(["_"]);
        }),
        ("method is not allowed", |d| {
            d["paths"]["/v1/widgets"]["summary"] = json!("Widget operations");
        }),
        ("requestBody example must be an object", |d| {
            d["paths"]["/v1/widgets"]["post"]["requestBody"]["content"][JSON]["example"] =
                json!("not-an-object");
        }),
        ("requestBody example must not be an object", |d| {
            d["paths"]["/v1/widgets"]["post"]["requestBody"]["content"][JSON]["schema"] =
                json!({ "$ref": "#/components/schemas/Color" });
            d["paths"]["/v1/widgets"]["post"]["requestBody"]["content"][JSON]["example"] =
                json!({});
        }),
        ("use OpenAPI 3.1 type array instead of nullable", |d| {
            d["components"]["schemas"]["Widget"]["properties"]["label"]["nullable"] = json!(true);
        }),
        ("omit empty required array", |d| {
            d["components"]["schemas"]["Empty"]["required"] = json!([]);
        }),
        ("responses must include \"200\"", |d| {
            d["paths"]["/v1/widgets"]["get"]["responses"] =
                json!({ "404": { "$ref": "#/components/responses/EmptyResponse" } });
        }),
    ];
    for &(expected, mutate) in cases {
        let mut doc = spec();
        mutate(&mut doc);
        let errors = validate(&doc);
        assert!(errors.iter().any(|e| e.contains(expected)), "{expected}: {errors:?}");
    }
}

#[test]
fn test_generate_rust() {
    let doc = spec();
    let json = serde_json::to_string_pretty(&doc).unwrap_or_default();
    let expected_body = "\
/// Widget color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    #[serde(rename = \"RED\")]
    Red,
    #[serde(rename = \"GREEN\")]
    Green,
}

/// Empty response body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Empty {}

/// A widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Widget {
    pub color: Color,
    pub name: String,
    /// Optional display label.
    #[serde(default, skip_serializing_if = \"Option::is_none\")]
    pub label: Option<String>,
}

/// Response body for `ListWidgets`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListWidgetsResponse {
    pub widgets: Vec<Widget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    ListWidgets,
    CreateWidget,
    GetWidget,
    DeleteWidget,
    ResetWidget,
}

pub const ROUTES: &[Route<Op>] = &[
    Route::new(Method::GET, \"/v1/widgets\", Op::ListWidgets),
    Route::new(Method::POST, \"/v1/widgets\", Op::CreateWidget),
    Route::new(Method::GET, \"/v1/widgets/{name}\", Op::GetWidget),
    Route::new(Method::DELETE, \"/v1/widgets/{name}\", Op::DeleteWidget),
    Route::new(Method::POST, \"/v1/widgets/{name}:reset\", Op::ResetWidget),
];
";
    let expected = format!(
        "/// OpenAPI 3.1 JSON document (from `openapi.yaml`) served by `GET /v1/openapi.json`.\npub const OPENAPI_JSON: &str = {json:?};\n\n{expected_body}"
    );
    assert_eq!(generate_rust(&doc), expected);

    // Multi-line descriptions emit `/// ` on every line.
    let multiline = json!({
        "components": {
            "schemas": {
                "Note": {
                    "type": "object",
                    "description": "Line 1.\nLine 2.",
                    "required": ["text"],
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Field line 1.\nField line 2."
                        }
                    }
                }
            }
        }
    });
    let rust = generate_rust(&multiline);
    assert!(rust.contains("/// Line 1.\n/// Line 2.\n"), "{rust}");
    assert!(rust.contains("    /// Field line 1.\n    /// Field line 2.\n"), "{rust}");
}

#[test]
fn test_netsim_openapi_spec() {
    let doc: Value = serde_json::from_str(netsim_rest_api::OPENAPI_JSON).unwrap();
    assert_eq!(validate(&doc), Vec::<String>::new());
    assert!(doc["components"]["schemas"]["ApCreate"].get("required").is_none());

    let ops: Vec<String> = netsim_rest_api::ROUTES.iter().map(Route::operation_id).collect();
    assert_eq!(
        ops,
        [
            "ListAps",
            "CreateAp",
            "PatchAp",
            "DeleteAp",
            "DisconnectAp",
            "ListDevices",
            "PatchDevice",
            "ResetDevices",
            "ListLinks",
            "GetVersion"
        ]
    );

    let cpp = generate_cpp(&doc);
    assert!(cpp.contains("struct VersionResponse {"), "{cpp}");
    assert!(cpp.contains("struct ListDeviceResponse {"), "{cpp}");
    assert!(cpp.contains("struct ListLinkResponse {"), "{cpp}");
    assert!(cpp.contains("struct ListApResponse {"), "{cpp}");
    assert!(cpp.contains("struct ApCreate {"), "{cpp}");
    assert!(cpp.contains("std::optional<std::string> ssid;"), "{cpp}");
    assert!(cpp.contains("struct RadioUpdate {"), "{cpp}");
    assert!(cpp.contains("std::optional<std::string> preset;"), "{cpp}");
    assert!(cpp.contains("struct ChipVariantUpdate {"), "{cpp}");
    assert!(cpp.contains("std::optional<ChipVariantUpdate> variant;"), "{cpp}");

    let py = generate_python(&doc);
    assert!(py.contains("class WifiMode(StrEnum):\n    A = \"a\""), "{py}");
    assert!(py.contains("class ListDeviceResponse:\n    devices: list[Device]"), "{py}");
    assert!(py.contains("class ApCreate:\n"), "{py}");
    assert!(py.contains("ssid: str | None = None"), "{py}");
}

#[test]
fn test_generate_cpp() {
    let expected = "\
#include <cstdint>
#include <optional>
#include <string>
#include <vector>
#include <nlohmann/json.hpp>

enum class Color {
  kRed,
  kGreen,
};
NLOHMANN_JSON_SERIALIZE_ENUM(Color, {
  {Color::kRed, \"RED\"},
  {Color::kGreen, \"GREEN\"},
})

struct Empty {};

struct Widget {
  Color color;
  std::string name;
  std::optional<std::string> label;
};
NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE_WITH_DEFAULT(Widget, color, name, label)

struct ListWidgetsResponse {
  std::vector<Widget> widgets;
};
NLOHMANN_DEFINE_TYPE_NON_INTRUSIVE_WITH_DEFAULT(ListWidgetsResponse, widgets)
";
    assert_eq!(generate_cpp(&spec()), expected);
}

#[test]
fn test_generate_python() {
    let expected = "\
from dataclasses import dataclass
from enum import StrEnum


class Color(StrEnum):
    RED = \"RED\"
    GREEN = \"GREEN\"


@dataclass
class Empty:
    pass


@dataclass
class Widget:
    color: Color
    name: str
    label: str | None = None


@dataclass
class ListWidgetsResponse:
    widgets: list[Widget]
";
    assert_eq!(generate_python(&spec()), expected);

    // Optional fields listed before required fields in JSON are still emitted
    // after required fields in Python `@dataclass`.
    let reordered = json!({
        "components": {
            "schemas": {
                "Item": {
                    "type": "object",
                    "required": ["id"],
                    "properties": {
                        "note": { "type": "string" },
                        "id": { "type": "string" }
                    }
                }
            }
        }
    });
    assert_eq!(
        generate_python(&reordered),
        "from dataclasses import dataclass\nfrom enum import StrEnum\n\n\n@dataclass\nclass Item:\n    id: str\n    note: str | None = None\n"
    );
}

#[test]
fn test_find_route_uses_table() {
    fn find(method: Method, path: &str) -> Option<(Op, Vec<(&str, &str)>)> {
        find_route(ROUTES, &method, path).map(|(r, params)| (r.op, params))
    }
    assert_eq!(find(Method::GET, "/v1/widgets"), Some((Op::ListWidgets, vec![])));
    assert_eq!(find(Method::POST, "/v1/widgets"), Some((Op::CreateWidget, vec![])));
    assert_eq!(find(Method::GET, "/v1/widgets/w1"), Some((Op::GetWidget, vec![("name", "w1")])));
    assert_eq!(
        find(Method::DELETE, "/v1/widgets/w1"),
        Some((Op::DeleteWidget, vec![("name", "w1")]))
    );
    assert_eq!(
        find(Method::POST, "/v1/widgets/w1:reset"),
        Some((Op::ResetWidget, vec![("name", "w1")]))
    );
    assert_eq!(find(Method::POST, "/v1/widgets/:reset"), None);
    assert_eq!(find(Method::PATCH, "/v1/widgets/w1"), None);
    assert_eq!(find(Method::GET, "/v1/widgets/w1/extra"), None);
    assert_eq!(find(Method::GET, "/v1/gadgets"), None);

    // Literal segments and AIP-136 `:verb` suffixes take precedence over bare
    // `{param}` placeholders even when listed later.
    let shadowed = [
        Route::new(Method::GET, "/v1/widgets/{name}", Op::GetWidget),
        Route::new(Method::GET, "/v1/widgets/all", Op::ListWidgets),
        Route::new(Method::POST, "/v1/widgets/{name}", Op::CreateWidget),
        Route::new(Method::POST, "/v1/widgets/{name}:reset", Op::ResetWidget),
    ];
    assert_eq!(
        find_route(&shadowed, &Method::GET, "/v1/widgets/all").map(|(r, _)| r.op),
        Some(Op::ListWidgets)
    );
    assert_eq!(
        find_route(&shadowed, &Method::POST, "/v1/widgets/w1:reset").map(|(r, p)| (r.op, p)),
        Some((Op::ResetWidget, vec![("name", "w1")]))
    );
}

#[test]
fn test_operation_ids_are_op_names() {
    let ids: Vec<String> = ROUTES.iter().map(Route::operation_id).collect();
    assert_eq!(ids, ["ListWidgets", "CreateWidget", "GetWidget", "DeleteWidget", "ResetWidget"]);
}

#[test]
fn test_handle_serves_document_and_dispatches() {
    assert_eq!(handle(&Method::GET, "/v1/openapi.json", b""), (200, spec()));

    let example =
        &spec()["paths"]["/v1/widgets"]["post"]["requestBody"]["content"][JSON]["example"];
    let body = serde_json::to_vec(example).unwrap_or_default();
    let (status, created) = handle(&Method::POST, "/v1/widgets", &body);
    assert_eq!(status, 200);
    assert_eq!(&created, example);

    assert_eq!(handle(&Method::POST, "/v1/widgets", b"{}").0, 400);
    assert_eq!(
        handle(&Method::GET, "/v1/widgets/w2", b""),
        (200, json!({ "name": "w2", "color": "RED" }))
    );
    assert_eq!(
        handle(&Method::GET, "/v1/widgets", b""),
        (200, json!({ "widgets": [{ "name": "w1", "color": "RED" }] }))
    );
    assert_eq!(handle(&Method::POST, "/v1/widgets/w2:reset", b""), (200, json!({})));
    assert_eq!(handle(&Method::DELETE, "/v1/widgets/w2", b""), (200, json!({})));
    assert_eq!(handle(&Method::GET, "/v1/gadgets", b"").0, 404);
}
