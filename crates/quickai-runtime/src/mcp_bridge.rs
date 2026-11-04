//! `quickai mcp-bridge <url>`: an MCP server at an address, for an engine that only starts servers as commands.
//!
//! llama-server starts each MCP server of its list as a program and talks to it in lines of JSON-RPC over its input
//! and output; a server at an address (Streamable HTTP) has no program. QuickAI is that program: it reads each line
//! the engine writes, POSTs it to the address, and writes back what the server answers, whether the answer is one
//! JSON body or a stream of events. The session the server gives, and the version of the protocol agreed on, go
//! with every later request. The headers of the server (a key) come in a variable, not on the command line
//! (`quickai_core::mcp::BRIDGE_HEADERS_VAR`). What goes wrong is said on the error output, and a request that got no
//! answer is answered with an error, so that the engine does not wait for it. The engine keeps both to itself: it logs
//! that the server gave no tools and nothing more, so the window asks the server again (`check`) to say why.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::time::Duration;

pub use quickai_core::mcp::BRIDGE_COMMAND;
use quickai_core::mcp::BRIDGE_HEADERS_VAR;
use serde_json::{json, Value as Json};

/// How long the server may take to accept the connection. A tool may then work for as long as it needs.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long `check` waits for a server, all told. Nothing is waiting for it but a line of the log.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// What the bridge remembers between requests.
#[derive(Default)]
struct Session {
    /// The session the server gave (`Mcp-Session-Id`).
    id: Option<String>,
    /// The version of the protocol agreed on at `initialize`.
    protocol: Option<String>,
}

/// Runs the bridge for `args` (`<url>`) until the engine closes its input. The exit code.
pub fn run(args: &[String]) -> i32 {
    let Some(url) = args.first() else {
        eprintln!("usage: quickai mcp-bridge <url>   (headers as JSON in {BRIDGE_HEADERS_VAR})");
        return 2;
    };
    let headers: BTreeMap<String, String> = match std::env::var(BRIDGE_HEADERS_VAR) {
        Ok(text) if !text.trim().is_empty() => match serde_json::from_str(&text) {
            Ok(headers) => headers,
            Err(e) => {
                eprintln!("mcp-bridge: {BRIDGE_HEADERS_VAR} is not a JSON table of headers: {e}");
                return 2;
            }
        },
        _ => BTreeMap::new(),
    };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .build()
        .into();
    let mut session = Session::default();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let message: Json = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(e) => {
                eprintln!("mcp-bridge: a line that is not JSON was left out: {e}");
                continue;
            }
        };
        let id = message.get("id").filter(|v| !v.is_null()).cloned();
        let initialize = message.get("method").and_then(Json::as_str) == Some("initialize");
        let replies = match exchange(&agent, url, &headers, &mut session, &line, id.as_ref()) {
            Ok(replies) => replies,
            Err(why) => {
                eprintln!("mcp-bridge: {url}: {why}");
                match &id {
                    Some(id) if message.get("method").is_some() => vec![json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32000, "message": format!("the MCP server at {url} could not be reached: {why}") },
                    })],
                    _ => Vec::new(),
                }
            }
        };
        let mut out = stdout.lock();
        for reply in replies {
            if initialize {
                if let Some(version) = reply
                    .pointer("/result/protocolVersion")
                    .and_then(Json::as_str)
                {
                    session.protocol = Some(version.to_string());
                }
            }
            if serde_json::to_writer(&mut out, &reply).is_err()
                || out.write_all(b"\n").is_err()
                || out.flush().is_err()
            {
                return 0; // the engine is gone
            }
        }
    }
    0
}

/// Asks the server at `url` what the engine asks when it starts: how many tools it has now, or why it gives none. Keeps
/// no session: the engine's own ended with its answer.
pub fn check(url: &str, headers: &BTreeMap<String, String>) -> Result<usize, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(CHECK_TIMEOUT))
        .build()
        .into();
    let mut session = Session::default();
    let hello = answer_to(
        &agent,
        url,
        headers,
        &mut session,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "quickai", "version": env!("CARGO_PKG_VERSION") },
            },
        }),
    )?;
    session.protocol = hello
        .pointer("/result/protocolVersion")
        .and_then(Json::as_str)
        .map(str::to_string);
    let ready = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    exchange(&agent, url, headers, &mut session, &ready.to_string(), None)?;
    let tools = answer_to(
        &agent,
        url,
        headers,
        &mut session,
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    )?;
    Ok(tools
        .pointer("/result/tools")
        .and_then(Json::as_array)
        .map_or(0, Vec::len))
}

/// The answer to one request; an error the server sent is an error here.
fn answer_to(
    agent: &ureq::Agent,
    url: &str,
    headers: &BTreeMap<String, String>,
    session: &mut Session,
    request: &Json,
) -> Result<Json, String> {
    let id = request.get("id");
    let replies = exchange(agent, url, headers, session, &request.to_string(), id)?;
    let answer = replies
        .into_iter()
        .find(|reply| reply.get("id") == id)
        .ok_or("the server did not answer")?;
    match answer.get("error") {
        Some(error) => Err(error
            .get("message")
            .and_then(Json::as_str)
            .map_or_else(|| error.to_string(), str::to_string)),
        None => Ok(answer),
    }
}

/// POSTs one message and gives the messages the server answers with: none for a notification it accepted, the
/// response (and whatever the server says before it) for a request.
fn exchange(
    agent: &ureq::Agent,
    url: &str,
    headers: &BTreeMap<String, String>,
    session: &mut Session,
    body: &str,
    id: Option<&Json>,
) -> Result<Vec<Json>, String> {
    let mut request = agent
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("user-agent", concat!("quickai/", env!("CARGO_PKG_VERSION")));
    for (name, value) in headers {
        request = request.header(name.as_str(), value.as_str());
    }
    if let Some(id) = &session.id {
        request = request.header("mcp-session-id", id.as_str());
    }
    if let Some(version) = &session.protocol {
        request = request.header("mcp-protocol-version", version.as_str());
    }
    let response = request.send(body).map_err(|e| e.to_string())?;
    if let Some(given) = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
    {
        session.id = Some(given.to_string());
    }
    let status = response.status().as_u16();
    let kind = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut answer = response.into_body();
    if status == 202 || status == 204 {
        return Ok(Vec::new());
    }
    if !(200..300).contains(&status) {
        let text = answer.read_to_string().unwrap_or_default();
        let text = text.trim();
        let older = if (status == 404 || status == 405)
            && url.trim_end_matches('/').ends_with("/sse")
        {
            " (an address that ends in /sse is a server of the older kind, which QuickAI does not carry: ask its maker for its Streamable HTTP address, often the same with /mcp)"
        } else {
            ""
        };
        return Err(format!(
            "the server answered {status}{}{older}",
            if text.is_empty() {
                String::new()
            } else {
                format!(": {}", text.chars().take(300).collect::<String>())
            }
        ));
    }
    if kind.starts_with("text/event-stream") {
        return Ok(events(answer.into_reader(), id));
    }
    let text = answer.read_to_string().map_err(|e| e.to_string())?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Json>(&text).map_err(|e| format!("the answer is not JSON: {e}"))? {
        Json::Array(batch) => Ok(batch),
        one => Ok(vec![one]),
    }
}

/// The messages of a stream of events, up to the response to the request `id` (the server may say other things
/// first: progress, a log line, a request of its own).
fn events(reader: impl Read, id: Option<&Json>) -> Vec<Json> {
    let mut messages = Vec::new();
    let mut data = String::new();
    for line in BufReader::new(reader).lines() {
        let Ok(line) = line else { break };
        if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            continue;
        }
        if !line.is_empty() {
            continue; // `event:`, `id:`, `retry:` and comments say nothing the engine needs
        }
        // a blank line ends an event
        let text = std::mem::take(&mut data);
        if text.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Json>(&text) else {
            eprintln!("mcp-bridge: an event that is not JSON was left out");
            continue;
        };
        let answered = id.is_some_and(|id| {
            message.get("id") == Some(id)
                && (message.get("result").is_some() || message.get("error").is_some())
        });
        messages.push(message);
        if answered {
            break;
        }
    }
    if !data.trim().is_empty() {
        if let Ok(message) = serde_json::from_str::<Json>(&data) {
            messages.push(message);
        }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_of_events_gives_its_messages_up_to_the_answer() {
        let stream = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n\
                      : a comment\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\ndata: \"result\":{}}\n\n\
                      data: {\"jsonrpc\":\"2.0\",\"method\":\"after\"}\n\n";
        let got = events(stream.as_bytes(), Some(&json!(7)));
        assert_eq!(got.len(), 2);
        assert_eq!(got[1]["id"], 7);
    }

    /// A stand-in for a server at an address: each request is answered with the next of `answers`.
    fn serve(path: &str, answers: Vec<String>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}{path}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for answer in answers {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let mut length = 0;
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 0) && line != "\r\n" {
                    let lower = line.to_ascii_lowercase();
                    if let Some(value) = lower.strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                    line.clear();
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                let _ = reader.get_mut().write_all(answer.as_bytes());
            }
        });
        url
    }

    fn http(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn a_check_says_how_many_tools_a_server_has_or_why_it_gave_none() {
        let hello = http(
            "200 OK",
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26"}}"#,
        );
        let accepted = http("202 Accepted", "");
        let none = BTreeMap::new();

        let url = serve(
            "/mcp",
            vec![
                hello.clone(),
                accepted.clone(),
                http(
                    "200 OK",
                    r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"a"},{"name":"b"}]}}"#,
                ),
            ],
        );
        assert_eq!(check(&url, &none), Ok(2));

        // a key it does not take, said in its own words
        let url = serve("/mcp", vec![http("401 Unauthorized", "a key is needed")]);
        let why = check(&url, &none).unwrap_err();
        assert!(
            why.contains("401") && why.contains("a key is needed"),
            "{why}"
        );

        // the older kind of address
        let url = serve("/sse", vec![http("405 Method Not Allowed", "")]);
        assert!(check(&url, &none).unwrap_err().contains("older kind"));

        // it took the greeting and refused the list
        let url = serve(
            "/mcp",
            vec![
                hello,
                accepted,
                http(
                    "200 OK",
                    r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"no tools for you"}}"#,
                ),
            ],
        );
        assert_eq!(check(&url, &none), Err("no tools for you".to_string()));

        // nothing there
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed = format!("http://{}/mcp", listener.local_addr().unwrap());
        drop(listener);
        assert!(check(&closed, &none).is_err());
    }
}
