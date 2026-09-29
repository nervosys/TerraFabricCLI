//! Result formatting. stdout only ever carries results.

use std::io::Write;

use serde_json::Value;

use crate::client::Problem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Ndjson,
    Text,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "json" => Some(Self::Json),
            "ndjson" => Some(Self::Ndjson),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Ndjson => "ndjson",
            Self::Text => "text",
        }
    }
}

/// Keys that hold the list a command returns, in preference order.
const LIST_KEYS: &[&str] = &[
    "results",
    "features",
    "items",
    "collections",
    "providers",
    "sources",
    "matches",
    "passes",
    "options",
    "tools",
    "commands",
    "exit_statuses",
    "triples",
    "bindings",
];

/// The list inside a result, if it has one.
pub fn list_of(v: &Value) -> Option<&Vec<Value>> {
    if let Some(a) = v.as_array() {
        return Some(a);
    }
    for k in LIST_KEYS {
        if let Some(a) = v.get(*k).and_then(Value::as_array) {
            return Some(a);
        }
    }
    v.pointer("/results/bindings").and_then(Value::as_array)
}

pub fn render(v: &Value, f: Format) -> String {
    match f {
        Format::Json => format!("{}\n", serde_json::to_string_pretty(v).unwrap_or_default()),
        Format::Ndjson => match list_of(v) {
            Some(items) => items
                .iter()
                .map(|i| format!("{}\n", serde_json::to_string(i).unwrap_or_default()))
                .collect(),
            None => format!("{}\n", serde_json::to_string(v).unwrap_or_default()),
        },
        Format::Text => text(v),
    }
}

/// A compact human view: one line per list element, else indented JSON.
fn text(v: &Value) -> String {
    let Some(items) = list_of(v) else {
        return format!("{}\n", serde_json::to_string_pretty(v).unwrap_or_default());
    };
    let mut out = String::new();
    if let Some(total) = v.get("total").and_then(Value::as_u64) {
        out.push_str(&format!("{} of {total}\n", items.len()));
    }
    for i in items {
        let pick = |k: &str| {
            i.get(k).and_then(|x| {
                x.as_str()
                    .map(String::from)
                    .or_else(|| x.as_f64().map(|n| n.to_string()))
            })
        };
        let id = pick("id")
            .or_else(|| pick("name"))
            .or_else(|| pick("command"))
            .or_else(|| pick("code"))
            .unwrap_or_default();
        let label = pick("title")
            .or_else(|| pick("summary"))
            .or_else(|| pick("meaning"))
            .or_else(|| pick("description"))
            .unwrap_or_default();
        let label: String = label.chars().take(100).collect();
        if id.is_empty() && label.is_empty() {
            out.push_str(&format!(
                "{}\n",
                serde_json::to_string(i).unwrap_or_default()
            ));
        } else {
            out.push_str(&format!("{id:<32} {label}\n"));
        }
    }
    out
}

pub fn emit(v: &Value, f: Format) -> Result<(), Problem> {
    let mut o = std::io::stdout().lock();
    match o.write_all(render(v, f).as_bytes()).and_then(|_| o.flush()) {
        Ok(()) => Ok(()),
        // A closed pipe (`tfab ... | head`) is not an error.
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(Problem::new(
            "internal",
            "Cannot write output",
            e.to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ndjson_streams_lists() {
        let v = json!({ "total": 2, "results": [{ "id": "a" }, { "id": "b" }] });
        assert_eq!(
            render(&v, Format::Ndjson),
            "{\"id\":\"a\"}\n{\"id\":\"b\"}\n"
        );
        assert_eq!(
            render(&json!({ "id": "x" }), Format::Ndjson),
            "{\"id\":\"x\"}\n"
        );
    }

    #[test]
    fn text_summarises_lists() {
        let v = json!({ "total": 9, "results": [{ "id": "sentinel-2-l2a", "title": "Sentinel-2 L2A" }] });
        let t = render(&v, Format::Text);
        assert!(
            t.starts_with("1 of 9\n")
                && t.contains("sentinel-2-l2a")
                && t.contains("Sentinel-2 L2A")
        );
    }

    #[test]
    fn json_round_trips() {
        let v = json!({ "a": [1, 2], "b": { "c": null } });
        assert_eq!(
            serde_json::from_str::<Value>(&render(&v, Format::Json)).unwrap(),
            v
        );
    }
}
