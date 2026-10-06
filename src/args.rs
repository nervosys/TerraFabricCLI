//! Turning flag strings into typed JSON arguments, and checking arguments
//! against a tool's input schema before anything is sent.
//!
//! Conventions (the same for every command, so agents can rely on them):
//! - a value starting with `{` or `[` is parsed as JSON;
//! - arrays also take comma-separated values: `--bbox 4.0,51.9,4.3,52.0`;
//! - booleans are flags (`--night`), or explicit (`--include-free=false`);
//! - secret values may be given as `env:NAME` or `file:PATH` so they never
//!   appear in the process list or shell history.

use serde_json::{Map, Number, Value};

use crate::spec::Param;

/// Convert one flag value according to its schema.
pub fn coerce(p: &Param, raw: &str) -> Result<Value, String> {
    let raw = if p.secret {
        resolve_secret(&p.flag, raw)?
    } else {
        raw.to_string()
    };
    let t = raw.trim();
    if t.starts_with('{') || (t.starts_with('[') && p.json_type() != "string") {
        return serde_json::from_str(t).map_err(|e| format!("--{}: not valid JSON: {e}", p.flag));
    }
    scalar_or_list(&p.schema, t).map_err(|e| format!("--{}: {e}", p.flag))
}

fn scalar_or_list(schema: &Value, t: &str) -> Result<Value, String> {
    // oneOf [bbox array, GeoJSON object]: a CSV value is the array branch.
    if let Some(alts) = schema.get("oneOf").and_then(Value::as_array) {
        if let Some(arr) = alts
            .iter()
            .find(|a| a.get("type").and_then(Value::as_str) == Some("array"))
        {
            return scalar_or_list(arr, t);
        }
        return Err("expects JSON".into());
    }
    match schema
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("string")
    {
        "array" => {
            let item = schema.get("items").cloned().unwrap_or(Value::Null);
            t.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| scalar(&item, s))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        "object" => Err("expects a JSON object".into()),
        _ => scalar(schema, t),
    }
}

fn scalar(schema: &Value, s: &str) -> Result<Value, String> {
    match schema
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("string")
    {
        "integer" => s
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| format!("'{s}' is not an integer")),
        "number" => s
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| format!("'{s}' is not a number")),
        "boolean" => match s.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(Value::Bool(true)),
            "false" | "no" | "0" => Ok(Value::Bool(false)),
            _ => Err(format!("'{s}' is not a boolean")),
        },
        _ => Ok(Value::String(s.to_string())),
    }
}

/// `env:NAME` and `file:PATH` indirection for secrets.
pub fn resolve_secret(flag: &str, raw: &str) -> Result<String, String> {
    if let Some(name) = raw.strip_prefix("env:") {
        return std::env::var(name)
            .map_err(|_| format!("--{flag}: environment variable {name} is not set"));
    }
    if let Some(path) = raw.strip_prefix("file:") {
        return std::fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("--{flag}: cannot read {path}: {e}"));
    }
    eprintln!("warning: --{flag} was given literally; prefer env:NAME or file:PATH so it stays out of shell history");
    Ok(raw.to_string())
}

/// Replace secret argument values, for anything printed.
pub fn redact(args: &Value, params: &[Param]) -> Value {
    let mut v = args.clone();
    if let Some(o) = v.as_object_mut() {
        for p in params.iter().filter(|p| p.secret) {
            if let Some(x) = o.get_mut(&p.key) {
                *x = Value::String("[redacted]".into());
            }
        }
    }
    v
}

/// Validate a value against the subset of JSON Schema the tools use:
/// type, required, properties, enum, minimum/maximum, minItems/maxItems,
/// maxLength, items and oneOf. Returns every problem, with its JSON pointer.
pub fn validate(schema: &Value, v: &Value) -> Vec<String> {
    let mut errs = Vec::new();
    check(schema, v, "", &mut errs);
    errs
}

fn check(schema: &Value, v: &Value, at: &str, errs: &mut Vec<String>) {
    let here = if at.is_empty() { "/" } else { at };
    if let Some(alts) = schema.get("oneOf").and_then(Value::as_array) {
        if !alts.iter().any(|a| validate(a, v).is_empty()) {
            errs.push(format!(
                "{here}: does not match any allowed form ({})",
                describe_alts(alts)
            ));
        }
        return;
    }
    let types: Vec<&str> = match &schema["type"] {
        Value::String(t) => vec![t.as_str()],
        Value::Array(types) => types.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    let matches = |ty: &str| match ty {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "integer" => v.as_i64().is_some() || v.as_u64().is_some(),
        "number" => v.is_number(),
        "null" => v.is_null(),
        _ => true,
    };
    if !types.is_empty() && !types.iter().any(|ty| matches(ty)) {
        errs.push(format!(
            "{here}: expected {}, got {}",
            types.join(" or "),
            kind(v)
        ));
        return;
    }
    if let Some(e) = schema.get("enum").and_then(Value::as_array) {
        if !e.contains(v) {
            let allowed: Vec<String> = e
                .iter()
                .map(|x| {
                    x.as_str()
                        .map(String::from)
                        .unwrap_or_else(|| x.to_string())
                })
                .collect();
            errs.push(format!(
                "{here}: {} is not one of: {}",
                v,
                allowed.join(", ")
            ));
        }
    }
    if let Some(n) = v.as_f64() {
        if let Some(min) = schema.get("minimum").and_then(Value::as_f64) {
            if n < min {
                errs.push(format!("{here}: {n} is below the minimum {min}"));
            }
        }
        if let Some(max) = schema.get("maximum").and_then(Value::as_f64) {
            if n > max {
                errs.push(format!("{here}: {n} is above the maximum {max}"));
            }
        }
    }
    if let (Some(s), Some(max)) = (v.as_str(), schema.get("maxLength").and_then(Value::as_u64)) {
        if s.chars().count() as u64 > max {
            errs.push(format!("{here}: longer than {max} characters"));
        }
    }
    if let Some(a) = v.as_array() {
        if let Some(min) = schema.get("minItems").and_then(Value::as_u64) {
            if (a.len() as u64) < min {
                errs.push(format!("{here}: needs at least {min} items"));
            }
        }
        if let Some(max) = schema.get("maxItems").and_then(Value::as_u64) {
            if a.len() as u64 > max {
                errs.push(format!("{here}: allows at most {max} items"));
            }
        }
        if let Some(items) = schema.get("items") {
            for (i, x) in a.iter().enumerate() {
                check(items, x, &format!("{at}/{i}"), errs);
            }
        }
    }
    if let Some(o) = v.as_object() {
        for r in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !o.contains_key(r) {
                errs.push(format!("{at}/{r}: required"));
            }
        }
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (k, x) in o {
                if let Some(ps) = props.get(k) {
                    check(ps, x, &format!("{at}/{k}"), errs);
                }
            }
        }
    }
}

fn describe_alts(alts: &[Value]) -> String {
    alts.iter()
        .map(|a| a.get("type").and_then(Value::as_str).unwrap_or("value"))
        .collect::<Vec<_>>()
        .join(" or ")
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Merge flag values over an `--input` object.
pub fn merge(base: Option<Value>, flags: Map<String, Value>) -> Result<Value, String> {
    let mut obj = match base {
        None => Map::new(),
        Some(Value::Object(o)) => o,
        Some(_) => return Err("--input must contain a JSON object".into()),
    };
    obj.extend(flags);
    Ok(Value::Object(obj))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::find;
    use serde_json::json;

    fn param(cmd: &[&str], key: &str) -> Param {
        find(cmd)
            .unwrap()
            .params
            .into_iter()
            .find(|p| p.key == key)
            .unwrap()
    }

    #[test]
    fn coerces_by_schema() {
        assert_eq!(
            coerce(
                &param(&["collections", "search"], "bbox"),
                "4.0, 51.9,4.3,52"
            )
            .unwrap(),
            json!([4.0, 51.9, 4.3, 52.0])
        );
        assert_eq!(
            coerce(&param(&["collections", "search"], "limit"), "5").unwrap(),
            json!(5)
        );
        assert_eq!(
            coerce(
                &param(&["collections", "search"], "modality"),
                "sar,optical"
            )
            .unwrap(),
            json!(["sar", "optical"])
        );
        assert_eq!(
            coerce(&param(&["collections", "search"], "night"), "false").unwrap(),
            json!(false)
        );
        // oneOf: CSV is a bbox, JSON is GeoJSON.
        assert_eq!(
            coerce(&param(&["osint", "assess"], "aoi"), "1,2,3,4").unwrap(),
            json!([1.0, 2.0, 3.0, 4.0])
        );
        assert_eq!(
            coerce(
                &param(&["osint", "assess"], "aoi"),
                r#"{"type":"Polygon","coordinates":[]}"#
            )
            .unwrap()["type"],
            "Polygon"
        );
        assert!(coerce(&param(&["collections", "search"], "limit"), "five").is_err());
    }

    #[test]
    fn validates_against_the_tool_schema() {
        let s = &find(&["collections", "search"]).unwrap().schema;
        assert!(validate(s, &json!({ "q": "radar", "limit": 5, "access": "free" })).is_empty());
        let errs = validate(
            s,
            &json!({ "limit": 1001, "access": "gratis", "bbox": [1, 2, 3] }),
        );
        assert_eq!(errs.len(), 3, "{errs:?}");
        assert!(
            errs.iter().any(|e| e.starts_with("/limit"))
                && errs.iter().any(|e| e.starts_with("/access"))
                && errs.iter().any(|e| e.starts_with("/bbox"))
        );
        let get = &find(&["collections", "get"]).unwrap().schema;
        assert_eq!(validate(get, &json!({})), vec!["/id: required".to_string()]);
        let assess = &find(&["osint", "assess"]).unwrap().schema;
        assert!(validate(assess, &json!({ "aoi": [1, 2, 3, 4] })).is_empty());
        assert!(!validate(assess, &json!({ "aoi": "rotterdam" })).is_empty());
    }

    #[test]
    fn secrets_resolve_and_redact() {
        std::env::set_var("TFAB_TEST_SECRET", "s3cr3t");
        let p = param(&["orders", "approve"], "principal_token");
        assert_eq!(coerce(&p, "env:TFAB_TEST_SECRET").unwrap(), json!("s3cr3t"));
        assert!(coerce(&p, "env:TFAB_TEST_UNSET_VARIABLE").is_err());
        let params = find(&["orders", "approve"]).unwrap().params;
        let shown = redact(&json!({ "id": "o1", "principal_token": "s3cr3t" }), &params);
        assert_eq!(shown["principal_token"], "[redacted]");
        assert_eq!(shown["id"], "o1");
    }

    #[test]
    fn flags_override_input() {
        let mut f = Map::new();
        f.insert("limit".into(), json!(3));
        assert_eq!(
            merge(Some(json!({ "q": "x", "limit": 9 })), f).unwrap(),
            json!({ "q": "x", "limit": 3 })
        );
        assert!(merge(Some(json!([1])), Map::new()).is_err());
    }
    #[test]
    fn nullable_types_and_sensitive_thermal_arguments_are_handled() {
        let schema = json!({"type":["boolean","null"]});
        assert!(validate(&schema, &json!(null)).is_empty());
        assert!(validate(&schema, &json!(false)).is_empty());
        assert!(!validate(&schema, &json!("false")).is_empty());
        let command = find(&["osint", "thermal-get"]).unwrap();
        assert!(command.params[0].secret);
        assert_eq!(
            redact(&json!({"id":"tr_private"}), &command.params)["id"],
            "[redacted]"
        );
        let command = find(&["osint", "thermal-save"]).unwrap();
        let input = json!({"model":{"private_site":"fixture"},"watch_id":"w_private"});
        let safe = redact(&input, &command.params);
        assert_eq!(safe["model"], "[redacted]");
        assert_eq!(safe["watch_id"], "[redacted]");
        assert_eq!(
            find(&["osint", "thermal-delete"]).unwrap().annotations()["destructiveHint"],
            true
        );
    }
}
