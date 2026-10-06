//! The command registry: the single source of truth for the parser, `describe`,
//! the MCP tool list and the ontology.
//!
//! Remote commands are generated from the server's tool manifest
//! (`ontology/tools.json`, written by `terrafabric export`), so the CLI takes
//! exactly the arguments the server's tools take. `ROUTES` only adds what the
//! manifest cannot know: a command path, the credential needed, what it
//! returns and examples. A tool with no route is still reachable as
//! `tfab call <tool>`, and a test fails until it gets one.

use serde_json::{json, Value};

/// The server tool manifest this build was generated from: a copy of what the
/// API serves at `/v1/agent/tools`, refreshed when the server's tools change.
pub const TOOLS_JSON: &str = include_str!("../ontology/tools.json");

pub const PROGRAM: &str = "tfab";
pub const DEFAULT_BASE_URL: &str = "https://terrafabric.world/api";

/// What running a command changes. Mirrors the MCP tool annotations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Reads only; safe to retry and to run speculatively.
    ReadOnly,
    /// Creates or updates server state (a watch, a mandate) but spends nothing.
    CreatesState,
    /// Commits money or tasks a sensor. Requires `--yes`.
    CommitsSpend,
    /// Runs locally; no network.
    Local,
}

impl Effect {
    pub fn id(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::CreatesState => "creates-state",
            Self::CommitsSpend => "commits-spend",
            Self::Local => "local",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read only",
            Self::CreatesState => "Creates state",
            Self::CommitsSpend => "Commits spend",
            Self::Local => "Local only",
        }
    }
    pub fn comment(self) -> &'static str {
        match self {
            Self::ReadOnly => "Reads data and changes nothing; safe to retry or run speculatively.",
            Self::CreatesState => "Creates or updates server-side state but commits no spend.",
            Self::CommitsSpend => {
                "Commits money or tasks a sensor; the CLI refuses to run it without --yes."
            }
            Self::Local => "Runs on the local machine without contacting the server.",
        }
    }
    pub const ALL: [Effect; 4] = [
        Self::ReadOnly,
        Self::CreatesState,
        Self::CommitsSpend,
        Self::Local,
    ];
}

/// The credential a command needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    None,
    /// An agent identity (`--agent-id` / `TFAB_AGENT_ID`) orders are placed for.
    AgentIdentity,
    /// A principal token, held by the human principal, never by the agent.
    Principal,
    /// Mission API, read scope.
    MissionRead,
    /// Mission API, task scope.
    MissionTask,
}

impl Auth {
    pub fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AgentIdentity => "agent-identity",
            Self::Principal => "principal",
            Self::MissionRead => "mission-read",
            Self::MissionTask => "mission-task",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No credential",
            Self::AgentIdentity => "Agent identity",
            Self::Principal => "Principal token",
            Self::MissionRead => "Mission API, read scope",
            Self::MissionTask => "Mission API, task scope",
        }
    }
    pub fn comment(self) -> &'static str {
        match self {
            Self::None => "Public; no credential is sent.",
            Self::AgentIdentity => "Acts for an agent identity (--agent-id or TFAB_AGENT_ID); spend is bounded by that agent's mandate.",
            Self::Principal => "Needs the principal token issued with the agent's mandate. Only the human principal holds it; an agent must never supply its own.",
            Self::MissionRead => "Mission API bearer credential with the read scope (TFAB_TOKEN, or OAuth client credentials).",
            Self::MissionTask => "Mission API bearer credential with the task scope (TFAB_TOKEN, or OAuth client credentials).",
        }
    }
    pub const ALL: [Auth; 5] = [
        Self::None,
        Self::AgentIdentity,
        Self::Principal,
        Self::MissionRead,
        Self::MissionTask,
    ];
}

/// How a server tool appears on the command line.
pub struct Route {
    pub tool: &'static str,
    pub path: &'static [&'static str],
    pub auth: Auth,
    /// Ontology class (CURIE) of what the command returns.
    pub returns: &'static str,
    pub examples: &'static [&'static str],
}

pub const ROUTES: &[Route] = &[
    Route { tool: "search_collections", path: &["collections", "search"], auth: Auth::None, returns: "tf:Collection",
        examples: &["tfab collections search --q radar --bbox 4.0,51.9,4.3,52.0 --limit 5", "tfab collections search --modality sar --access free --all-weather"] },
    Route { tool: "get_collection", path: &["collections", "get"], auth: Auth::None, returns: "tf:Collection", examples: &["tfab collections get --id sentinel-2-l2a"] },
    Route { tool: "search_items", path: &["items", "search"], auth: Auth::None, returns: "tf:Item",
        examples: &["tfab items search --collection sentinel-2-l2a --bbox 4.0,51.9,4.3,52.0 --datetime 2026-09-01/.. --max-cloud 20"] },
    Route { tool: "predict_passes", path: &["passes", "predict"], auth: Auth::None, returns: "tf:Satellite",
        examples: &["tfab passes predict --collection sentinel-1-grd --bbox 4.0,51.9,4.3,52.0"] },
    Route { tool: "list_providers", path: &["providers", "list"], auth: Auth::None, returns: "tf:Provider", examples: &["tfab providers list"] },
    Route { tool: "describe_ontology", path: &["ontology", "describe"], auth: Auth::None, returns: "owl:Ontology", examples: &["tfab ontology describe --module c2"] },
    Route { tool: "describe_term", path: &["ontology", "term"], auth: Auth::None, returns: "rdfs:Resource", examples: &["tfab ontology term --term tf:Collection"] },
    Route { tool: "query_graph", path: &["graph", "match"], auth: Auth::None, returns: "rdf:Statement", examples: &["tfab graph match --p rdf:type --o tf:Satellite --limit 20"] },
    Route { tool: "sparql_query", path: &["graph", "sparql"], auth: Auth::None, returns: "rdf:Statement",
        examples: &["tfab graph sparql --query 'PREFIX tf: <https://terrafabric.world/ont#> SELECT ?c WHERE { ?c a tf:Collection } LIMIT 5'"] },
    Route { tool: "get_quote", path: &["quotes", "create"], auth: Auth::None, returns: "tf:Quote",
        examples: &["tfab quotes create --collection maxar-worldview-legion --aoi 4.0,51.9,4.3,52.0 --mode archive"] },
    Route { tool: "set_mandate", path: &["mandates", "set"], auth: Auth::Principal, returns: "tf:SpendingMandate",
        examples: &["tfab mandates set --agent-id analyst-bot --principal ops-lead --max-order-usd 500 --budget-usd 5000"] },
    Route { tool: "place_order", path: &["orders", "place"], auth: Auth::AgentIdentity, returns: "tf:Order",
        examples: &["tfab --agent-id analyst-bot orders place --quote-id q_123 --yes"] },
    Route { tool: "get_order", path: &["orders", "get"], auth: Auth::None, returns: "tf:Order", examples: &["tfab orders get --id o_123"] },
    Route { tool: "approve_order", path: &["orders", "approve"], auth: Auth::Principal, returns: "tf:Order",
        examples: &["tfab orders approve --id o_123 --approver ops-lead --principal-token env:TFAB_PRINCIPAL_TOKEN --yes"] },
    Route { tool: "assess_coverage", path: &["c2", "coverage"], auth: Auth::MissionRead, returns: "tf:CoverageAssessment",
        examples: &["tfab c2 coverage --input requirement.json", "tfab c2 coverage --aoi 4.0,51.9,4.3,52.0 --intent 'port activity'"] },
    Route { tool: "submit_tasking", path: &["c2", "task"], auth: Auth::MissionTask, returns: "tf:TaskingRequest",
        examples: &["tfab c2 task --input tasking.json --yes"] },
    Route { tool: "get_tasking", path: &["c2", "status"], auth: Auth::MissionRead, returns: "tf:TaskingRequest", examples: &["tfab c2 status --id t_123"] },
    Route { tool: "osint_sources", path: &["osint", "sources"], auth: Auth::None, returns: "tf:Collection", examples: &["tfab osint sources"] },
    Route { tool: "osint_assess", path: &["osint", "assess"], auth: Auth::None, returns: "tf:CoverageAssessment",
        examples: &["tfab osint assess --aoi 4.0,51.9,4.3,52.0 --start 2026-09-01T00:00:00Z"] },
    Route { tool: "create_watch", path: &["osint", "watch-create"], auth: Auth::None, returns: "tf:AreaOfInterest",
        examples: &["tfab osint watch-create --aoi 4.0,51.9,4.3,52.0 --name rotterdam"] },
    Route { tool: "get_watch", path: &["osint", "watch-get"], auth: Auth::None, returns: "tf:AreaOfInterest", examples: &["tfab osint watch-get --id w_123"] },
    Route { tool: "analyze_thermal", path: &["osint", "thermal-analyze"], auth: Auth::None, returns: "tf:AgentAction", examples: &["tfab osint thermal-analyze --input thermal-args.json"] },
    Route { tool: "save_thermal_run", path: &["osint", "thermal-save"], auth: Auth::None, returns: "tf:AgentAction", examples: &["tfab osint thermal-save --input thermal-args.json"] },
    Route { tool: "get_thermal_run", path: &["osint", "thermal-get"], auth: Auth::None, returns: "tf:AgentAction", examples: &["tfab osint thermal-get --id tr_PRIVATE"] },
    Route { tool: "delete_thermal_run", path: &["osint", "thermal-delete"], auth: Auth::None, returns: "tf:AgentAction", examples: &["tfab osint thermal-delete --id tr_PRIVATE"] },

];

/// Group summaries, shown in help and typed in the ontology.
pub const GROUPS: &[(&str, &str)] = &[
    ("collections", "Discover Earth observation collections"),
    ("items", "Find scenes (STAC items) within a collection"),
    ("passes", "Predict satellite passes over an area"),
    ("providers", "Data providers"),
    ("ontology", "The TerraFabric ontology on the server"),
    (
        "graph",
        "Query the knowledge graph (triple patterns, SPARQL)",
    ),
    ("quotes", "Price an acquisition"),
    ("mandates", "Agent spending mandates (principal only)"),
    ("orders", "Place, track and approve orders"),
    (
        "c2",
        "Mission API: coverage and tasking (credential required)",
    ),
    ("osint", "Open-licence sources, assessment and watches"),
];

/// Commands that run without a server tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Local {
    Describe,
    Tools,
    Schema,
    Guide,
    ExitCodes,
    Doctor,
    Config,
    Call,
    Mcp,
    Completions,
}

pub struct LocalSpec {
    pub local: Local,
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// Local commands that still contact the server.
    pub network: bool,
    pub examples: &'static [&'static str],
}

pub const LOCALS: &[LocalSpec] = &[
    LocalSpec { local: Local::Describe, name: "describe", title: "Describe the CLI or one command",
        description: "Machine-readable description of the program (no argument) or of one command: input JSON Schema, side effect, credential, exit statuses and examples. Start here.",
        network: false, examples: &["tfab describe", "tfab describe collections search"] },
    LocalSpec { local: Local::Tools, name: "tools", title: "List commands as MCP tools",
        description: "Every command in Model Context Protocol tool shape (name, inputSchema, annotations), for agents that plan in tool terms.",
        network: false, examples: &["tfab tools"] },
    LocalSpec { local: Local::Schema, name: "schema", title: "Emit the CLI ontology",
        description: "The CLI's own ontology (OWL classes and properties, plus every command, option, environment variable and exit status as linked data) in Turtle or JSON-LD, linked to the TerraFabric ontology.",
        network: false, examples: &["tfab schema --format turtle", "tfab schema --format jsonld"] },
    LocalSpec { local: Local::Guide, name: "guide", title: "Agent orientation guide",
        description: "A concise Markdown orientation for AI agents: conventions, credentials, safety rules and the command list.",
        network: false, examples: &["tfab guide"] },
    LocalSpec { local: Local::ExitCodes, name: "exit-codes", title: "List exit statuses",
        description: "Every exit status with its meaning and whether retrying can help.",
        network: false, examples: &["tfab exit-codes"] },
    LocalSpec { local: Local::Doctor, name: "doctor", title: "Check connectivity and compatibility",
        description: "Reaches the server, checks which credentials are configured (never their values) and compares the server's tool manifest with the one this CLI was built from.",
        network: true, examples: &["tfab doctor"] },
    LocalSpec { local: Local::Config, name: "config", title: "Show effective configuration",
        description: "The effective base URL, output format, agent id and which credentials are present. Secret values are never shown.",
        network: false, examples: &["tfab config"] },
    LocalSpec { local: Local::Call, name: "call", title: "Invoke any server tool by name",
        description: "Invoke a server tool directly with JSON arguments (--input or --args). For tools newer than this CLI.",
        network: true, examples: &["tfab call search_collections --args '{\"q\":\"radar\",\"limit\":3}'"] },
    LocalSpec { local: Local::Mcp, name: "mcp", title: "Run as an MCP server (stdio)",
        description: "Serve every remote command as a Model Context Protocol tool over stdio. Credentials stay in the CLI's environment. Spend tools are offered only when TFAB_MCP_ALLOW_SPEND=true.",
        network: true, examples: &["tfab mcp"] },
    LocalSpec { local: Local::Completions, name: "completions", title: "Shell completions",
        description: "Print a completion script for bash, zsh, fish, powershell or elvish.",
        network: false, examples: &["tfab completions bash > /etc/bash_completion.d/tfab"] },
];

/// Exit statuses. Stable: agents branch on these.
pub struct ExitStatus {
    pub code: i32,
    pub name: &'static str,
    pub meaning: &'static str,
    pub retryable: bool,
}

pub const EXIT_STATUSES: &[ExitStatus] = &[
    ExitStatus { code: 0, name: "ok", meaning: "Success; the result is on stdout.", retryable: false },
    ExitStatus { code: 1, name: "internal", meaning: "Unexpected failure inside the CLI.", retryable: false },
    ExitStatus { code: 2, name: "usage", meaning: "Unknown command or malformed flags.", retryable: false },
    ExitStatus { code: 3, name: "not-found", meaning: "The requested collection, order, tasking or term does not exist.", retryable: false },
    ExitStatus { code: 4, name: "auth", meaning: "A credential is missing, invalid, or lacks the required scope.", retryable: false },
    ExitStatus { code: 5, name: "conflict", meaning: "The request conflicts with current state (for example a mandate owned by another principal).", retryable: false },
    ExitStatus { code: 6, name: "rate-limited", meaning: "Too many requests; wait for retry_after_s and retry.", retryable: true },
    ExitStatus { code: 7, name: "unavailable", meaning: "Network failure, timeout, or a server/upstream error.", retryable: true },
    ExitStatus { code: 8, name: "confirmation-required", meaning: "The command commits spend or tasks a sensor and --yes was not given.", retryable: false },
    ExitStatus { code: 9, name: "invalid-input", meaning: "Arguments failed validation against the command's input schema.", retryable: false },
];

pub fn exit_status(name: &str) -> &'static ExitStatus {
    EXIT_STATUSES
        .iter()
        .find(|e| e.name == name)
        .expect("known exit status")
}

/// Output formats.
pub const FORMATS: &[(&str, &str)] = &[
    (
        "json",
        "One JSON document (the default when stdout is not a terminal).",
    ),
    (
        "ndjson",
        "One JSON object per line: list results are streamed element by element.",
    ),
    (
        "text",
        "Human-readable summary (the default on a terminal).",
    ),
];

/// Options every command accepts.
pub struct GlobalOption {
    pub flag: &'static str,
    pub env: Option<&'static str>,
    pub value: Option<&'static str>,
    pub help: &'static str,
}

pub const GLOBAL_OPTIONS: &[GlobalOption] = &[
    GlobalOption { flag: "base-url", env: Some("TFAB_BASE_URL"), value: Some("URL"), help: "TerraFabric API base URL (default https://terrafabric.world/api)." },
    GlobalOption { flag: "format", env: Some("TFAB_FORMAT"), value: Some("FORMAT"), help: "Output format: json, ndjson or text (default: json unless stdout is a terminal)." },
    GlobalOption { flag: "agent-id", env: Some("TFAB_AGENT_ID"), value: Some("ID"), help: "Agent identity sent as x-agent-id; orders are bounded by this agent's mandate." },
    GlobalOption { flag: "input", env: None, value: Some("FILE"), help: "Read arguments as a JSON object from FILE, or '-' for stdin. Flags override its fields." },
    GlobalOption { flag: "dry-run", env: None, value: None, help: "Validate and print the request that would be sent, without sending it." },
    GlobalOption { flag: "yes", env: None, value: None, help: "Confirm a command that commits spend or tasks a sensor." },
    GlobalOption { flag: "timeout", env: Some("TFAB_TIMEOUT"), value: Some("SECONDS"), help: "Request timeout in seconds (default 60)." },
];

/// Environment variables read, besides the global-option ones.
pub const ENVIRONMENT: &[(&str, &str, bool)] = &[
    (
        "TFAB_TOKEN",
        "Bearer credential for the mission API (a shared key or an access token).",
        true,
    ),
    (
        "TFAB_OAUTH_TOKEN_URL",
        "OAuth 2.0 token endpoint for client-credentials authentication to the mission API.",
        false,
    ),
    ("TFAB_OAUTH_CLIENT_ID", "OAuth client id.", false),
    ("TFAB_OAUTH_CLIENT_SECRET", "OAuth client secret.", true),
    (
        "TFAB_OAUTH_SCOPES",
        "Space-separated OAuth scopes to request.",
        false,
    ),
    (
        "TFAB_MCP_ALLOW_SPEND",
        "Set to true to expose spend commands through `tfab mcp`.",
        false,
    ),
];

/// One input argument of a remote command, derived from the tool's JSON Schema.
#[derive(Debug, Clone)]
pub struct Param {
    pub key: String,
    pub flag: String,
    pub schema: Value,
    pub required: bool,
    pub description: String,
    /// Tokens are never echoed; they may be passed as `env:NAME` or `file:PATH`.
    pub secret: bool,
}

impl Param {
    pub fn json_type(&self) -> &str {
        self.schema
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("object")
    }
    pub fn is_array(&self) -> bool {
        self.json_type() == "array"
    }
    pub fn enum_values(&self) -> Vec<String> {
        let e = if self.is_array() {
            self.schema.pointer("/items/enum")
        } else {
            self.schema.get("enum")
        };
        e.and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// XSD datatype for the ontology.
    pub fn xsd(&self) -> &'static str {
        let t = if self.is_array() {
            self.schema
                .pointer("/items/type")
                .and_then(Value::as_str)
                .unwrap_or("string")
        } else {
            self.json_type()
        };
        match t {
            "integer" => "xsd:integer",
            "number" => "xsd:decimal",
            "boolean" => "xsd:boolean",
            "string" => match self.schema.get("format").and_then(Value::as_str) {
                Some("date-time") => "xsd:dateTime",
                _ => "xsd:string",
            },
            _ => "rdf:JSON",
        }
    }
}

/// A remote command: one server tool.
#[derive(Debug, Clone)]
pub struct Command {
    pub path: Vec<String>,
    pub tool: String,
    pub title: String,
    pub description: String,
    pub schema: Value,
    pub params: Vec<Param>,
    pub effect: Effect,
    pub idempotent: bool,
    pub destructive: bool,
    pub auth: Auth,
    pub returns: String,
    pub examples: Vec<String>,
}

impl Command {
    pub fn id(&self) -> String {
        self.path.join("-")
    }
    pub fn line(&self) -> String {
        format!("{PROGRAM} {}", self.path.join(" "))
    }
    /// Exit statuses this command can end with.
    pub fn exit_statuses(&self) -> Vec<&'static ExitStatus> {
        let mut names = vec![
            "ok",
            "internal",
            "usage",
            "not-found",
            "rate-limited",
            "unavailable",
            "invalid-input",
        ];
        if self.auth != Auth::None || self.effect != Effect::ReadOnly {
            names.push("auth");
            names.push("conflict");
        }
        if self.effect == Effect::CommitsSpend {
            names.push("confirmation-required");
        }
        EXIT_STATUSES
            .iter()
            .filter(|e| names.contains(&e.name))
            .collect()
    }
    pub fn annotations(&self) -> Value {
        json!({
            "readOnlyHint": self.effect == Effect::ReadOnly,
            "destructiveHint": self.destructive,
            "idempotentHint": self.idempotent || self.effect == Effect::ReadOnly,
            "openWorldHint": self.effect == Effect::CommitsSpend,
        })
    }
}

fn effect_of(annotations: &Value) -> (Effect, bool) {
    let b = |k: &str| annotations.get(k).and_then(Value::as_bool);
    let idempotent = b("idempotentHint").unwrap_or(false);
    if b("readOnlyHint") == Some(true) {
        (Effect::ReadOnly, true)
    } else if b("openWorldHint") == Some(true) {
        (Effect::CommitsSpend, idempotent)
    } else {
        (Effect::CreatesState, idempotent)
    }
}

/// Parameters from an input schema, in schema order with required first.
pub fn params_of(schema: &Value) -> Vec<Param> {
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut out: Vec<Param> = schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| {
            props
                .iter()
                .map(|(k, s)| Param {
                    key: k.clone(),
                    flag: k.replace('_', "-"),
                    schema: s.clone(),
                    required: required.contains(&k.as_str()),
                    description: s
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    secret: k.contains("token") || k.contains("secret") || s["x-sensitive"] == true,
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|p| !p.required);
    out
}

/// The server tool manifest this CLI was built from.
pub fn manifest() -> Vec<Value> {
    serde_json::from_str(TOOLS_JSON).expect("ontology/tools.json is valid JSON")
}

/// Every remote command, in manifest order.
pub fn commands() -> Vec<Command> {
    manifest()
        .into_iter()
        .filter_map(|t| {
            let tool = t.get("name")?.as_str()?.to_string();
            let route = ROUTES.iter().find(|r| r.tool == tool)?;
            let schema = t
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
            let (effect, idempotent) = effect_of(t.get("annotations").unwrap_or(&Value::Null));
            Some(Command {
                path: route.path.iter().map(|s| s.to_string()).collect(),
                title: t
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(&tool)
                    .to_string(),
                description: t
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                params: params_of(&schema),
                schema,
                effect,
                idempotent,
                destructive: t["annotations"]["destructiveHint"] == true,
                auth: route.auth,
                returns: route.returns.to_string(),
                examples: route.examples.iter().map(|s| s.to_string()).collect(),
                tool,
            })
        })
        .collect()
}

/// Find a remote command by its path words.
pub fn find(path: &[&str]) -> Option<Command> {
    commands()
        .into_iter()
        .find(|c| c.path.iter().map(String::as_str).eq(path.iter().copied()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_server_tool_has_a_route_and_every_route_a_tool() {
        let tools: Vec<String> = manifest()
            .iter()
            .filter_map(|t| t["name"].as_str().map(String::from))
            .collect();
        for t in &tools {
            assert!(
                ROUTES.iter().any(|r| r.tool == t),
                "server tool '{t}' has no CLI route; add one to ROUTES"
            );
        }
        for r in ROUTES {
            assert!(
                tools.iter().any(|t| t == r.tool),
                "route for unknown tool '{}'",
                r.tool
            );
        }
        assert_eq!(commands().len(), tools.len());
    }

    #[test]
    fn paths_are_unique_and_grouped() {
        let cmds = commands();
        for (i, c) in cmds.iter().enumerate() {
            assert!(
                cmds[i + 1..].iter().all(|d| d.path != c.path),
                "duplicate path {:?}",
                c.path
            );
            assert!(
                GROUPS.iter().any(|(g, _)| *g == c.path[0]),
                "group '{}' is not declared",
                c.path[0]
            );
            assert!(
                LOCALS.iter().all(|l| l.name != c.path[0]),
                "'{}' collides with a local command",
                c.path[0]
            );
        }
    }

    #[test]
    fn spend_follows_the_server_annotations() {
        for name in ["place_order", "approve_order", "submit_tasking"] {
            assert_eq!(
                commands().iter().find(|c| c.tool == name).unwrap().effect,
                Effect::CommitsSpend,
                "{name}"
            );
        }
        assert_eq!(
            find(&["collections", "search"]).unwrap().effect,
            Effect::ReadOnly
        );
        assert_eq!(
            find(&["osint", "watch-create"]).unwrap().effect,
            Effect::CreatesState
        );
    }

    #[test]
    fn secrets_are_recognised() {
        let approve = find(&["orders", "approve"]).unwrap();
        assert!(approve
            .params
            .iter()
            .any(|p| p.key == "principal_token" && p.secret));
        assert!(approve.params.iter().filter(|p| p.required).count() >= 3);
    }

    /// Inside the TerraFabric source tree, the copy must match the server's
    /// export exactly. Standalone (the published crate) there is nothing to compare.
    #[test]
    fn manifest_copy_matches_the_server_export() {
        let upstream =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ontology/tools.json");
        if let Ok(server) = std::fs::read_to_string(&upstream) {
            let norm = |s: &str| s.replace("\r\n", "\n");
            assert!(
                norm(&server) == norm(TOOLS_JSON),
                "cli/ontology/tools.json is stale: copy ontology/tools.json over it"
            );
        }
    }

    #[test]
    fn exit_codes_are_unique() {
        for (i, e) in EXIT_STATUSES.iter().enumerate() {
            assert!(EXIT_STATUSES[i + 1..]
                .iter()
                .all(|f| f.code != e.code && f.name != e.name));
        }
    }
}
