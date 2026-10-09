//! tfab: agent-first command line for TerraFabric.
//!
//! Design rules, all enforced by tests:
//! - Every command is described by the registry (spec.rs), from which the
//!   parser, `describe`, `tools`, the MCP server and the ontology are built.
//! - stdout carries only results (JSON unless on a terminal); stderr carries
//!   RFC 9457 problem JSON; the exit status is stable and documented.
//! - Nothing that commits spend runs without `--yes`; `--dry-run` shows the
//!   exact request instead of sending it; secrets are never printed.

mod agent;
mod args;
mod client;
mod mcp;
mod ontology;
mod output;
mod spec;

use std::io::{IsTerminal, Read};
use std::time::Duration;

use clap::{Arg, ArgAction, ArgMatches};
use serde_json::{json, Map, Value};

use client::{Client, Problem};
use output::Format;
use spec::{Command, Effect, Local, EXIT_STATUSES, GLOBAL_OPTIONS, GROUPS, LOCALS, PROGRAM};

fn main() {
    let code = match run(std::env::args_os().collect()) {
        Ok(()) => 0,
        Err(p) => {
            eprintln!(
                "{}",
                serde_json::to_string(&p.to_json()).unwrap_or_default()
            );
            p.code()
        }
    };
    std::process::exit(code);
}

/// The full clap tree.
pub fn cli() -> clap::Command {
    let mut root = clap::Command::new(PROGRAM)
        .version(env!("CARGO_PKG_VERSION"))
        .about("Agent-first command line for TerraFabric: Earth observation discovery, pricing, orders, C2 tasking and OSINT.")
        .after_help("Agents: run `tfab guide` for conventions, `tfab describe <command>` for any command's input schema, and `tfab exit-codes` for exit statuses.")
        .subcommand_required(true)
        .arg_required_else_help(true);
    for g in GLOBAL_OPTIONS {
        let mut a = Arg::new(g.flag).long(g.flag).global(true).help(g.help);
        a = match g.value {
            Some(v) => a.value_name(v).num_args(1),
            None => a.action(ArgAction::SetTrue),
        };
        if let Some(env) = g.env {
            a = a.env(env);
        }
        if g.flag == "format" {
            a = a.value_parser(["json", "ndjson", "text"]);
        }
        root = root.arg(a);
    }

    for (group, about) in GROUPS {
        let mut gc = clap::Command::new(*group)
            .about(*about)
            .subcommand_required(true)
            .arg_required_else_help(true);
        for c in spec::commands().into_iter().filter(|c| c.path[0] == *group) {
            gc = gc.subcommand(command_for(&c));
        }
        root = root.subcommand(gc);
    }

    for l in LOCALS {
        let mut c = clap::Command::new(l.name)
            .about(l.title)
            .long_about(l.description);
        c =
            match l.local {
                Local::Describe => c.arg(
                    Arg::new("command")
                        .num_args(0..)
                        .help("Command words, e.g. `collections search`"),
                ),
                Local::Schema => c
                    .arg(
                        Arg::new("rdf")
                            .long("rdf")
                            .value_parser(["turtle", "jsonld"])
                            .default_value("turtle")
                            .help("RDF serialisation"),
                    )
                    .arg(
                        Arg::new("shapes")
                            .long("shapes")
                            .action(ArgAction::SetTrue)
                            .help("Emit the SHACL shapes instead of the ontology"),
                    ),
                Local::Call => c
                    .arg(Arg::new("tool").required(true).help(
                        "Server tool name (see `tfab tools` or the server's /v1/agent/tools)",
                    ))
                    .arg(
                        Arg::new("args")
                            .long("args")
                            .value_name("JSON")
                            .help("Tool arguments as a JSON object"),
                    ),
                Local::Ask => c
                    .arg(
                        Arg::new("question")
                            .required(true)
                            .num_args(1..)
                            .help("The question, in plain language"),
                    )
                    .arg(
                        Arg::new("engine")
                            .long("engine")
                            .env("TFAB_ENGINE")
                            .value_name("NAME|URL")
                            .help("ironworks, ollama, vllm, sglang, or the engine's base URL (default: the first local engine that answers)"),
                    )
                    .arg(
                        Arg::new("model")
                            .long("model")
                            .env("TFAB_MODEL")
                            .value_name("MODEL")
                            .help("Model to use (default: the first local model the engine lists)"),
                    )
                    .arg(
                        Arg::new("max-steps")
                            .long("max-steps")
                            .value_name("N")
                            .default_value("8")
                            .value_parser(clap::value_parser!(u8).range(1..=24))
                            .help("Most model turns before stopping"),
                    )
                    .arg(
                        Arg::new("max-tokens")
                            .long("max-tokens")
                            .value_name("N")
                            .default_value("4096")
                            .value_parser(clap::value_parser!(u32).range(64..=32768))
                            .help("Most tokens the model may generate per turn"),
                    )
                    .arg(
                        Arg::new("engine-timeout")
                            .long("engine-timeout")
                            .value_name("SECONDS")
                            .default_value("300")
                            .value_parser(clap::value_parser!(u64).range(5..=3600))
                            .help("Seconds allowed for one model call"),
                    ),
                Local::Completions => c.arg(
                    Arg::new("shell")
                        .required(true)
                        .value_parser(clap::value_parser!(clap_complete::Shell)),
                ),
                _ => c,
            };
        root = root.subcommand(c);
    }
    root
}

fn command_for(c: &Command) -> clap::Command {
    let mut after = String::new();
    after.push_str(&format!(
        "Side effect: {}. Credential: {}. Returns: {}.\n",
        c.effect.label(),
        c.auth.label(),
        c.returns
    ));
    if c.effect == Effect::CommitsSpend {
        after.push_str("Commits spend or tasks a sensor: requires --yes.\n");
    }
    after.push_str("\nExamples:\n");
    for e in &c.examples {
        after.push_str(&format!("  {e}\n"));
    }
    let mut cmd = clap::Command::new(c.path[1].clone())
        .about(c.title.clone())
        .long_about(c.description.clone())
        .after_help(after);
    for p in c.params.iter().filter(|p| !is_global(p)) {
        let mut help = p.description.clone();
        if p.secret {
            help.push_str(" (secret: pass env:NAME or file:PATH)");
        }
        // Required-ness is checked against the merged --input + flags, not by clap.
        let mut a = Arg::new(p.key.clone())
            .long(p.flag.clone())
            .help(help)
            .value_name(p.key.to_uppercase());
        if p.json_type() == "boolean" {
            a = a
                .num_args(0..=1)
                .default_missing_value("true")
                .require_equals(true)
                .value_name("BOOL");
        } else {
            a = a.num_args(1);
            if p.is_array() {
                a = a.action(ArgAction::Append);
            }
        }
        let allowed = p.enum_values();
        if !allowed.is_empty() && !p.is_array() {
            a = a.value_parser(clap::builder::PossibleValuesParser::new(allowed));
        }
        cmd = cmd.arg(a);
    }
    cmd
}

/// A tool argument whose flag is also a global option (`agent_id`) takes the
/// global's value instead of a second, conflicting flag.
fn is_global(p: &spec::Param) -> bool {
    GLOBAL_OPTIONS.iter().any(|g| g.flag == p.flag)
}

struct Globals {
    format: Format,
    base: String,
    agent_id: Option<String>,
    input: Option<String>,
    dry_run: bool,
    yes: bool,
    timeout: Duration,
}

fn globals(m: &ArgMatches) -> Result<Globals, Problem> {
    let s = |k: &str| m.get_one::<String>(k).cloned();
    let format = match s("format").as_deref() {
        Some(f) => Format::parse(f)
            .ok_or_else(|| Problem::new("usage", "Unknown format", f.to_string()))?,
        None if std::io::stdout().is_terminal() => Format::Text,
        None => Format::Json,
    };
    let timeout = match s("timeout") {
        Some(t) => t
            .parse::<u64>()
            .map_err(|_| Problem::new("usage", "Invalid --timeout", t))?,
        None => 60,
    };
    Ok(Globals {
        format,
        base: s("base-url").unwrap_or_else(|| spec::DEFAULT_BASE_URL.to_string()),
        agent_id: s("agent-id").filter(|a| !a.is_empty()),
        input: s("input"),
        dry_run: m.get_flag("dry-run"),
        yes: m.get_flag("yes"),
        timeout: Duration::from_secs(timeout.max(1)),
    })
}

pub fn run(argv: Vec<std::ffi::OsString>) -> Result<(), Problem> {
    let m = match cli().try_get_matches_from(argv) {
        Ok(m) => m,
        Err(e) => {
            use clap::error::ErrorKind::*;
            if matches!(
                e.kind(),
                DisplayHelp | DisplayVersion | DisplayHelpOnMissingArgumentOrSubcommand
            ) {
                print!("{e}");
                return Ok(());
            }
            let text = e.render().to_string();
            return Err(Problem::new(
                "usage",
                "Invalid command line",
                text.lines()
                    .next()
                    .unwrap_or("")
                    .trim_start_matches("error: ")
                    .to_string(),
            )
            .hint("run `tfab describe` for every command, or `tfab <command> --help`"));
        }
    };
    let (name, sub) = m.subcommand().expect("subcommand_required");
    let g = globals(sub).or_else(|_| globals(&m))?;

    if let Some(l) = LOCALS.iter().find(|l| l.name == name) {
        return local(l.local, sub, &g);
    }
    let (leaf, lm) = sub.subcommand().expect("subcommand_required");
    let cmd = spec::find(&[name, leaf])
        .ok_or_else(|| Problem::new("usage", "Unknown command", format!("{name} {leaf}")))?;
    let args = collect_args(&cmd, lm, g.input.as_deref(), g.agent_id.as_deref())?;
    execute(&cmd, args, &g)
}

/// Flags (and --input) → the tool's JSON input, validated.
fn collect_args(
    cmd: &Command,
    m: &ArgMatches,
    input: Option<&str>,
    agent_id: Option<&str>,
) -> Result<Value, Problem> {
    let mut flags = Map::new();
    for p in cmd.params.iter().filter(|p| is_global(p)) {
        if let (Some(a), "agent_id") = (agent_id, p.key.as_str()) {
            flags.insert(p.key.clone(), json!(a));
        }
    }
    for p in cmd.params.iter().filter(|p| !is_global(p)) {
        let values: Vec<String> = m
            .get_many::<String>(&p.key)
            .map(|v| v.cloned().collect())
            .unwrap_or_default();
        if values.is_empty() {
            continue;
        }
        let v = if p.is_array() && values.len() > 1 {
            // Repeated flag: each occurrence may itself be a CSV list.
            let mut all = Vec::new();
            for raw in &values {
                match args::coerce(p, raw)
                    .map_err(|e| Problem::new("invalid-input", "Invalid argument", e))?
                {
                    Value::Array(a) => all.extend(a),
                    x => all.push(x),
                }
            }
            Value::Array(all)
        } else {
            args::coerce(p, &values[0])
                .map_err(|e| Problem::new("invalid-input", "Invalid argument", e))?
        };
        flags.insert(p.key.clone(), v);
    }
    let base = input.map(read_input).transpose()?;
    let merged = args::merge(base, flags)
        .map_err(|e| Problem::new("invalid-input", "Invalid --input", e))?;
    let errs = args::validate(&cmd.schema, &merged);
    if !errs.is_empty() {
        return Err(Problem::new(
            "invalid-input",
            "Arguments do not match the input schema",
            errs.join("; "),
        )
        .hint(format!(
            "run `tfab describe {}` for the schema",
            cmd.path.join(" ")
        )));
    }
    Ok(merged)
}

fn read_input(src: &str) -> Result<Value, Problem> {
    let text = if src == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| Problem::new("invalid-input", "Cannot read stdin", e.to_string()))?;
        s
    } else {
        std::fs::read_to_string(src).map_err(|e| {
            Problem::new(
                "invalid-input",
                "Cannot read --input",
                format!("{src}: {e}"),
            )
        })?
    };
    serde_json::from_str(&text)
        .map_err(|e| Problem::new("invalid-input", "--input is not JSON", e.to_string()))
}

fn execute(cmd: &Command, args: Value, g: &Globals) -> Result<(), Problem> {
    let client = Client::new(&g.base, g.agent_id.clone(), g.timeout)?;
    if g.dry_run {
        let plan = json!({
            "dry_run": true,
            "command": cmd.line(),
            "tool": cmd.tool,
            "request": { "method": "POST", "url": client.tool_url(&cmd.tool), "body": args::redact(&args, &cmd.params) },
            "headers": {
                "x-agent-id": if cmd.auth == spec::Auth::Principal { Value::Null } else { json!(g.agent_id) },
                "authorization": if matches!(cmd.auth, spec::Auth::MissionRead | spec::Auth::MissionTask) { json!(format!("Bearer [from {}]", client::credential_source())) } else { Value::Null },
            },
            "side_effect": cmd.effect.id(),
            "would_require_yes": cmd.effect == Effect::CommitsSpend,
        });
        return output::emit(&plan, g.format);
    }
    if cmd.effect == Effect::CommitsSpend && !g.yes {
        return Err(Problem::new("confirmation-required", "Confirmation required", format!("`{}` {}", cmd.line(), "commits spend or tasks a sensor"))
            .hint("re-run with --yes once the principal has approved, or use --dry-run to inspect the request"));
    }
    if cmd.tool == "place_order" && g.agent_id.is_none() && args.get("agent_id").is_none() {
        return Err(Problem::new(
            "invalid-input",
            "Agent identity required",
            "orders are placed for an agent",
        )
        .hint("pass --agent-id or set TFAB_AGENT_ID"));
    }
    let result = client.call(&cmd.tool, &args, cmd.auth)?;
    output::emit(&result, g.format)
}

fn local(which: Local, m: &ArgMatches, g: &Globals) -> Result<(), Problem> {
    match which {
        Local::Describe => {
            let words: Vec<&str> = m
                .get_many::<String>("command")
                .map(|v| v.map(String::as_str).collect())
                .unwrap_or_default();
            output::emit(&describe(&words)?, g.format)
        }
        Local::Tools => output::emit(&json!({ "tools": mcp::tool_list(true) }), g.format),
        Local::Schema => {
            let graph = if m.get_flag("shapes") {
                ontology::shapes_graph()
            } else {
                ontology::graph()
            };
            match m.get_one::<String>("rdf").map(String::as_str) {
                Some("jsonld") => output::emit(&ontology::to_jsonld(&graph), Format::Json),
                _ => {
                    print!("{}", ontology::to_turtle(&graph));
                    Ok(())
                }
            }
        }
        Local::Guide => {
            print!("{}", guide_text());
            Ok(())
        }
        Local::ExitCodes => output::emit(
            &json!({ "exit_statuses": EXIT_STATUSES.iter().map(|e| json!({ "code": e.code, "name": e.name, "meaning": e.meaning, "retryable": e.retryable })).collect::<Vec<_>>() }),
            g.format,
        ),
        Local::Config => output::emit(&config(g), g.format),
        Local::Doctor => doctor(g),
        Local::Call => {
            let tool = m.get_one::<String>("tool").expect("required");
            let mut args = match m.get_one::<String>("args") {
                Some(a) => serde_json::from_str(a).map_err(|e| {
                    Problem::new("invalid-input", "--args is not JSON", e.to_string())
                })?,
                None => json!({}),
            };
            if let Some(i) = &g.input {
                args = args::merge(
                    Some(read_input(i)?),
                    args.as_object().cloned().unwrap_or_default(),
                )
                .map_err(|e| Problem::new("invalid-input", "Invalid --input", e))?;
            }
            match spec::commands().into_iter().find(|c| &c.tool == tool) {
                // A known tool goes through the same checks as its command.
                Some(c) => {
                    let errs = args::validate(&c.schema, &args);
                    if !errs.is_empty() {
                        return Err(Problem::new(
                            "invalid-input",
                            "Arguments do not match the input schema",
                            errs.join("; "),
                        ));
                    }
                    execute(&c, args, g)
                }
                None => {
                    // Newer than this build: annotations are unknown, so treat it as spend.
                    if !g.yes && !g.dry_run {
                        return Err(Problem::new("confirmation-required", "Unknown tool", format!("'{tool}' is not in this CLI's manifest, so its side effects are unknown")).hint("check the server's /v1/agent/tools, then re-run with --yes"));
                    }
                    let c = Command {
                        path: vec!["call".into(), tool.clone()],
                        tool: tool.clone(),
                        title: tool.clone(),
                        description: String::new(),
                        schema: json!({ "type": "object" }),
                        params: vec![],
                        effect: Effect::CommitsSpend,
                        idempotent: false,
                        destructive: true,
                        auth: spec::Auth::None,
                        returns: "rdfs:Resource".into(),
                        examples: vec![],
                    };
                    execute(
                        &c,
                        args,
                        &Globals {
                            yes: true,
                            ..clone_globals(g)
                        },
                    )
                }
            }
        }
        Local::Ask => {
            let question = m
                .get_many::<String>("question")
                .map(|v| v.cloned().collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            if question.trim().is_empty() || question.chars().count() > 4000 {
                return Err(Problem::new(
                    "invalid-input",
                    "Invalid question",
                    "the question must be 1–4000 characters",
                ));
            }
            let o = agent::Options {
                question: question.trim().to_string(),
                engine: m.get_one::<String>("engine").cloned(),
                model: m.get_one::<String>("model").cloned(),
                max_steps: *m.get_one::<u8>("max-steps").expect("default") as usize,
                max_tokens: *m.get_one::<u32>("max-tokens").expect("default"),
                engine_timeout: Duration::from_secs(
                    *m.get_one::<u64>("engine-timeout").expect("default"),
                ),
            };
            let client = Client::new(&g.base, g.agent_id.clone(), g.timeout)?;
            if g.dry_run {
                return output::emit(&agent::preview(&client, &o), g.format);
            }
            let engine = agent::Engine::resolve(&o)?;
            let result = agent::run(&engine, &o, &|c, input| client.call(&c.tool, input, c.auth))?;
            if g.format == Format::Text {
                println!(
                    "{}",
                    output::printable(result["answer"].as_str().unwrap_or_default())
                );
                let steps = result["steps"].as_array().map(Vec::len).unwrap_or(0);
                eprintln!(
                    "\n[{} · {} · {steps} lookup{} · {} tokens]",
                    engine.name,
                    engine.model,
                    if steps == 1 { "" } else { "s" },
                    result["usage"]["input_tokens"].as_u64().unwrap_or(0)
                        + result["usage"]["output_tokens"].as_u64().unwrap_or(0)
                );
                return Ok(());
            }
            output::emit(&result, g.format)
        }
        Local::Mcp => mcp::serve(g.base.clone(), g.agent_id.clone(), g.timeout),
        Local::Completions => {
            let shell = *m
                .get_one::<clap_complete::Shell>("shell")
                .expect("required");
            clap_complete::generate(shell, &mut cli(), PROGRAM, &mut std::io::stdout());
            Ok(())
        }
    }
}

fn clone_globals(g: &Globals) -> Globals {
    Globals {
        format: g.format,
        base: g.base.clone(),
        agent_id: g.agent_id.clone(),
        input: g.input.clone(),
        dry_run: g.dry_run,
        yes: g.yes,
        timeout: g.timeout,
    }
}

/// `tfab describe [words...]`.
pub fn describe(words: &[&str]) -> Result<Value, Problem> {
    if words.is_empty() {
        let remote: Vec<Value> = spec::commands().iter().map(|c| json!({ "command": c.line(), "title": c.title, "side_effect": c.effect.id(), "credential": c.auth.id() })).collect();
        let local: Vec<Value> = LOCALS.iter().map(|l| json!({ "command": format!("{PROGRAM} {}", l.name), "title": l.title, "side_effect": if l.network { "read-only" } else { "local" } })).collect();
        return Ok(json!({
            "program": PROGRAM,
            "version": env!("CARGO_PKG_VERSION"),
            "description": env!("CARGO_PKG_DESCRIPTION"),
            "ontology": { "iri": ontology::ONTOLOGY_IRI, "program": ontology::program_iri(), "get": "tfab schema --rdf turtle" },
            "conventions": {
                "stdout": "results only: JSON (default off a terminal), NDJSON or text via --format",
                "stderr": "RFC 9457 problem JSON with exit_code, exit_status, retryable and hint",
                "input": "flags named after the tool's JSON keys (underscores become dashes), or a JSON object via --input FILE|-; flags win",
                "arrays": "comma-separated or repeated flags; values starting with { or [ are JSON",
                "spend": "commands with side_effect commits-spend refuse to run without --yes",
                "secrets": "pass tokens as env:NAME or file:PATH; they are never printed",
                "dry_run": "--dry-run validates and prints the request without sending it",
            },
            "global_options": GLOBAL_OPTIONS.iter().map(|o| json!({ "flag": format!("--{}", o.flag), "env": o.env, "value": o.value, "help": o.help })).collect::<Vec<_>>(),
            "commands": remote.into_iter().chain(local).collect::<Vec<_>>(),
            "exit_statuses": EXIT_STATUSES.iter().map(|e| json!({ "code": e.code, "name": e.name, "retryable": e.retryable })).collect::<Vec<_>>(),
        }));
    }
    if let Some(l) = LOCALS.iter().find(|l| words == [l.name]) {
        return Ok(
            json!({ "command": format!("{PROGRAM} {}", l.name), "title": l.title, "description": l.description, "uses_network": l.network, "examples": l.examples }),
        );
    }
    let c = spec::find(words).ok_or_else(|| {
        Problem::new("not-found", "Unknown command", words.join(" "))
            .hint("run `tfab describe` to list commands")
    })?;
    Ok(json!({
        "command": c.line(),
        "iri": format!("{}/command/{}", ontology::program_iri(), c.id()),
        "title": c.title,
        "description": c.description,
        "tool": c.tool,
        "tool_iri": format!("tfr:tool/{}", c.tool),
        "side_effect": c.effect.id(),
        "idempotent": c.idempotent || c.effect == Effect::ReadOnly,
        "requires_yes": c.effect == Effect::CommitsSpend,
        "credential": c.auth.id(),
        "returns": c.returns,
        "input_schema": c.schema,
        "flags": c.params.iter().map(|p| json!({ "flag": format!("--{}", p.flag), "key": p.key, "required": p.required, "secret": p.secret, "type": p.json_type(), "enum": p.enum_values() })).collect::<Vec<_>>(),
        "exit_statuses": c.exit_statuses().iter().map(|e| json!({ "code": e.code, "name": e.name })).collect::<Vec<_>>(),
        "examples": c.examples,
    }))
}

fn config(g: &Globals) -> Value {
    json!({
        "base_url": g.base,
        "format": g.format.name(),
        "agent_id": g.agent_id,
        "timeout_s": g.timeout.as_secs(),
        "mission_credential": client::credential_source(),
        "mcp_allow_spend": mcp::allow_spend(),
    })
}

fn doctor(g: &Globals) -> Result<(), Problem> {
    let client = Client::new(&g.base, g.agent_id.clone(), g.timeout)?;
    let health = client
        .get("/healthz")
        .map(|_| "ok".to_string())
        .unwrap_or_else(|p| format!("{}: {}", p.title, p.detail));
    let (drift, server_tools) = match client.get("/v1/agent/tools") {
        Ok(v) => {
            let list = v.get("tools").cloned().unwrap_or(v);
            let server: Vec<String> = list
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t["name"].as_str().map(String::from))
                .collect();
            let ours: Vec<String> = spec::manifest()
                .iter()
                .filter_map(|t| t["name"].as_str().map(String::from))
                .collect();
            let new: Vec<&String> = server.iter().filter(|t| !ours.contains(t)).collect();
            let gone: Vec<&String> = ours.iter().filter(|t| !server.contains(t)).collect();
            (
                json!({ "server_only": new, "cli_only": gone, "compatible": gone.is_empty() }),
                server.len(),
            )
        }
        Err(p) => (json!({ "error": format!("{}: {}", p.title, p.detail) }), 0),
    };
    let report = json!({ "config": config(g), "server": { "health": health, "tools": server_tools }, "manifest": drift });
    output::emit(&report, g.format)
}

pub fn guide_text() -> String {
    let mut s = String::new();
    s.push_str("# tfab — TerraFabric command line for agents\n\n");
    s.push_str("TerraFabric is a meta-repository of free and commercial Earth observation data. `tfab` exposes every server tool as a command with typed flags, a JSON input schema and a stable exit status.\n\n");
    s.push_str("## Conventions\n\n");
    s.push_str("- Results go to stdout as JSON (NDJSON with `--format ndjson`, text on a terminal). Errors go to stderr as RFC 9457 problem JSON with `exit_code`, `retryable` and a `hint`.\n");
    s.push_str("- Flags are the tool's JSON keys with `_` → `-`. Arrays: `--bbox 4.0,51.9,4.3,52.0`. Anything complex: `--input args.json` or `--input -`.\n");
    s.push_str("- `tfab describe <command>` gives the full input schema, side effect, credential and examples. `tfab schema` gives the same as RDF linked to the TerraFabric ontology.\n");
    s.push_str("- `--dry-run` validates and shows the request without sending it.\n\n");
    s.push_str("## Safety\n\n");
    s.push_str("- Commands whose side effect is `commits-spend` (placing and approving orders, submitting tasking) refuse to run without `--yes`. Only pass `--yes` when your principal has authorised that specific action.\n");
    s.push_str("- Never supply a principal token you were not explicitly given for that action; `orders approve` is the human principal's decision.\n");
    s.push_str("- Pass secrets as `env:NAME` or `file:PATH`. The CLI never prints them.\n");
    s.push_str("- Mission (C2) commands need `TFAB_TOKEN` or OAuth client credentials (`TFAB_OAUTH_*`).\n\n");
    s.push_str("## Your own inference engine\n\n");
    s.push_str("- `tfab ask \"<question>\"` runs an agent on an engine you operate (IronWorks, Ollama, vLLM, SGLang or any OpenAI-compatible endpoint with tool calling) with TerraFabric's public read-only tools. The conversation stays on your engine; TerraFabric receives only tool calls. It can search, plan and quote, never order or task.\n");
    s.push_str("- To wire your own agent instead: `GET /v1/agent/tools?format=openai` returns the tools ready for a chat-completions request, each invoked with `POST /v1/agent/tools/{name}`; `tfab mcp` and the server's `/mcp` serve the same tools over MCP.\n\n");
    s.push_str("## Commands\n\n");
    for c in spec::commands() {
        s.push_str(&format!(
            "- `{}` — {} [{}{}]\n",
            c.line(),
            c.title,
            c.effect.id(),
            if c.auth == spec::Auth::None {
                String::new()
            } else {
                format!(", {}", c.auth.id())
            }
        ));
    }
    for l in LOCALS {
        s.push_str(&format!("- `{PROGRAM} {}` — {}\n", l.name, l.title));
    }
    s.push_str("\n## Exit statuses\n\n");
    for e in EXIT_STATUSES {
        s.push_str(&format!(
            "- {} `{}` — {}{}\n",
            e.code,
            e.name,
            e.meaning,
            if e.retryable { " (retryable)" } else { "" }
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<std::ffi::OsString> {
        std::iter::once("tfab")
            .chain(s.split_whitespace())
            .map(Into::into)
            .collect()
    }

    #[test]
    fn parser_is_consistent() {
        cli().debug_assert();
    }

    #[test]
    fn every_command_is_reachable_and_describable() {
        for c in spec::commands() {
            let words: Vec<&str> = c.path.iter().map(String::as_str).collect();
            assert!(describe(&words).is_ok(), "{words:?}");
            assert!(
                cli()
                    .try_get_matches_from(argv(&format!("{} --help", words.join(" "))))
                    .is_err(),
                "help should short-circuit"
            );
        }
    }

    #[test]
    fn flags_become_typed_arguments() {
        let m = cli().try_get_matches_from(argv("collections search --q radar --bbox 4,51.9,4.3,52 --limit 5 --night --modality sar --modality optical")).unwrap();
        let (_, g) = m.subcommand().unwrap();
        let (_, lm) = g.subcommand().unwrap();
        let v = collect_args(
            &spec::find(&["collections", "search"]).unwrap(),
            lm,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            v,
            json!({ "q": "radar", "bbox": [4.0, 51.9, 4.3, 52.0], "limit": 5, "night": true, "modality": ["sar", "optical"] })
        );
    }

    #[test]
    fn invalid_input_is_exit_9_before_any_request() {
        let e = run(argv(
            "--base-url https://unreachable.invalid collections search --limit 1001",
        ))
        .unwrap_err();
        assert_eq!(e.code(), 9, "{:?}", e);
        let e = run(argv(
            "--base-url https://unreachable.invalid collections get",
        ))
        .unwrap_err();
        assert!(e.detail.contains("/id: required"));
    }

    #[test]
    fn spend_needs_yes_and_dry_run_sends_nothing() {
        let e = run(argv(
            "--base-url https://unreachable.invalid --agent-id a1 orders place --quote-id q1",
        ))
        .unwrap_err();
        assert_eq!(e.status, "confirmation-required");
        assert_eq!(e.code(), 8);
        // Dry run succeeds with no network (the host does not resolve).
        assert!(run(argv("--base-url https://unreachable.invalid --format json --dry-run orders place --quote-id q1")).is_ok());
    }

    #[test]
    fn agent_id_fills_the_tool_argument() {
        let m = cli()
            .try_get_matches_from(argv(
                "--agent-id bot-7 mandates set --principal ops --max-order-usd 10 --budget-usd 100",
            ))
            .unwrap();
        let (_, g) = m.subcommand().unwrap();
        let (_, lm) = g.subcommand().unwrap();
        let v = collect_args(
            &spec::find(&["mandates", "set"]).unwrap(),
            lm,
            None,
            Some("bot-7"),
        )
        .unwrap();
        assert_eq!(v["agent_id"], "bot-7");
    }

    #[test]
    fn usage_errors_are_exit_2() {
        assert_eq!(run(argv("collections frobnicate")).unwrap_err().code(), 2);
        assert_eq!(run(argv("--format yaml describe")).unwrap_err().code(), 2);
    }

    #[test]
    fn describe_program_lists_everything() {
        let d = describe(&[]).unwrap();
        assert_eq!(
            d["commands"].as_array().unwrap().len(),
            spec::commands().len() + LOCALS.len()
        );
        assert_eq!(
            d["exit_statuses"].as_array().unwrap().len(),
            EXIT_STATUSES.len()
        );
        let one = describe(&["c2", "task"]).unwrap();
        assert_eq!(one["requires_yes"], true);
        assert_eq!(one["credential"], "mission-task");
        assert_eq!(describe(&["nope"]).unwrap_err().code(), 3);
    }

    #[test]
    fn guide_mentions_every_command() {
        let g = guide_text();
        for c in spec::commands() {
            assert!(g.contains(&c.line()), "{}", c.line());
        }
    }
}
