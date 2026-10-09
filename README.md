# tfab: the TerraFabric command line

`tfab` is an agent-first client for [TerraFabric](https://terrafabric.world), the agentic-first geospatial intelligence market: imagery, signals, tracks, weather, infrastructure, industry and economic data from open, commercial and government sources. Each server tool is a command with typed flags. Every command has a JSON input schema, a declared side effect and a stable exit status, and the whole program is described by an ontology linked to the TerraFabric knowledge graph.

```sh
cargo install terrafabric      # installs the `tfab` binary
tfab guide                     # orientation for agents
tfab describe                  # every command, machine-readable
tfab collections search --q radar --bbox 4.0,51.9,4.3,52.0 --limit 5
```

## Why it suits agents

| Need | What tfab does |
|---|---|
| Know what exists | `tfab describe` lists every command with its side effect and credential. `tfab describe <cmd>` gives the input JSON Schema, flags, exit statuses and examples. `tfab tools` gives the same commands in MCP tool shape. |
| Parse results | stdout carries results only: JSON by default when not on a terminal, `--format ndjson` to stream lists, `text` for people. |
| Handle failure | stderr carries RFC 9457 problem JSON with `exit_code`, `exit_status`, `retryable`, `retry_after_s` and a `hint`. `tfab exit-codes` lists the exit statuses. |
| Avoid bad calls | Arguments are checked against the tool's schema before any request is sent (exit 9). `--dry-run` prints the exact request instead of sending it. |
| Stay safe | Commands that commit spend or task a sensor refuse to run without `--yes` (exit 8). Principal commands never send the agent's identity. Secrets are given as `env:NAME` or `file:PATH` and never printed. |
| Reason over it | `tfab schema` emits the CLI ontology (Turtle or JSON-LD). `tfab schema --shapes` emits its SHACL shapes. The same graph is loaded into the server, so `tfr:cli/tfab` resolves there and is queryable by SPARQL. |
| Bring your own model | `tfab ask "<question>"` runs an agent on an inference engine you operate (IronWorks, Ollama, vLLM, SGLang, or any OpenAI-compatible endpoint with tool calling) over TerraFabric's remote data. The conversation stays on your engine; TerraFabric receives only the tool calls. The model gets the public read-only tools, so it can search, plan and quote but never order or task. |
| Use it as tools | `tfab mcp` serves the commands over MCP stdio. Credentials stay in the CLI's environment, not the model's context. Spend tools are offered only with `TFAB_MCP_ALLOW_SPEND=true`. |

## Commands

| Group | Commands |
|---|---|
| `collections` | `search`, `get` |
| `items` | `search` |
| `passes` | `predict` |
| `providers` | `list` |
| `ontology` | `describe`, `term` |
| `graph` | `match`, `sparql` |
| `quotes` | `create` |
| `mandates` | `set` (principal) |
| `orders` | `place` (spend), `get`, `approve` (principal, spend) |
| `c2` | `coverage`, `task` (spend), `status`. These need a mission credential. |
| `osint` | `sources`, `assess`, `watch-create`, `watch-get`, `thermal-analyze`, `thermal-save`, `thermal-get`, `thermal-delete` |
| `tracks` | `get` (live aircraft and vessel positions with pedigree and quality) |
| `plan` | `observe` |
| `artifacts` | `search` |
| local | `describe`, `tools`, `schema`, `guide`, `exit-codes`, `config`, `doctor`, `call`, `ask`, `mcp`, `completions` |

Flags are named after the tool's JSON keys, with `_` written as `-`.

- **Arrays:** take comma-separated values or repeated flags.
- **JSON values:** any value starting with `{` or `[` is parsed as JSON.
- **Whole argument object:** pass it with `--input file.json` or `--input -`. Flags override its fields.

## Your own inference engine

```sh
tfab ask "Which open radar data covers the Port of Rotterdam?"
tfab ask --engine ollama --model ornith-1.5:9b "Monitor maritime traffic in the Strait of Hormuz"
tfab ask --engine http://gpu-box:8000 --model Qwen/Qwen3-30B-A3B "Compare commercial SAR providers"
tfab ask --dry-run "anything"     # list the tools; contact nothing
```

- **Engine:** `--engine` takes `ironworks`, `ollama`, `vllm`, `sglang` or a base URL. Without it, `tfab` uses the first of those that answers on its default local port (8080, 11434, 8000, 30000).
- **Model:** it must support tool calling. Without `--model`, the first local model the engine lists is used. Hosted models (Ollama `:cloud`) are refused, because the conversation would leave your machine.
- **IronWorks:** start it with enough context for the tool definitions, for example `iwx serve model.gguf -b cuda -c 16384`. If you set `--api-key`, export it as `TFAB_ENGINE_KEY`.
- **What goes where:** the question, the conversation and tool results go to your engine. TerraFabric receives only the tool calls. Nothing is billed.
- **Limits:** only public read-only tools are offered. Take the quote ids from the result to `tfab orders place` or the website to buy.

To wire a different agent framework, `GET /api/v1/agent/tools?format=openai` returns the same tools in chat-completions form, each invoked with `POST /api/v1/agent/tools/{name}`.

## Thermal analysis and private packets

`tfab osint thermal-analyze --input thermal-args.json` analyzes an argument object containing `{"model": {...}}`. `thermal-save` accepts the same object with an optional `watch_id`; `thermal-get` and `thermal-delete` accept `--id file:private-run-id.txt`. Delete permanently removes that saved packet and is marked destructive in tool descriptions.

Model objects and private run/watch IDs are redacted from dry-run request output. Use `--model file:model.json` or `--input` to keep models out of shell history. Results intentionally include the requested analysis or private packet; protect output files and capability IDs. Packets are capability-protected, with no public listing or account isolation. This uses analyst-supplied probabilities, not a bundled trained or calibrated classifier.

## Configuration

| Variable | Purpose |
|---|---|
| `TFAB_BASE_URL` | API base. Defaults to `https://terrafabric.world/api`. Plain `http` is allowed only to localhost, for example a local development server or port forward. |
| `TFAB_FORMAT` | `json`, `ndjson` or `text` |
| `TFAB_AGENT_ID` | Agent identity for orders. Spend is bounded by that agent's mandate. |
| `TFAB_TOKEN` | Mission API bearer credential |
| `TFAB_OAUTH_TOKEN_URL`, `TFAB_OAUTH_CLIENT_ID`, `TFAB_OAUTH_CLIENT_SECRET`, `TFAB_OAUTH_SCOPES` | Mission API via OAuth client credentials. The token is cached until a minute before it expires. |
| `TFAB_ENGINE`, `TFAB_MODEL` | Inference engine (name or base URL) and model for `tfab ask` |
| `TFAB_ENGINE_KEY` | Bearer key for that engine, if it needs one |
| `TFAB_MCP_ALLOW_SPEND` | `true` to offer spend tools through `tfab mcp` |
| `TFAB_TIMEOUT` | Request timeout in seconds |

`tfab config` and `tfab doctor` report which credentials are set, never their values.

## Keeping it in step with the server

The CLI is generated from `ontology/tools.json`, a copy of the tool manifest the API serves at [`/v1/agent/tools`](https://terrafabric.world/api/v1/agent/tools). Tests fail if:

- a server tool has no CLI command, or a command points to a tool that no longer exists;
- the committed `ontology/cli.ttl` is out of date. Regenerate it with `tfab schema > ontology/cli.ttl`. The same graph is published in the TerraFabric knowledge graph.

`tfab doctor` compares a running server's tools with the manifest the CLI was built from.

## Security

Report vulnerabilities privately to terrafabric@nervosys.ai rather than opening a public issue.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT licence](LICENSE-MIT), at your option. Contributions are accepted under the same dual licence.

Copyright © 2026 NERVOSYS.
