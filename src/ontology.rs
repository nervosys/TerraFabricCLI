//! The CLI ontology: OWL terms for command-line programs (TBox) and this
//! program's commands, options, environment and exit statuses (ABox), all
//! generated from the registry so it cannot drift from the binary.
//!
//! Terms live in the TerraFabric namespace (`tf:`, module "cli") and link to
//! it: each remote command `tf:invokesTool` the server's `tfr:tool/<name>`
//! and `tf:returnsType` a TerraFabric class. Alignment: schema.org
//! (SoftwareApplication, EntryPoint, PropertyValueSpecification) and SKOS.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::spec::{
    self, Auth, Command, Effect, EXIT_STATUSES, FORMATS, GLOBAL_OPTIONS, GROUPS, LOCALS, PROGRAM,
};

pub const PREFIXES: &[(&str, &str)] = &[
    ("tf", "https://terrafabric.world/ont#"),
    ("tfr", "https://terrafabric.world/id/"),
    ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
    ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
    ("owl", "http://www.w3.org/2002/07/owl#"),
    ("xsd", "http://www.w3.org/2001/XMLSchema#"),
    ("schema", "https://schema.org/"),
    ("skos", "http://www.w3.org/2004/02/skos/core#"),
    ("dct", "http://purl.org/dc/terms/"),
    ("sh", "http://www.w3.org/ns/shacl#"),
];

pub const ONTOLOGY_IRI: &str = "https://terrafabric.world/ont/cli";

#[derive(Debug, Clone, PartialEq)]
pub enum Obj {
    Iri(String),
    Str(String),
    Typed(String, &'static str),
    Bool(bool),
    Int(i64),
}

pub type Graph = BTreeMap<String, Vec<(String, Obj)>>;

struct G(Graph);

impl G {
    fn add(&mut self, s: &str, p: &str, o: Obj) {
        let v = self.0.entry(s.to_string()).or_default();
        if !v.iter().any(|(pp, oo)| pp == p && *oo == o) {
            v.push((p.to_string(), o));
        }
    }
    fn iri(&mut self, s: &str, p: &str, o: &str) {
        self.add(s, p, Obj::Iri(o.to_string()));
    }
    fn str(&mut self, s: &str, p: &str, o: impl Into<String>) {
        self.add(s, p, Obj::Str(o.into()));
    }
}

fn class(g: &mut G, id: &str, label: &str, comment: &str, sup: &[&str]) {
    g.iri(id, "rdf:type", "owl:Class");
    g.str(id, "rdfs:label", label);
    g.str(id, "rdfs:comment", comment);
    g.str(id, "tf:module", "cli");
    g.iri(id, "rdfs:isDefinedBy", ONTOLOGY_IRI);
    for s in sup {
        g.iri(id, "rdfs:subClassOf", s);
    }
}

fn prop(g: &mut G, id: &str, kind: &str, label: &str, comment: &str, domain: &str, range: &str) {
    g.iri(id, "rdf:type", kind);
    g.str(id, "rdfs:label", label);
    g.str(id, "rdfs:comment", comment);
    g.str(id, "tf:module", "cli");
    g.iri(id, "rdfs:isDefinedBy", ONTOLOGY_IRI);
    g.iri(id, "rdfs:domain", domain);
    g.iri(id, "rdfs:range", range);
}

/// Vocabulary (TBox).
fn tbox(g: &mut G) {
    let o = ONTOLOGY_IRI;
    g.iri(o, "rdf:type", "owl:Ontology");
    g.iri(o, "owl:imports", "https://terrafabric.world/ont");
    g.str(o, "dct:title", "TerraFabric CLI Ontology");
    g.str(o, "dct:description", "Describes command-line programs for agents: commands, their inputs, side effects, required credentials, results and exit statuses, linked to the TerraFabric ontology's tools and classes.");
    g.str(o, "owl:versionInfo", env!("CARGO_PKG_VERSION"));

    class(
        g,
        "tf:CommandLineProgram",
        "Command-line program",
        "An executable with subcommands that agents and people invoke from a shell.",
        &["schema:SoftwareApplication"],
    );
    class(
        g,
        "tf:CliCommandGroup",
        "Command group",
        "A named set of related subcommands (the first word after the program name).",
        &["skos:Collection"],
    );
    class(g, "tf:CliCommand", "CLI command", "One invocable command line. Its inputs are tf:CliOption values; running it has one tf:SideEffect.", &["schema:EntryPoint"]);
    class(
        g,
        "tf:RemoteCliCommand",
        "Remote CLI command",
        "A command that invokes a TerraFabric server tool (tf:Tool) over HTTPS.",
        &["tf:CliCommand"],
    );
    class(g, "tf:LocalCliCommand", "Local CLI command", "A command implemented by the program itself (description, ontology, configuration, MCP serving).", &["tf:CliCommand"]);
    class(g, "tf:CliOption", "CLI option", "A named input of a command, given as --flag VALUE. Maps to one key of the command's JSON input.", &["schema:PropertyValueSpecification"]);
    class(
        g,
        "tf:GlobalOption",
        "Global option",
        "An option every command accepts.",
        &["tf:CliOption"],
    );
    class(
        g,
        "tf:EnvironmentVariable",
        "Environment variable",
        "A process environment variable the program reads.",
        &[],
    );
    class(
        g,
        "tf:ExitStatus",
        "Exit status",
        "A process exit code with a stable meaning agents can branch on.",
        &[],
    );
    class(
        g,
        "tf:OutputFormat",
        "Output format",
        "A serialisation of command results on stdout.",
        &["skos:Concept"],
    );
    class(
        g,
        "tf:SideEffect",
        "Side effect",
        "What running a command changes, from nothing to committed spend.",
        &["skos:Concept"],
    );
    class(
        g,
        "tf:CredentialRequirement",
        "Credential requirement",
        "The credential a command needs.",
        &["skos:Concept"],
    );

    let op = "owl:ObjectProperty";
    let dp = "owl:DatatypeProperty";
    prop(
        g,
        "tf:hasCommand",
        op,
        "has command",
        "A command the program offers.",
        "tf:CommandLineProgram",
        "tf:CliCommand",
    );
    prop(
        g,
        "tf:hasCommandGroup",
        op,
        "has command group",
        "A command group of the program.",
        "tf:CommandLineProgram",
        "tf:CliCommandGroup",
    );
    prop(
        g,
        "tf:inCommandGroup",
        op,
        "in command group",
        "The group a command belongs to.",
        "tf:CliCommand",
        "tf:CliCommandGroup",
    );
    prop(
        g,
        "tf:invokesTool",
        op,
        "invokes tool",
        "The server tool a remote command calls.",
        "tf:RemoteCliCommand",
        "tf:Tool",
    );
    prop(
        g,
        "tf:hasOption",
        op,
        "has option",
        "An input option of a command.",
        "tf:CliCommand",
        "tf:CliOption",
    );
    prop(
        g,
        "tf:hasGlobalOption",
        op,
        "has global option",
        "An option every command of the program accepts.",
        "tf:CommandLineProgram",
        "tf:GlobalOption",
    );
    prop(
        g,
        "tf:hasSideEffect",
        op,
        "has side effect",
        "What running the command changes.",
        "tf:CliCommand",
        "tf:SideEffect",
    );
    prop(
        g,
        "tf:requiresCredential",
        op,
        "requires credential",
        "The credential the command needs.",
        "tf:CliCommand",
        "tf:CredentialRequirement",
    );
    prop(
        g,
        "tf:returnsType",
        op,
        "returns type",
        "The TerraFabric class of the command's result.",
        "tf:CliCommand",
        "rdfs:Class",
    );
    prop(
        g,
        "tf:mayExitWith",
        op,
        "may exit with",
        "An exit status the command can end with.",
        "tf:CliCommand",
        "tf:ExitStatus",
    );
    prop(
        g,
        "tf:supportsFormat",
        op,
        "supports format",
        "An output format the program can write.",
        "tf:CommandLineProgram",
        "tf:OutputFormat",
    );
    prop(
        g,
        "tf:readsEnvironment",
        op,
        "reads environment",
        "An environment variable the program or option reads.",
        "rdfs:Resource",
        "tf:EnvironmentVariable",
    );
    prop(
        g,
        "tf:valueType",
        op,
        "value type",
        "Datatype of an option's value (rdf:JSON for structured values).",
        "tf:CliOption",
        "rdfs:Datatype",
    );
    prop(
        g,
        "tf:commandLine",
        dp,
        "command line",
        "The words that invoke the command, e.g. 'tfab collections search'.",
        "tf:CliCommand",
        "xsd:string",
    );
    prop(
        g,
        "tf:optionFlag",
        dp,
        "option flag",
        "The long flag, without dashes.",
        "tf:CliOption",
        "xsd:string",
    );
    prop(
        g,
        "tf:argumentKey",
        dp,
        "argument key",
        "The key the option sets in the JSON input sent to the tool.",
        "tf:CliOption",
        "xsd:string",
    );
    prop(
        g,
        "tf:allowedValue",
        dp,
        "allowed value",
        "One permitted value of an enumerated option.",
        "tf:CliOption",
        "xsd:string",
    );
    prop(
        g,
        "tf:secret",
        dp,
        "secret",
        "True when the value is a credential: never echoed; pass as env:NAME or file:PATH.",
        "tf:CliOption",
        "xsd:boolean",
    );
    prop(
        g,
        "tf:idempotent",
        dp,
        "idempotent",
        "True when repeating the command with the same input has no further effect.",
        "tf:CliCommand",
        "xsd:boolean",
    );
    prop(
        g,
        "tf:usesNetwork",
        dp,
        "uses network",
        "True when the command contacts the server.",
        "tf:CliCommand",
        "xsd:boolean",
    );
    prop(
        g,
        "tf:inputSchema",
        dp,
        "input schema",
        "JSON Schema of the command's input object.",
        "tf:CliCommand",
        "rdf:JSON",
    );
    prop(
        g,
        "tf:example",
        dp,
        "example",
        "A complete, runnable example invocation.",
        "tf:CliCommand",
        "xsd:string",
    );
    prop(
        g,
        "tf:exitCode",
        dp,
        "exit code",
        "The numeric process exit code.",
        "tf:ExitStatus",
        "xsd:integer",
    );
    prop(
        g,
        "tf:retryable",
        dp,
        "retryable",
        "True when retrying later can succeed.",
        "tf:ExitStatus",
        "xsd:boolean",
    );
    prop(
        g,
        "tf:envName",
        dp,
        "environment variable name",
        "The variable's name.",
        "tf:EnvironmentVariable",
        "xsd:string",
    );

    scheme(
        g,
        "tf:SideEffectScheme",
        "Side effects",
        "tf:SideEffect",
        Effect::ALL
            .iter()
            .map(|e| (format!("tf:effect-{}", e.id()), e.label(), e.comment()))
            .collect(),
    );
    scheme(
        g,
        "tf:CredentialScheme",
        "Credential requirements",
        "tf:CredentialRequirement",
        Auth::ALL
            .iter()
            .map(|a| (format!("tf:credential-{}", a.id()), a.label(), a.comment()))
            .collect(),
    );
    scheme(
        g,
        "tf:OutputFormatScheme",
        "Output formats",
        "tf:OutputFormat",
        FORMATS
            .iter()
            .map(|(id, c)| (format!("tf:output-{id}"), *id, *c))
            .collect(),
    );
}

fn scheme(g: &mut G, id: &str, label: &str, class: &str, members: Vec<(String, &str, &str)>) {
    g.iri(id, "rdf:type", "skos:ConceptScheme");
    g.str(id, "skos:prefLabel", label);
    for (m, l, c) in members {
        g.iri(&m, "rdf:type", class);
        g.iri(&m, "skos:inScheme", id);
        g.str(&m, "skos:prefLabel", l);
        g.str(&m, "skos:definition", c);
    }
}

pub fn program_iri() -> String {
    format!("tfr:cli/{PROGRAM}")
}
fn command_iri(id: &str) -> String {
    format!("tfr:cli/{PROGRAM}/command/{id}")
}
fn exit_iri(name: &str) -> String {
    format!("tfr:cli/{PROGRAM}/exit/{name}")
}
fn env_iri(name: &str) -> String {
    format!("tfr:cli/{PROGRAM}/env/{name}")
}

/// This program (ABox).
fn abox(g: &mut G, cmds: &[Command]) {
    let p = program_iri();
    g.iri(&p, "rdf:type", "tf:CommandLineProgram");
    g.str(&p, "schema:name", PROGRAM);
    g.str(&p, "schema:description", env!("CARGO_PKG_DESCRIPTION"));
    g.str(&p, "schema:softwareVersion", env!("CARGO_PKG_VERSION"));
    g.str(&p, "schema:applicationCategory", "DeveloperApplication");
    g.iri(&p, "schema:isPartOf", "https://terrafabric.world/");
    for (id, _) in FORMATS {
        g.iri(&p, "tf:supportsFormat", &format!("tf:output-{id}"));
    }

    for (name, meaning) in GROUPS {
        let gi = format!("tfr:cli/{PROGRAM}/group/{name}");
        g.iri(&gi, "rdf:type", "tf:CliCommandGroup");
        g.str(&gi, "skos:prefLabel", *name);
        g.str(&gi, "skos:definition", *meaning);
        g.iri(&p, "tf:hasCommandGroup", &gi);
    }

    for e in EXIT_STATUSES {
        let s = exit_iri(e.name);
        g.iri(&s, "rdf:type", "tf:ExitStatus");
        g.add(&s, "tf:exitCode", Obj::Int(e.code as i64));
        g.str(&s, "rdfs:label", e.name);
        g.str(&s, "rdfs:comment", e.meaning);
        g.add(&s, "tf:retryable", Obj::Bool(e.retryable));
    }

    let env = |g: &mut G, name: &str, comment: &str, secret: bool| {
        let s = env_iri(name);
        g.iri(&s, "rdf:type", "tf:EnvironmentVariable");
        g.str(&s, "tf:envName", name);
        g.str(&s, "rdfs:comment", comment);
        g.add(&s, "tf:secret", Obj::Bool(secret));
        g.iri(&program_iri(), "tf:readsEnvironment", &s);
        s
    };
    for (name, comment, secret) in spec::ENVIRONMENT {
        env(g, name, comment, *secret);
    }
    for o in GLOBAL_OPTIONS {
        let s = format!("tfr:cli/{PROGRAM}/option/{}", o.flag);
        g.iri(&s, "rdf:type", "tf:GlobalOption");
        g.str(&s, "tf:optionFlag", o.flag);
        g.str(&s, "schema:valueName", o.value.unwrap_or(""));
        g.str(&s, "schema:description", o.help);
        g.add(&s, "schema:valueRequired", Obj::Bool(false));
        g.iri(
            &s,
            "tf:valueType",
            if o.value.is_some() {
                "xsd:string"
            } else {
                "xsd:boolean"
            },
        );
        if let Some(var) = o.env {
            let e = env(g, var, &format!("Default for --{}.", o.flag), false);
            g.iri(&s, "tf:readsEnvironment", &e);
        }
        g.iri(&p, "tf:hasGlobalOption", &s);
    }

    for c in cmds {
        let s = command_iri(&c.id());
        g.iri(&s, "rdf:type", "tf:RemoteCliCommand");
        g.str(&s, "schema:name", c.title.clone());
        g.str(&s, "schema:description", c.description.clone());
        g.str(&s, "tf:commandLine", c.line());
        g.iri(
            &s,
            "tf:inCommandGroup",
            &format!("tfr:cli/{PROGRAM}/group/{}", c.path[0]),
        );
        g.iri(&s, "tf:invokesTool", &format!("tfr:tool/{}", c.tool));
        g.iri(
            &s,
            "tf:hasSideEffect",
            &format!("tf:effect-{}", c.effect.id()),
        );
        g.iri(
            &s,
            "tf:requiresCredential",
            &format!("tf:credential-{}", c.auth.id()),
        );
        g.iri(&s, "tf:returnsType", &c.returns);
        g.add(
            &s,
            "tf:idempotent",
            Obj::Bool(c.idempotent || c.effect == Effect::ReadOnly),
        );
        g.add(&s, "tf:usesNetwork", Obj::Bool(true));
        g.str(&s, "schema:encodingType", "application/json");
        g.add(
            &s,
            "tf:inputSchema",
            Obj::Typed(c.schema.to_string(), "rdf:JSON"),
        );
        for ex in &c.examples {
            g.str(&s, "tf:example", ex.clone());
        }
        for e in c.exit_statuses() {
            g.iri(&s, "tf:mayExitWith", &exit_iri(e.name));
        }
        for prm in &c.params {
            let o = format!("{s}/option/{}", prm.flag);
            g.iri(&o, "rdf:type", "tf:CliOption");
            g.str(&o, "tf:optionFlag", prm.flag.clone());
            g.str(&o, "tf:argumentKey", prm.key.clone());
            g.str(&o, "schema:valueName", prm.key.to_uppercase());
            g.add(&o, "schema:valueRequired", Obj::Bool(prm.required));
            g.add(&o, "schema:multipleValues", Obj::Bool(prm.is_array()));
            g.iri(&o, "tf:valueType", prm.xsd());
            g.add(&o, "tf:secret", Obj::Bool(prm.secret));
            if !prm.description.is_empty() {
                g.str(&o, "schema:description", prm.description.clone());
            }
            for v in prm.enum_values() {
                g.str(&o, "tf:allowedValue", v);
            }
            if let Some(n) = prm.schema.get("minimum").and_then(Value::as_f64) {
                g.add(&o, "schema:minValue", Obj::Typed(fmt_num(n), "xsd:decimal"));
            }
            if let Some(n) = prm.schema.get("maximum").and_then(Value::as_f64) {
                g.add(&o, "schema:maxValue", Obj::Typed(fmt_num(n), "xsd:decimal"));
            }
            g.iri(&s, "tf:hasOption", &o);
        }
        g.iri(&p, "tf:hasCommand", &s);
    }

    for l in LOCALS {
        let s = command_iri(l.name);
        g.iri(&s, "rdf:type", "tf:LocalCliCommand");
        g.str(&s, "schema:name", l.title);
        g.str(&s, "schema:description", l.description);
        g.str(&s, "tf:commandLine", format!("{PROGRAM} {}", l.name));
        g.iri(
            &s,
            "tf:hasSideEffect",
            if l.network {
                "tf:effect-read-only"
            } else {
                "tf:effect-local"
            },
        );
        g.iri(&s, "tf:requiresCredential", "tf:credential-none");
        g.add(&s, "tf:idempotent", Obj::Bool(l.local != spec::Local::Call));
        g.add(&s, "tf:usesNetwork", Obj::Bool(l.network));
        for ex in l.examples {
            g.str(&s, "tf:example", *ex);
        }
        for e in ["ok", "internal", "usage"] {
            g.iri(&s, "tf:mayExitWith", &exit_iri(e));
        }
        g.iri(&p, "tf:hasCommand", &s);
    }
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{n:.1}")
    } else {
        n.to_string()
    }
}

/// SHACL shapes for the ABox: what every command, option and exit status must state.
fn shapes(g: &mut G) {
    let shape = |g: &mut G, id: &str, target: &str, props: &[(&str, &str, u8, Option<&str>)]| {
        g.iri(id, "rdf:type", "sh:NodeShape");
        g.iri(id, "sh:targetClass", target);
        for (i, (path, kind, min, node_class)) in props.iter().enumerate() {
            let b = format!("{id}-p{i}");
            g.iri(id, "sh:property", &b);
            g.iri(&b, "sh:path", path);
            g.add(&b, "sh:minCount", Obj::Int(*min as i64));
            if *min == 1
                && !path.ends_with("Option")
                && *path != "tf:mayExitWith"
                && *path != "tf:example"
            {
                g.add(&b, "sh:maxCount", Obj::Int(1));
            }
            match *kind {
                "iri" => g.iri(&b, "sh:nodeKind", "sh:IRI"),
                dt => g.iri(&b, "sh:datatype", dt),
            }
            if let Some(c) = node_class {
                g.iri(&b, "sh:class", c);
            }
        }
    };
    shape(
        g,
        "tf:CliCommandShape",
        "tf:CliCommand",
        &[
            ("tf:commandLine", "xsd:string", 1, None),
            ("schema:name", "xsd:string", 1, None),
            ("schema:description", "xsd:string", 1, None),
            ("tf:hasSideEffect", "iri", 1, Some("tf:SideEffect")),
            (
                "tf:requiresCredential",
                "iri",
                1,
                Some("tf:CredentialRequirement"),
            ),
            ("tf:mayExitWith", "iri", 1, Some("tf:ExitStatus")),
            ("tf:example", "xsd:string", 1, None),
        ],
    );
    shape(
        g,
        "tf:RemoteCliCommandShape",
        "tf:RemoteCliCommand",
        &[
            ("tf:invokesTool", "iri", 1, None),
            ("tf:returnsType", "iri", 1, None),
            ("tf:inputSchema", "rdf:JSON", 1, None),
        ],
    );
    shape(
        g,
        "tf:CliOptionShape",
        "tf:CliOption",
        &[
            ("tf:optionFlag", "xsd:string", 1, None),
            ("tf:valueType", "iri", 1, None),
            ("schema:valueRequired", "xsd:boolean", 1, None),
        ],
    );
    shape(
        g,
        "tf:ExitStatusShape",
        "tf:ExitStatus",
        &[
            ("tf:exitCode", "xsd:integer", 1, None),
            ("tf:retryable", "xsd:boolean", 1, None),
        ],
    );
}

/// The full ontology graph: vocabulary plus this program.
pub fn graph() -> Graph {
    let mut g = G(BTreeMap::new());
    tbox(&mut g);
    abox(&mut g, &spec::commands());
    g.0
}

pub fn shapes_graph() -> Graph {
    let mut g = G(BTreeMap::new());
    shapes(&mut g);
    g.0
}

fn expand(curie: &str) -> String {
    for (p, ns) in PREFIXES {
        if let Some(rest) = curie.strip_prefix(&format!("{p}:")) {
            return format!("{ns}{rest}");
        }
    }
    curie.to_string()
}

/// A CURIE is only safe in Turtle when its local part is simple.
fn term(t: &str) -> String {
    if t.starts_with("http://") || t.starts_with("https://") {
        return format!("<{t}>");
    }
    match t.split_once(':') {
        Some((_, local))
            if !local.is_empty()
                && local
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && !local.ends_with('-') =>
        {
            t.to_string()
        }
        _ => format!("<{}>", expand(t)),
    }
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o
}

fn obj_ttl(o: &Obj) -> String {
    match o {
        Obj::Iri(i) => term(i),
        Obj::Str(s) => format!("\"{}\"", escape(s)),
        Obj::Typed(s, dt) => format!("\"{}\"^^{}", escape(s), term(dt)),
        Obj::Bool(b) => b.to_string(),
        Obj::Int(i) => i.to_string(),
    }
}

pub fn to_turtle(g: &Graph) -> String {
    let mut out = String::new();
    for (p, ns) in PREFIXES {
        out.push_str(&format!("@prefix {p}: <{ns}> .\n"));
    }
    out.push('\n');
    // Ontology header first, then subjects in order.
    let mut subjects: Vec<&String> = g.keys().collect();
    subjects.sort_by_key(|s| (s.as_str() != ONTOLOGY_IRI, s.to_string()));
    for s in subjects {
        let mut props: Vec<&(String, Obj)> = g[s].iter().collect();
        props.sort_by_key(|(p, _)| (p != "rdf:type", p.clone()));
        out.push_str(&term(s));
        for (i, (p, o)) in props.iter().enumerate() {
            let pred = if p == "rdf:type" {
                "a".to_string()
            } else {
                term(p)
            };
            out.push_str(if i == 0 { " " } else { " ;\n    " });
            out.push_str(&format!("{pred} {}", obj_ttl(o)));
        }
        out.push_str(" .\n\n");
    }
    out
}

pub fn context() -> Value {
    let mut c = Map::new();
    for (p, ns) in PREFIXES {
        c.insert((*p).into(), json!(ns));
    }
    Value::Object(c)
}

pub fn to_jsonld(g: &Graph) -> Value {
    let nodes: Vec<Value> = g
        .iter()
        .map(|(s, props)| {
            let mut n = Map::new();
            n.insert("@id".into(), json!(s));
            let mut by: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
            for (p, o) in props {
                if p == "rdf:type" {
                    if let Obj::Iri(t) = o {
                        by.entry("@type").or_default().push(json!(t));
                    }
                    continue;
                }
                by.entry(p.as_str()).or_default().push(match o {
                    Obj::Iri(i) => json!({ "@id": i }),
                    Obj::Str(s) => json!(s),
                    Obj::Typed(v, "rdf:JSON") => json!({ "@value": serde_json::from_str::<Value>(v).unwrap_or(json!(v)), "@type": "@json" }),
                    Obj::Typed(v, dt) => json!({ "@value": v, "@type": dt }),
                    Obj::Bool(b) => json!(b),
                    Obj::Int(i) => json!(i),
                });
            }
            for (k, mut v) in by {
                n.insert(k.into(), if v.len() == 1 { v.remove(0) } else { Value::Array(v) });
            }
            Value::Object(n)
        })
        .collect();
    json!({ "@context": context(), "@graph": nodes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(ttl: &str) -> Vec<(String, String, String)> {
        oxttl::TurtleParser::new()
            .for_slice(ttl.as_bytes())
            .map(|t| {
                let t = t.expect("valid Turtle");
                (
                    t.subject.to_string(),
                    t.predicate.to_string(),
                    t.object.to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn turtle_parses_and_is_complete() {
        let ttl = to_turtle(&graph());
        let triples = parse(&ttl);
        assert!(triples.len() > 500, "only {} triples", triples.len());
        let tf = |l: &str| format!("<https://terrafabric.world/ont#{l}>");
        let rdf_type = "<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>";
        let has = |s: &str, p: &str| triples.iter().any(|t| t.0 == s && t.1 == p);
        let commands: Vec<&String> = triples
            .iter()
            .filter(|t| {
                t.1 == rdf_type && (t.2 == tf("RemoteCliCommand") || t.2 == tf("LocalCliCommand"))
            })
            .map(|t| &t.0)
            .collect();
        assert_eq!(commands.len(), spec::commands().len() + LOCALS.len());
        for c in &commands {
            for p in [
                "commandLine",
                "hasSideEffect",
                "requiresCredential",
                "mayExitWith",
            ] {
                assert!(has(c, &tf(p)), "{c} lacks tf:{p}");
            }
        }
        // Remote commands link to the server's tool individuals.
        assert!(triples.iter().any(|t| t.1 == tf("invokesTool")
            && t.2 == "<https://terrafabric.world/id/tool/search_collections>"));
        // Every exit status is typed and numbered.
        assert_eq!(
            triples.iter().filter(|t| t.1 == tf("exitCode")).count(),
            EXIT_STATUSES.len()
        );
    }

    #[test]
    fn committed_ontology_is_current() {
        let committed = include_str!("../ontology/cli.ttl").replace("\r\n", "\n");
        assert!(
            committed == to_turtle(&graph()),
            "ontology/cli.ttl is stale: run `tfab schema > ontology/cli.ttl` and `tfab schema --shapes > ontology/cli-shapes.ttl`"
        );
    }

    #[test]
    fn shapes_parse() {
        let triples = parse(&to_turtle(&shapes_graph()));
        assert!(
            triples
                .iter()
                .filter(|t| t.2 == "<http://www.w3.org/ns/shacl#NodeShape>")
                .count()
                == 4
        );
    }

    #[test]
    fn jsonld_has_context_and_every_command() {
        let v = to_jsonld(&graph());
        assert_eq!(v["@context"]["tf"], "https://terrafabric.world/ont#");
        let nodes = v["@graph"].as_array().unwrap();
        let remote = nodes
            .iter()
            .filter(|n| n["@type"] == "tf:RemoteCliCommand")
            .count();
        assert_eq!(remote, spec::commands().len());
        let search = nodes
            .iter()
            .find(|n| n["@id"] == "tfr:cli/tfab/command/collections-search")
            .unwrap();
        assert_eq!(search["tf:inputSchema"]["@type"], "@json");
        assert_eq!(
            search["tf:invokesTool"]["@id"],
            "tfr:tool/search_collections"
        );
    }

    #[test]
    fn awkward_literals_are_escaped() {
        let mut g = G(BTreeMap::new());
        g.str(
            "tfr:x",
            "rdfs:comment",
            "quote \" backslash \\ newline \n tab \t",
        );
        g.add(
            "tfr:x",
            "tf:inputSchema",
            Obj::Typed("{\"a\":\"b\"}".into(), "rdf:JSON"),
        );
        assert_eq!(parse(&to_turtle(&g.0)).len(), 2);
    }
}
