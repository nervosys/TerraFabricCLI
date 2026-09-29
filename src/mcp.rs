//! `tfab mcp`: the CLI's commands as a Model Context Protocol server on stdio
//! (newline-delimited JSON-RPC 2.0).
//!
//! Why proxy rather than point an agent at the server's own /mcp: credentials
//! (TFAB_TOKEN, OAuth client secrets) stay in this process's environment and
//! never enter the model's context, arguments are validated locally, and
//! spend tools are withheld unless the operator opts in.

use std::io::{BufRead, Write};
use std::time::Duration;

use serde_json::{json, Value};

use crate::client::{Client, Problem};
use crate::spec::{self, Effect};

pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Spend tools are exposed only when the operator sets TFAB_MCP_ALLOW_SPEND=true.
pub fn allow_spend() -> bool {
    std::env::var("TFAB_MCP_ALLOW_SPEND").is_ok_and(|v| v.eq_ignore_ascii_case("true") || v == "1")
}

/// Commands in MCP tool shape. Tool names are the server's, so an agent that
/// knows TerraFabric's tools needs nothing new.
pub fn tool_list(include_spend: bool) -> Vec<Value> {
    spec::commands()
        .into_iter()
        .filter(|c| include_spend || c.effect != Effect::CommitsSpend)
        .map(|c| {
            json!({
                "name": c.tool,
                "title": c.title,
                "description": format!("{} (CLI: `{}`; side effect: {}; credential: {})", c.description, c.line(), c.effect.id(), c.auth.id()),
                "inputSchema": c.schema,
                "annotations": c.annotations(),
                "_meta": { "tfab/command": c.line(), "tfab/iri": format!("{}/command/{}", crate::ontology::program_iri(), c.id()), "tfab/returns": c.returns },
            })
        })
        .collect()
}

fn resources() -> Vec<Value> {
    vec![
        json!({ "uri": "tfab://ontology.ttl", "name": "tfab CLI ontology (Turtle)", "mimeType": "text/turtle" }),
        json!({ "uri": "tfab://shapes.ttl", "name": "tfab SHACL shapes (Turtle)", "mimeType": "text/turtle" }),
        json!({ "uri": "tfab://guide.md", "name": "tfab agent guide", "mimeType": "text/markdown" }),
    ]
}

fn read_resource(uri: &str) -> Option<(&'static str, String)> {
    match uri {
        "tfab://ontology.ttl" => Some((
            "text/turtle",
            crate::ontology::to_turtle(&crate::ontology::graph()),
        )),
        "tfab://shapes.ttl" => Some((
            "text/turtle",
            crate::ontology::to_turtle(&crate::ontology::shapes_graph()),
        )),
        "tfab://guide.md" => Some(("text/markdown", crate::guide_text())),
        _ => None,
    }
}

pub struct Server {
    client: Client,
    spend: bool,
}

impl Server {
    pub fn new(client: Client, spend: bool) -> Self {
        Self { client, spend }
    }

    /// One JSON-RPC message in, at most one out (none for notifications).
    pub fn handle(&self, line: &str) -> Option<Value> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(
                    json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {e}") } }),
                )
            }
        };
        let id = msg.get("id").cloned()?; // notifications get no reply
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let ok = |r: Value| json!({ "jsonrpc": "2.0", "id": id, "result": r });
        let err = |code: i64, m: String| json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": m } });
        Some(match method {
            "initialize" => {
                let asked = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(PROTOCOL_VERSIONS[0]);
                let version = if PROTOCOL_VERSIONS.contains(&asked) {
                    asked
                } else {
                    PROTOCOL_VERSIONS[0]
                };
                ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false }, "resources": { "listChanged": false } },
                    "serverInfo": { "name": "tfab", "title": "TerraFabric CLI", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "TerraFabric Earth observation tools. Read tfab://guide.md first. Tools marked openWorldHint commit spend and are offered only when the operator allows it.",
                }))
            }
            "ping" => ok(json!({})),
            "tools/list" => ok(json!({ "tools": tool_list(self.spend) })),
            "tools/call" => ok(self.call(&params)),
            "resources/list" => ok(json!({ "resources": resources() })),
            "resources/read" => {
                let uri = params
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match read_resource(uri) {
                    Some((mime, text)) => {
                        ok(json!({ "contents": [{ "uri": uri, "mimeType": mime, "text": text }] }))
                    }
                    None => err(-32002, format!("resource not found: {uri}")),
                }
            }
            m => err(-32601, format!("method not found: {m}")),
        })
    }

    fn call(&self, params: &Value) -> Value {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        let failure = |p: Problem| json!({ "content": [{ "type": "text", "text": p.to_json().to_string() }], "structuredContent": p.to_json(), "isError": true });
        let Some(cmd) = spec::commands().into_iter().find(|c| c.tool == name) else {
            return failure(Problem::new("not-found", "Unknown tool", name.to_string()));
        };
        if cmd.effect == Effect::CommitsSpend && !self.spend {
            return failure(
                Problem::new(
                    "confirmation-required",
                    "Spend tools are disabled",
                    format!("'{name}' commits spend"),
                )
                .hint("the operator can enable them with TFAB_MCP_ALLOW_SPEND=true"),
            );
        }
        let errs = crate::args::validate(&cmd.schema, &args);
        if !errs.is_empty() {
            return failure(Problem::new(
                "invalid-input",
                "Arguments do not match the input schema",
                errs.join("; "),
            ));
        }
        match self.client.call(&cmd.tool, &args, cmd.auth) {
            Ok(v) => json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
                "structuredContent": if v.is_object() { v } else { json!({ "result": v }) },
                "isError": false,
            }),
            Err(p) => failure(p),
        }
    }
}

pub fn serve(base: String, agent_id: Option<String>, timeout: Duration) -> Result<(), Problem> {
    let server = Server::new(Client::new(&base, agent_id, timeout)?, allow_spend());
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| Problem::new("internal", "stdin", e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle(&line) {
            let written = writeln!(out, "{}", serde_json::to_string(&reply).unwrap_or_default())
                .and_then(|_| out.flush());
            if written.is_err() {
                break; // client went away
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(spend: bool) -> Server {
        Server::new(
            Client::new("https://unreachable.invalid", None, Duration::from_secs(1)).unwrap(),
            spend,
        )
    }

    #[test]
    fn handshake_and_listing() {
        let s = server(false);
        let init = s.handle(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert!(s
            .handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none());
        let tools = s
            .handle(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .unwrap();
        let names: Vec<&str> = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(
            names.contains(&"search_collections")
                && !names.contains(&"place_order")
                && !names.contains(&"submit_tasking")
        );
        assert_eq!(tool_list(true).len(), spec::commands().len());
    }

    #[test]
    fn spend_is_refused_unless_allowed_and_bad_input_never_leaves() {
        let s = server(false);
        let r = s.handle(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"place_order","arguments":{"quote_id":"q"}}}"#).unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(
            r["result"]["structuredContent"]["exit_status"],
            "confirmation-required"
        );
        let r = s.handle(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_collection","arguments":{}}}"#).unwrap();
        assert_eq!(
            r["result"]["structuredContent"]["exit_status"],
            "invalid-input"
        );
    }

    #[test]
    fn resources_and_errors() {
        let s = server(false);
        let r = s.handle(r#"{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"tfab://ontology.ttl"}}"#).unwrap();
        assert!(r["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("tf:CliCommand"));
        assert_eq!(
            s.handle(r#"{"jsonrpc":"2.0","id":6,"method":"nope"}"#)
                .unwrap()["error"]["code"],
            -32601
        );
        assert_eq!(s.handle("{not json").unwrap()["error"]["code"], -32700);
    }
}
