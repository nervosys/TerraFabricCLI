//! HTTP to the TerraFabric API: tool invocation, credentials, and mapping
//! failures to stable exit statuses.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::spec::{exit_status, Auth};

/// A failure, printed as RFC 9457 problem JSON on stderr, with an exit status.
#[derive(Debug, Clone)]
pub struct Problem {
    pub status: &'static str,
    pub title: String,
    pub detail: String,
    pub http_status: Option<u16>,
    pub retry_after_s: Option<u64>,
    pub hint: Option<String>,
}

impl Problem {
    pub fn new(status: &'static str, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status,
            title: title.into(),
            detail: detail.into(),
            http_status: None,
            retry_after_s: None,
            hint: None,
        }
    }
    pub fn hint(mut self, h: impl Into<String>) -> Self {
        self.hint = Some(h.into());
        self
    }
    pub fn code(&self) -> i32 {
        exit_status(self.status).code
    }
    pub fn to_json(&self) -> Value {
        let e = exit_status(self.status);
        let mut v = json!({
            // Dereferenceable: the exit status's IRI in the CLI ontology.
            "type": format!("https://terrafabric.world/id/cli/{}/exit/{}", crate::spec::PROGRAM, e.name),
            "title": self.title,
            "detail": self.detail,
            "exit_code": e.code,
            "exit_status": e.name,
            "retryable": e.retryable,
        });
        if let Some(s) = self.http_status {
            v["status"] = json!(s);
        }
        if let Some(r) = self.retry_after_s {
            v["retry_after_s"] = json!(r);
        }
        if let Some(h) = &self.hint {
            v["hint"] = json!(h);
        }
        v
    }
}

pub struct Client {
    base: String,
    http: reqwest::blocking::Client,
    agent_id: Option<String>,
    token: Mutex<Option<(String, Instant)>>,
}

/// Where the mission credential comes from; never the value itself.
pub fn credential_source() -> &'static str {
    if nonempty("TFAB_TOKEN") {
        "TFAB_TOKEN"
    } else if [
        "TFAB_OAUTH_TOKEN_URL",
        "TFAB_OAUTH_CLIENT_ID",
        "TFAB_OAUTH_CLIENT_SECRET",
    ]
    .iter()
    .all(|k| nonempty(k))
    {
        "oauth-client-credentials"
    } else {
        "none"
    }
}

fn nonempty(k: &str) -> bool {
    std::env::var(k).is_ok_and(|v| !v.trim().is_empty())
}

impl Client {
    pub fn new(base: &str, agent_id: Option<String>, timeout: Duration) -> Result<Self, Problem> {
        let base = base.trim_end_matches('/').to_string();
        let url = reqwest::Url::parse(&base)
            .map_err(|e| Problem::new("usage", "Invalid base URL", format!("{base}: {e}")))?;
        // Plain http only to this machine (a local server or port forward).
        let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
        if !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
            return Err(Problem::new(
                "usage",
                "Insecure base URL",
                format!("{base}: only https, or http to localhost, is allowed"),
            ));
        }
        let http = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .user_agent(concat!("tfab/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| Problem::new("internal", "HTTP client", e.to_string()))?;
        Ok(Self {
            base,
            http,
            agent_id,
            token: Mutex::new(None),
        })
    }

    pub fn tool_url(&self, tool: &str) -> String {
        format!("{}/v1/agent/tools/{tool}", self.base)
    }

    /// Invoke a server tool.
    pub fn call(&self, tool: &str, args: &Value, auth: Auth) -> Result<Value, Problem> {
        let mut req = self.http.post(self.tool_url(tool)).json(args);
        // A principal acts for themselves: sending the agent's identity would
        // make a mandate look self-issued (which the server refuses).
        if let (Some(a), false) = (&self.agent_id, auth == Auth::Principal) {
            req = req.header("x-agent-id", a);
        }
        if matches!(auth, Auth::MissionRead | Auth::MissionTask) {
            match self.bearer()? {
                Some(t) => req = req.bearer_auth(t),
                None => {
                    return Err(Problem::new("auth", "Mission credential required", format!("'{tool}' is part of the mission API"))
                        .hint("set TFAB_TOKEN, or TFAB_OAUTH_TOKEN_URL + TFAB_OAUTH_CLIENT_ID + TFAB_OAUTH_CLIENT_SECRET"));
                }
            }
        }
        let res = req.send().map_err(network)?;
        read(res)
    }

    /// GET a JSON document relative to the base URL.
    pub fn get(&self, path: &str) -> Result<Value, Problem> {
        read(
            self.http
                .get(format!("{}{path}", self.base))
                .send()
                .map_err(network)?,
        )
    }

    /// The mission bearer token: TFAB_TOKEN, or an OAuth client-credentials
    /// access token (cached until a minute before it expires).
    fn bearer(&self) -> Result<Option<String>, Problem> {
        if let Ok(t) = std::env::var("TFAB_TOKEN") {
            if !t.trim().is_empty() {
                return Ok(Some(t.trim().to_string()));
            }
        }
        if credential_source() != "oauth-client-credentials" {
            return Ok(None);
        }
        let mut cache = self.token.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((t, exp)) = cache.as_ref() {
            if Instant::now() < *exp {
                return Ok(Some(t.clone()));
            }
        }
        let url = std::env::var("TFAB_OAUTH_TOKEN_URL").unwrap_or_default();
        if !url.starts_with("https://") {
            return Err(Problem::new(
                "usage",
                "Insecure token URL",
                "TFAB_OAUTH_TOKEN_URL must use https",
            ));
        }
        let id = std::env::var("TFAB_OAUTH_CLIENT_ID").unwrap_or_default();
        let secret = std::env::var("TFAB_OAUTH_CLIENT_SECRET").unwrap_or_default();
        let mut form = vec![("grant_type", "client_credentials".to_string())];
        if let Ok(s) = std::env::var("TFAB_OAUTH_SCOPES") {
            form.push(("scope", s));
        }
        let res = self
            .http
            .post(&url)
            .basic_auth(&id, Some(&secret))
            .form(&form)
            .send()
            .map_err(network)?;
        if !res.status().is_success() {
            return Err(Problem::new(
                "auth",
                "OAuth token request refused",
                format!("the token endpoint answered {}", res.status()),
            ));
        }
        let body: Value = res
            .json()
            .map_err(|_| Problem::new("auth", "OAuth token response", "not JSON"))?;
        let token = body["access_token"]
            .as_str()
            .ok_or_else(|| Problem::new("auth", "OAuth token response", "no access_token"))?
            .to_string();
        let ttl = body["expires_in"]
            .as_u64()
            .unwrap_or(300)
            .saturating_sub(60)
            .max(1);
        *cache = Some((token.clone(), Instant::now() + Duration::from_secs(ttl)));
        Ok(Some(token))
    }
}

fn network(e: reqwest::Error) -> Problem {
    let what = if e.is_timeout() {
        "Request timed out"
    } else if e.is_connect() {
        "Cannot connect"
    } else {
        "Network error"
    };
    // reqwest errors carry the URL; they never carry credentials.
    Problem::new("unavailable", what, e.to_string())
}

fn read(res: reqwest::blocking::Response) -> Result<Value, Problem> {
    let status = res.status().as_u16();
    let retry = res
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let text = res.text().unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    if (200..300).contains(&status) {
        return Ok(body);
    }
    let detail = body
        .get("detail")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| match &body {
            Value::String(s) => s.chars().take(500).collect(),
            v => v.to_string(),
        });
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Request failed")
        .to_string();
    let name = classify(status, &detail);
    let mut p = Problem::new(name, title, detail);
    p.http_status = Some(status);
    p.retry_after_s = retry;
    Err(p)
}

/// HTTP status → exit status. Tool errors arrive as 400 with the reason in
/// `detail`, so "not found" is recognised there too.
pub fn classify(status: u16, detail: &str) -> &'static str {
    match status {
        401 | 403 => "auth",
        404 => "not-found",
        409 | 412 => "conflict",
        429 => "rate-limited",
        400 | 422 if detail.contains("not found") => "not-found",
        400 | 422 => "invalid-input",
        s if s >= 500 => "unavailable",
        _ => "internal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_exit_codes() {
        assert_eq!(classify(401, ""), "auth");
        assert_eq!(classify(403, ""), "auth");
        assert_eq!(
            classify(400, "Bad request: collection 'x' not found"),
            "not-found"
        );
        assert_eq!(
            classify(400, "Bad request: missing field aoi"),
            "invalid-input"
        );
        assert_eq!(classify(429, ""), "rate-limited");
        assert_eq!(classify(502, ""), "unavailable");
        assert_eq!(Problem::new("auth", "t", "d").code(), 4);
    }

    #[test]
    fn refuses_plain_http_to_remote_hosts() {
        assert!(Client::new("http://terrafabric.world/api", None, Duration::from_secs(5)).is_err());
        assert!(
            Client::new(
                "https://terrafabric.world/api/",
                None,
                Duration::from_secs(5)
            )
            .unwrap()
            .tool_url("x")
                == "https://terrafabric.world/api/v1/agent/tools/x"
        );
        assert!(Client::new("http://127.0.0.1:8787", None, Duration::from_secs(5)).is_ok());
        assert!(Client::new(
            "http://localhost.evil.example/api",
            None,
            Duration::from_secs(5)
        )
        .is_err());
    }

    #[test]
    fn problem_json_is_self_describing() {
        let mut p = Problem::new("rate-limited", "Too many requests", "slow down");
        p.retry_after_s = Some(7);
        let v = p.to_json();
        assert_eq!(v["exit_code"], 6);
        assert_eq!(v["retryable"], true);
        assert_eq!(v["retry_after_s"], 7);
    }
}
