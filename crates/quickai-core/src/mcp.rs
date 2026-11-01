//! MCP servers: the tools a model may use, in programs of their own that speak the Model Context Protocol.
//!
//! The person's list is the text of a field, in the form the programs that use MCP give it and their read-mes
//! print, so that one can be pasted as it is: `{"mcpServers": {"<name>": {...}}}` (Claude Desktop's and Cursor's).
//! A server is a command, which the engine starts and talks to over its input and output
//! (`{"command", "args", "env"}`), or an address (`{"url", "headers"}`). `"disabled": true` keeps one in the list
//! without using it. VS Code's form (`{"servers": {...}}`), a bare table of servers and a single server are read
//! too, and written back in the first form.
//!
//! llama-server starts the servers that are commands (`--mcp-servers-config`); one that is an address is reached
//! through QuickAI itself, started as a command that carries what is said between the two (`mcp-bridge`).

use std::collections::BTreeMap;

use serde_json::{json, Map, Value as Json};

/// What QuickAI is started with to carry an MCP server at an address (`quickai mcp-bridge <url>`).
pub const BRIDGE_COMMAND: &str = "mcp-bridge";

/// The variable that hands the bridge the headers of its server, so that a key in them is not on a command line.
pub const BRIDGE_HEADERS_VAR: &str = "QUICKAI_MCP_HEADERS";

/// One server of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    /// Its name: llama-server names its tools `<name>_<tool>`.
    pub name: String,
    pub reach: Reach,
    /// Kept in the list and not used.
    pub disabled: bool,
}

/// How a server is reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reach {
    /// A program started by the engine, which talks over its input and output.
    Command {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
    },
    /// A server at an address (Streamable HTTP), with the headers it is sent (a key).
    Url {
        url: String,
        headers: BTreeMap<String, String>,
    },
}

impl Server {
    /// What the list says of it in a sentence: `npx -y @modelcontextprotocol/server-filesystem ~/notes`.
    pub fn summary(&self) -> String {
        match &self.reach {
            Reach::Command { command, args, .. } => {
                let mut words = vec![command.clone()];
                words.extend(args.iter().map(|a| {
                    if a.contains(char::is_whitespace) {
                        format!("\"{a}\"")
                    } else {
                        a.clone()
                    }
                }));
                words.join(" ")
            }
            Reach::Url { url, .. } => url.clone(),
        }
    }
}

/// The servers of a field's text. Empty text is an empty list; text that is not a list of servers says why.
pub fn parse(text: &str) -> Result<Vec<Server>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let json: Json = serde_json::from_str(text).map_err(|e| format!("this is not JSON: {e}"))?;
    let Json::Object(top) = json else {
        return Err("the list is a JSON object, {\"mcpServers\": {...}}".to_string());
    };
    // the forms the programs that use MCP print
    let table = if let Some(t) = top.get("mcpServers").or_else(|| top.get("servers")) {
        t.as_object()
            .ok_or("\"mcpServers\" is a table of servers by name")?
            .clone()
    } else if top.contains_key("command") || top.contains_key("url") {
        // one server, without a name
        let mut one = Map::new();
        one.insert("server".to_string(), Json::Object(top));
        one
    } else {
        // a bare table of servers by name
        top
    };
    let mut servers = Vec::new();
    for (name, body) in table {
        servers.push(server(&name, &body)?);
    }
    Ok(servers)
}

fn server(name: &str, body: &Json) -> Result<Server, String> {
    let Json::Object(body) = body else {
        return Err(format!("the server \"{name}\" is not a table"));
    };
    let text = |key: &str| body.get(key).and_then(Json::as_str).map(str::trim);
    let table = |key: &str| -> Result<BTreeMap<String, String>, String> {
        match body.get(key) {
            None | Some(Json::Null) => Ok(BTreeMap::new()),
            Some(Json::Object(map)) => Ok(map
                .iter()
                .map(|(k, v)| {
                    let v = match v {
                        Json::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    (k.clone(), v)
                })
                .collect()),
            Some(_) => Err(format!("\"{key}\" of the server \"{name}\" is a table of names and values")),
        }
    };
    let disabled = body.get("disabled").and_then(Json::as_bool).unwrap_or(false)
        || body.get("enabled").and_then(Json::as_bool) == Some(false);
    let reach = if let Some(command) = text("command").filter(|c| !c.is_empty()) {
        let args = match body.get("args") {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Array(items)) => items
                .iter()
                .map(|a| match a {
                    Json::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect(),
            Some(_) => return Err(format!("\"args\" of the server \"{name}\" is a list of words")),
        };
        Reach::Command {
            command: command.to_string(),
            args,
            env: table("env")?,
        }
    } else if let Some(url) = text("url").or_else(|| text("serverUrl")).filter(|u| !u.is_empty()) {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(format!("the address of the server \"{name}\" begins with http:// or https://"));
        }
        Reach::Url {
            url: url.to_string(),
            headers: table("headers")?,
        }
    } else {
        return Err(format!("the server \"{name}\" has neither a \"command\" nor a \"url\""));
    };
    Ok(Server {
        name: name.trim().to_string(),
        reach,
        disabled,
    })
}

/// The list as the field keeps it: `{"mcpServers": {...}}`, readable, by name. An empty list is no text.
pub fn to_text(servers: &[Server]) -> String {
    if servers.is_empty() {
        return String::new();
    }
    let mut table = Map::new();
    for s in servers {
        let mut body = Map::new();
        match &s.reach {
            Reach::Command { command, args, env } => {
                body.insert("command".into(), json!(command));
                if !args.is_empty() {
                    body.insert("args".into(), json!(args));
                }
                if !env.is_empty() {
                    body.insert("env".into(), json!(env));
                }
            }
            Reach::Url { url, headers } => {
                body.insert("url".into(), json!(url));
                if !headers.is_empty() {
                    body.insert("headers".into(), json!(headers));
                }
            }
        }
        if s.disabled {
            body.insert("disabled".into(), json!(true));
        }
        table.insert(s.name.clone(), Json::Object(body));
    }
    serde_json::to_string_pretty(&json!({ "mcpServers": table })).unwrap_or_default()
}

/// A name llama-server can put in front of a tool's: letters, digits, `-` and `_`.
pub fn tool_prefix(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if cleaned.trim_matches('_').is_empty() {
        "server".to_string()
    } else {
        cleaned
    }
}

/// What the engine is given: llama-server's list (`{"mcpServers": {...}}`) of the servers in use, each a command;
/// one at an address is QuickAI (`bridge`, its executable) started to carry it. `None` when no server is in use.
/// The notes say what was left out and why.
pub fn engine_config(servers: &[Server], bridge: Option<&str>) -> (Option<String>, Vec<String>) {
    let mut table = Map::new();
    let mut notes = Vec::new();
    for s in servers.iter().filter(|s| !s.disabled) {
        let mut name = tool_prefix(&s.name);
        while table.contains_key(&name) {
            name.push('_');
        }
        let body = match &s.reach {
            Reach::Command { command, args, env } => {
                json!({ "command": command, "args": args, "env": env })
            }
            Reach::Url { url, headers } => {
                let Some(bridge) = bridge.filter(|b| !b.is_empty()) else {
                    notes.push(format!(
                        "The MCP server {} is left out: QuickAI cannot tell where its own program is, which carries a server at an address.",
                        s.name
                    ));
                    continue;
                };
                let mut env = Map::new();
                if !headers.is_empty() {
                    env.insert(
                        BRIDGE_HEADERS_VAR.into(),
                        json!(serde_json::to_string(headers).unwrap_or_default()),
                    );
                }
                json!({ "command": bridge, "args": [BRIDGE_COMMAND, url], "env": env })
            }
        };
        table.insert(name, body);
    }
    if table.is_empty() {
        return (None, notes);
    }
    let text = serde_json::to_string_pretty(&json!({ "mcpServers": table })).unwrap_or_default();
    (Some(text), notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_forms_that_are_pasted_are_read() {
        let claude = r#"{"mcpServers": {"files": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/a"], "env": {"DEBUG": "1"}}}}"#;
        let s = parse(claude).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "files");
        assert_eq!(s[0].summary(), "npx -y @modelcontextprotocol/server-filesystem /home/a");

        let vscode = r#"{"servers": {"gh": {"type": "http", "url": "https://api.example.com/mcp", "headers": {"Authorization": "Bearer x"}}}}"#;
        let s = parse(vscode).unwrap();
        assert!(matches!(&s[0].reach, Reach::Url { url, headers } if url == "https://api.example.com/mcp" && headers["Authorization"] == "Bearer x"));

        let bare = r#"{"time": {"command": "uvx", "args": ["mcp-server-time"], "disabled": true}}"#;
        let s = parse(bare).unwrap();
        assert!(s[0].disabled);

        let one = r#"{"command": "uvx", "args": ["mcp-server-fetch"]}"#;
        assert_eq!(parse(one).unwrap()[0].name, "server");

        assert_eq!(parse("  ").unwrap(), Vec::new());
        assert!(parse("{\"mcpServers\": {\"x\": {}}}").unwrap_err().contains("neither"));
        assert!(parse("[1]").is_err());
        assert!(parse("{\"x\": {\"url\": \"ftp://a\"}}").is_err());
    }

    #[test]
    fn the_text_kept_reads_back_the_same() {
        let text = r#"{"mcpServers": {"b": {"url": "https://x/mcp", "headers": {"K": "v"}, "disabled": true}, "a": {"command": "c", "args": ["1 2"]}}}"#;
        let servers = parse(text).unwrap();
        assert_eq!(parse(&to_text(&servers)).unwrap(), servers);
        assert_eq!(to_text(&[]), "");
    }

    #[test]
    fn the_engine_gets_the_servers_in_use_and_an_address_through_quickai() {
        let text = r#"{"mcpServers": {"my files": {"command": "npx", "args": ["x"]}, "web": {"url": "https://w/mcp", "headers": {"Authorization": "Bearer k"}}, "off": {"command": "y", "disabled": true}}}"#;
        let servers = parse(text).unwrap();
        let (config, notes) = engine_config(&servers, Some("/opt/quickai/quickai"));
        let config: Json = serde_json::from_str(&config.unwrap()).unwrap();
        let table = config["mcpServers"].as_object().unwrap();
        assert_eq!(table.len(), 2, "the one switched off is left out");
        assert_eq!(table["my_files"]["command"], "npx");
        assert_eq!(table["web"]["command"], "/opt/quickai/quickai");
        assert_eq!(table["web"]["args"], json!(["mcp-bridge", "https://w/mcp"]));
        let headers: BTreeMap<String, String> =
            serde_json::from_str(table["web"]["env"][BRIDGE_HEADERS_VAR].as_str().unwrap()).unwrap();
        assert_eq!(headers["Authorization"], "Bearer k");
        assert!(notes.is_empty());

        let (config, notes) = engine_config(&servers, None);
        assert!(config.is_some());
        assert_eq!(notes.len(), 1, "an address cannot be carried without QuickAI's own program");
        assert_eq!(engine_config(&[], Some("q")), (None, Vec::new()));
    }
}
