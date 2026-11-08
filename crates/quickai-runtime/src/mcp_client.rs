//! QuickAI's own MCP client (C204): it starts a server of the person's list, asks it for its tools and calls them, for
//! the chat of the window, whatever engine runs the model. A server that is a command is started as every program
//! QuickAI starts (a process group of its own, gone with QuickAI, no console window) and talked to in lines of JSON-RPC
//! over its input and output; one at an address is talked to over Streamable HTTP, as the bridge does (`mcp_bridge`).

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError};
use quickai_core::mcp::{Reach, Server};
use serde_json::{json, Value as Json};

use crate::mcp_bridge::{answer_to, exchange, Session};
use crate::runtime::process::Daemon;

/// How long a server may take to answer that it is there: `npx` and `uvx` fetch a server the first time it is started.
pub const START_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a tool may work.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(300);
/// The version of the protocol QuickAI speaks; a server answers with the one it takes.
const PROTOCOL: &str = "2025-03-26";
/// The lines of what a server said on its error output that a failure quotes.
const ERROR_LINES: usize = 6;

/// A tool a server offers: its name, what it does, and the JSON Schema of its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub schema: Json,
}

/// What a tool gave back: its text, and whether it says it failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
}

/// A server QuickAI talks to.
pub struct Client {
    transport: Transport,
    next_id: u64,
}

enum Transport {
    Stdio {
        daemon: Daemon,
        input: ChildStdin,
        lines: Receiver<Json>,
        errors: Arc<Mutex<VecDeque<String>>>,
    },
    Http {
        agent: ureq::Agent,
        url: String,
        headers: BTreeMap<String, String>,
        session: Session,
    },
}

impl Client {
    /// Starts `server` and says hello: the server is ready for `tools` and `call`, or why not.
    pub fn start(server: &Server) -> Result<Client, String> {
        let transport = match &server.reach {
            Reach::Command { command, args, env } => spawn(command, args, env)?,
            Reach::Url { url, headers } => Transport::Http {
                agent: ureq::Agent::config_builder()
                    .http_status_as_error(false)
                    .timeout_connect(Some(Duration::from_secs(30)))
                    .timeout_global(Some(CALL_TIMEOUT))
                    .build()
                    .into(),
                url: url.clone(),
                headers: headers.clone(),
                session: Session::default(),
            },
        };
        let mut client = Client {
            transport,
            next_id: 1,
        };
        let hello = client.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": { "name": "quickai", "version": env!("CARGO_PKG_VERSION") },
            }),
            START_TIMEOUT,
        )?;
        if let Transport::Http { session, .. } = &mut client.transport {
            session.protocol = hello
                .get("protocolVersion")
                .and_then(Json::as_str)
                .map(str::to_string);
        }
        client.notify("notifications/initialized")?;
        Ok(client)
    }

    /// The tools the server offers now (all of their pages).
    pub fn tools(&mut self) -> Result<Vec<Tool>, String> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..50 {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let page = self.request("tools/list", params, START_TIMEOUT)?;
            if let Some(list) = page.get("tools").and_then(Json::as_array) {
                tools.extend(list.iter().filter_map(tool_of));
            }
            cursor = page
                .get("nextCursor")
                .and_then(Json::as_str)
                .filter(|c| !c.is_empty())
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    /// Calls the tool `name` with `arguments` (a JSON object): what it gave back, as text.
    pub fn call(&mut self, name: &str, arguments: &Json) -> Result<ToolOutput, String> {
        let arguments = if arguments.is_object() {
            arguments.clone()
        } else {
            json!({})
        };
        let result = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
            CALL_TIMEOUT,
        )?;
        Ok(output_of(&result))
    }

    /// One request and its answer's `result`; an error the server sent is an error here.
    fn request(&mut self, method: &str, params: Json, timeout: Duration) -> Result<Json, String> {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let answer = match &mut self.transport {
            Transport::Http {
                agent,
                url,
                headers,
                session,
            } => answer_to(agent, url, headers, session, &message)?,
            Transport::Stdio {
                input,
                lines,
                errors,
                ..
            } => {
                send_line(input, &message)?;
                wait_for(id, input, lines, errors, timeout)?
            }
        };
        Ok(answer.get("result").cloned().unwrap_or(Json::Null))
    }

    /// A notification: nothing is answered.
    fn notify(&mut self, method: &str) -> Result<(), String> {
        let message = json!({ "jsonrpc": "2.0", "method": method });
        match &mut self.transport {
            Transport::Http {
                agent,
                url,
                headers,
                session,
            } => exchange(agent, url, headers, session, &message.to_string(), None).map(|_| ()),
            Transport::Stdio { input, .. } => send_line(input, &message),
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Transport::Stdio { daemon, .. } = &mut self.transport {
            daemon.kill();
            let _ = daemon.wait();
        }
    }
}

/// Starts the server's command, with its variables, and the threads that read what it says.
fn spawn(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> Result<Transport, String> {
    let mut cmd = Command::new(program(command));
    cmd.args(args).envs(env);
    let mut daemon = Daemon::start_with_input(cmd, true)
        .map_err(|e| format!("{command} could not be started: {e}"))?;
    let input = daemon
        .take_input()
        .ok_or_else(|| format!("{command} has no input to talk to"))?;
    let (stdout, stderr) = daemon.take_output();
    let (tx, lines) = crossbeam_channel::unbounded();
    if let Some(stdout) = stdout {
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                // a server that writes something else on its output (a banner) is not a message
                if let Ok(message) = serde_json::from_str::<Json>(line.trim()) {
                    if tx.send(message).is_err() {
                        break;
                    }
                }
            }
        });
    }
    let errors = Arc::new(Mutex::new(VecDeque::new()));
    if let Some(stderr) = stderr {
        let kept = errors.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                let Ok(mut kept) = kept.lock() else { break };
                kept.push_back(line);
                while kept.len() > ERROR_LINES {
                    kept.pop_front();
                }
            }
        });
    }
    Ok(Transport::Stdio {
        daemon,
        input,
        lines,
        errors,
    })
}

/// The program to start for `command`: on Windows `npx` and its kind are batch files (`npx.cmd`), which a process is not
/// started from by name alone.
fn program(command: &str) -> String {
    if cfg!(windows) && !command.contains(['/', '\\', '.']) {
        if let Ok(path) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path) {
                for ending in ["exe", "cmd", "bat"] {
                    let found = dir.join(format!("{command}.{ending}"));
                    if found.is_file() {
                        return found.display().to_string();
                    }
                }
            }
        }
    }
    command.to_string()
}

fn send_line(input: &mut ChildStdin, message: &Json) -> Result<(), String> {
    let mut line = message.to_string();
    line.push('\n');
    input
        .write_all(line.as_bytes())
        .and_then(|()| input.flush())
        .map_err(|e| format!("the server stopped taking messages: {e}"))
}

/// Waits for the answer to the request `id`; a request the server makes meanwhile is answered (it is asked nothing
/// QuickAI offers), and a notification is passed over.
fn wait_for(
    id: u64,
    input: &mut ChildStdin,
    lines: &Receiver<Json>,
    errors: &Arc<Mutex<VecDeque<String>>>,
    timeout: Duration,
) -> Result<Json, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let message = match lines.recv_timeout(left) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!(
                    "the server did not answer in {} s",
                    timeout.as_secs()
                ))
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(match last_words(errors) {
                    Some(said) => format!("the server stopped: {said}"),
                    None => "the server stopped".to_string(),
                })
            }
        };
        let method = message.get("method").and_then(Json::as_str);
        match (message.get("id"), method) {
            (Some(asked), Some(method)) => {
                // the server asks something: `ping` is answered, anything else is not offered
                let reply = if method == "ping" {
                    json!({ "jsonrpc": "2.0", "id": asked, "result": {} })
                } else if method == "roots/list" {
                    json!({ "jsonrpc": "2.0", "id": asked, "result": { "roots": [] } })
                } else {
                    json!({ "jsonrpc": "2.0", "id": asked, "error": { "code": -32601, "message": "not offered" } })
                };
                send_line(input, &reply)?;
            }
            (Some(answered), None) if answered.as_u64() == Some(id) => {
                if let Some(error) = message.get("error") {
                    return Err(error
                        .get("message")
                        .and_then(Json::as_str)
                        .map_or_else(|| error.to_string(), str::to_string));
                }
                return Ok(message);
            }
            _ => {}
        }
    }
}

/// The last lines the server said on its error output, joined.
fn last_words(errors: &Arc<Mutex<VecDeque<String>>>) -> Option<String> {
    let kept = errors.lock().ok()?;
    let said: Vec<&str> = kept
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    (!said.is_empty()).then(|| said.join(" · "))
}

/// A tool of a `tools/list` page.
fn tool_of(entry: &Json) -> Option<Tool> {
    Some(Tool {
        name: entry.get("name")?.as_str()?.to_string(),
        description: entry
            .get("description")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_string(),
        schema: entry
            .get("inputSchema")
            .cloned()
            .filter(Json::is_object)
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
    })
}

/// What a `tools/call` gave back, as the text a model reads: its text parts, a word for each picture or sound, a
/// resource's text or address, and the structured content when there is nothing else.
pub fn output_of(result: &Json) -> ToolOutput {
    let mut parts: Vec<String> = Vec::new();
    for item in result
        .get("content")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        match item.get("type").and_then(Json::as_str) {
            Some("text") => {
                if let Some(text) = item.get("text").and_then(Json::as_str) {
                    parts.push(text.to_string());
                }
            }
            Some("image") => parts.push("[a picture]".to_string()),
            Some("audio") => parts.push("[a sound]".to_string()),
            Some("resource") => {
                let resource = item.get("resource");
                match resource.and_then(|r| r.get("text")).and_then(Json::as_str) {
                    Some(text) => parts.push(text.to_string()),
                    None => {
                        if let Some(uri) =
                            resource.and_then(|r| r.get("uri")).and_then(Json::as_str)
                        {
                            parts.push(format!("[{uri}]"));
                        }
                    }
                }
            }
            Some("resource_link") => {
                if let Some(uri) = item.get("uri").and_then(Json::as_str) {
                    parts.push(format!("[{uri}]"));
                }
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        if let Some(structured) = result.get("structuredContent") {
            parts.push(structured.to_string());
        }
    }
    ToolOutput {
        text: parts.join("\n"),
        is_error: result
            .get("isError")
            .and_then(Json::as_bool)
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_tool_gave_back_is_its_text_with_a_word_for_what_is_not() {
        let result = json!({
            "content": [
                { "type": "text", "text": "two files" },
                { "type": "image", "data": "...", "mimeType": "image/png" },
                { "type": "resource", "resource": { "uri": "file:///a.md", "text": "# A" } },
                { "type": "resource_link", "uri": "file:///b.md", "name": "b" },
            ],
        });
        assert_eq!(
            output_of(&result),
            ToolOutput {
                text: "two files\n[a picture]\n# A\n[file:///b.md]".to_string(),
                is_error: false,
            }
        );
        let failed =
            json!({ "content": [{ "type": "text", "text": "no such file" }], "isError": true });
        assert!(output_of(&failed).is_error);
        let structured = json!({ "content": [], "structuredContent": { "n": 2 } });
        assert_eq!(output_of(&structured).text, "{\"n\":2}");
    }

    #[test]
    fn a_tool_without_a_schema_takes_an_empty_object() {
        let tool = tool_of(&json!({ "name": "now" })).unwrap();
        assert_eq!(tool.schema, json!({ "type": "object", "properties": {} }));
        assert!(tool_of(&json!({ "description": "no name" })).is_none());
    }

    /// I48 (C204): a server that is a command (here a little program in Python that speaks the protocol on its input and
    /// output) is started, asked for its tools and called.
    #[cfg(unix)]
    #[test]
    fn p3_i48_a_command_server_is_started_asked_for_its_tools_and_called() {
        let dir = std::env::temp_dir().join(format!("quickai-mcp-client-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("server.py");
        std::fs::write(
            &script,
            r#"import json, sys
for line in sys.stdin:
    m = json.loads(line)
    if "id" not in m:
        continue
    if m["method"] == "initialize":
        r = {"protocolVersion": "2025-03-26", "capabilities": {"tools": {}}, "serverInfo": {"name": "t"}}
    elif m["method"] == "tools/list":
        r = {"tools": [{"name": "echo", "description": "Says it back", "inputSchema": {"type": "object"}}]}
    elif m["method"] == "tools/call":
        r = {"content": [{"type": "text", "text": "you said " + m["params"]["arguments"].get("words", "")}]}
    else:
        r = {}
    print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": r}), flush=True)
"#,
        )
        .unwrap();
        let python = ["python3", "python"]
            .into_iter()
            .find(|p| Command::new(p).arg("--version").output().is_ok());
        let Some(python) = python else {
            return; // no Python on this computer: nothing to start the server with
        };
        let server = Server {
            name: "echo".into(),
            reach: Reach::Command {
                command: python.into(),
                args: vec![script.display().to_string()],
                env: BTreeMap::new(),
            },
            disabled: false,
        };
        let mut client = Client::start(&server).unwrap();
        let tools = client.tools().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].description, "Says it back");
        let said = client.call("echo", &json!({ "words": "hello" })).unwrap();
        assert_eq!(said.text, "you said hello");
        drop(client);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_command_that_is_not_there_says_so() {
        let server = Server {
            name: "gone".into(),
            reach: Reach::Command {
                command: "quickai-no-such-program".into(),
                args: Vec::new(),
                env: BTreeMap::new(),
            },
            disabled: false,
        };
        let why = Client::start(&server).err().unwrap();
        assert!(why.contains("could not be started"), "{why}");
    }
}
