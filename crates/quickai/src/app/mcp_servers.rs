//! The MCP servers of an engine (`ui.widget = "mcp_servers"`, `quickai_core::mcp`): the tools a model may use in the
//! engine's chat page, on a page of the Settings of their own.
//!
//! The field holds the list as text, in the form the programs that use MCP print. The page shows it as a list, a server a
//! row that can be switched off, changed or removed, and adds one from what its instructions give: a command or an
//! address with its variables or headers, or the JSON to paste. The whole text can be edited as it is too. Each change
//! is handed back as the new text; what is half typed in the boxes is the window's own (egui's memory of the page),
//! not the field's.

use eframe::egui;
use quickai_core::form::FormField;
use quickai_core::mcp::{self, Reach, Server};
use quickai_core::value::Value;

use super::theme;
use super::widgets::{Reply, MCP_SERVERS};
use super::App;

/// The server a line of llama-server's log says gave no tools (`MCP warmup: 'wiki' discovered 0 tools`). If the words
/// change, what is lost is only the line that says why.
fn server_without_tools(line: &str) -> Option<&str> {
    let (_, rest) = line.split_once("MCP warmup: '")?;
    let (name, rest) = rest.split_once('\'')?;
    rest.trim_start()
        .starts_with("discovered 0 tools")
        .then_some(name)
}

/// What the log says after a server at an address gave no tools, once it was asked why. Its name and not its address:
/// an address can hold a key.
fn why_no_tools(name: &str, found: Result<usize, String>) -> String {
    match found {
        Err(why) => format!("[MCP] {name} gave no tools: {why}"),
        Ok(0) => format!(
            "[MCP] {name} answers but offers no tools. Look at its instructions, and at whether its key allows them."
        ),
        Ok(tools) => format!(
            "[MCP] {name} gave no tools at this Start, and has {tools} now. Stop and start the model again to use them."
        ),
    }
}

impl App {
    /// For each line of the engine's log. llama-server says that a server gave no tools and keeps to itself what the
    /// bridge told it about why, so a server at an address is asked again, on a thread, and the log gets the answer.
    /// (A server that is a command has no one to ask but the program, which llama-server already started.)
    pub(super) fn explain_mcp_line(&mut self, line: &str, ctx: &egui::Context) {
        let Some(name) = server_without_tools(line) else {
            return;
        };
        let Some(field) = self.engine_doc().and_then(|engine| {
            engine
                .doc
                .fields
                .iter()
                .find(|f| f.ui.widget.as_deref() == Some(MCP_SERVERS))
        }) else {
            return;
        };
        let servers =
            mcp::parse(self.values.text(&field.id).unwrap_or_default()).unwrap_or_default();
        let Some(Reach::Url { url, headers }) = servers
            .into_iter()
            .find(|server| server.name == name)
            .map(|server| server.reach)
        else {
            return;
        };
        let name = name.to_string();
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.mcp_checks.push(rx);
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let found = quickai_runtime::mcp_bridge::check(&url, &headers);
            let _ = tx.send(why_no_tools(&name, found));
            wake.request_repaint();
        });
    }

    /// Each frame: what the servers that were asked have answered goes in the log.
    pub(super) fn poll_mcp_checks(&mut self) {
        let mut said = Vec::new();
        self.mcp_checks.retain(|rx| match rx.try_recv() {
            Ok(line) => {
                said.push(line);
                false
            }
            Err(crossbeam_channel::TryRecvError::Empty) => true,
            Err(crossbeam_channel::TryRecvError::Disconnected) => false,
        });
        for line in said {
            self.push_log(line);
        }
    }
}

/// What the boxes of the page hold between two frames.
#[derive(Clone, Default)]
struct Boxes {
    name: String,
    /// The command with its arguments, or the address.
    line: String,
    /// KEY=VALUE a line: the variables of a command, the headers of an address (`Name: value` is read too).
    pairs: String,
    /// The server being changed, by its place in the list: the boxes hold it, and Save puts it back there.
    editing: Option<usize>,
    /// JSON pasted to add.
    paste: String,
    /// The whole list as text, while it is edited as it is.
    raw: Option<String>,
    /// What is wrong with what was given, said under it.
    problem: Option<String>,
}

/// The list and the ways to change it. What comes back is the list's new text, when the person changed it.
pub(super) fn editor(ui: &mut egui::Ui, field: &FormField) -> Option<Reply> {
    let id = ui.id().with(("mcp_servers", &field.key));
    let mut boxes: Boxes = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
    let text = field.value.as_str().unwrap_or_default().to_string();
    let reply = draw(ui, &text, &mut boxes);
    ui.data_mut(|d| d.insert_temp(id, boxes));
    reply
}

fn draw(ui: &mut egui::Ui, text: &str, boxes: &mut Boxes) -> Option<Reply> {
    let p = theme::palette();
    // the text as it is, when that is how it is being edited, or when it cannot be read as a list
    let parsed = mcp::parse(text);
    if boxes.raw.is_some() || parsed.is_err() {
        return raw_editor(ui, text, boxes, parsed.err());
    }
    let mut servers = parsed.unwrap_or_default();
    let mut changed = false;

    if servers.is_empty() {
        ui.label(theme::muted(
            "No servers yet. Add one below: its instructions give a command to run or an address.",
        ));
    }
    let mut remove = None;
    for (i, server) in servers.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let mut on = !server.disabled;
            if ui
                .checkbox(&mut on, "")
                .on_hover_text(
                    "Used from the next Start; off, it stays in the list and is not used.",
                )
                .changed()
            {
                server.disabled = !on;
                changed = true;
            }
            let kind = match server.reach {
                Reach::Command { .. } => "command",
                Reach::Url { .. } => "address",
            };
            ui.label(egui::RichText::new(&server.name).strong().color(if on {
                p.text
            } else {
                p.muted
            }))
            .on_hover_text(format!(
                "Its tools are called {}_<tool> in the chat page.",
                mcp::tool_prefix(&server.name)
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Remove")
                    .on_hover_text("Take it out of the list")
                    .clicked()
                {
                    remove = Some(i);
                }
                if ui.small_button("Change").clicked() {
                    load(boxes, i, server);
                }
                ui.add(
                    egui::Label::new(theme::muted(format!("{kind} · {}", server.summary())))
                        .truncate(),
                )
                .on_hover_text(server.summary());
            });
        });
    }
    if let Some(i) = remove {
        servers.remove(i);
        if boxes.editing == Some(i) {
            *boxes = Boxes::default();
        }
        changed = true;
    }

    ui.add_space(10.0);
    ui.label(
        egui::RichText::new(if boxes.editing.is_some() {
            "Change the server"
        } else {
            "Add a server"
        })
        .strong(),
    );
    egui::Grid::new(ui.id().with("mcp_add"))
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Name:");
            ui.add(
                egui::TextEdit::singleline(&mut boxes.name)
                    .hint_text("e.g. files")
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();
            ui.label("Command or address:");
            ui.add(
                egui::TextEdit::singleline(&mut boxes.line)
                    .hint_text("npx -y @modelcontextprotocol/server-filesystem ~/Documents   or   https://…/mcp")
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();
            let address = is_address(&boxes.line);
            ui.label(if address { "Headers:" } else { "Variables:" })
                .on_hover_text(if address {
                    "Sent with every request, one a line: Authorization: Bearer <key>"
                } else {
                    "Set for the program, one KEY=VALUE a line: an API key it asks for"
                });
            ui.add(
                egui::TextEdit::multiline(&mut boxes.pairs)
                    .hint_text(if address { "Authorization: Bearer …" } else { "API_KEY=…" })
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();
        });
    ui.horizontal(|ui| {
        let ready = !boxes.name.trim().is_empty() && !boxes.line.trim().is_empty();
        let label = if boxes.editing.is_some() {
            "Save"
        } else {
            "Add"
        };
        if theme::button_sized(
            ui,
            theme::Role::Primary,
            ready,
            label,
            egui::vec2(70.0, 0.0),
        )
        .clicked()
        {
            match server_from(boxes) {
                Ok(server) => {
                    match boxes.editing.filter(|i| *i < servers.len()) {
                        Some(i) => servers[i] = server,
                        None => put(&mut servers, server),
                    }
                    *boxes = Boxes::default();
                    changed = true;
                }
                Err(why) => boxes.problem = Some(why),
            }
        }
        if (boxes.editing.is_some() || !boxes.line.is_empty() || !boxes.name.is_empty())
            && ui.button("Clear").clicked()
        {
            let raw = boxes.raw.take();
            *boxes = Boxes {
                raw,
                ..Boxes::default()
            };
        }
    });

    ui.add_space(6.0);
    egui::CollapsingHeader::new("Paste the JSON of a server's instructions")
        .id_salt(ui.id().with("mcp_paste"))
        .show(ui, |ui| {
            ui.label(theme::muted(
                "As a server's read-me gives it for Claude Desktop, Cursor or VS Code: {\"mcpServers\": {…}}. A server of the same name is replaced.",
            ));
            ui.add(
                egui::TextEdit::multiline(&mut boxes.paste)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(5)
                    .desired_width(f32::INFINITY),
            );
            if ui
                .add_enabled(!boxes.paste.trim().is_empty(), egui::Button::new("Add these"))
                .clicked()
            {
                match mcp::parse(&boxes.paste) {
                    Ok(given) if given.is_empty() => boxes.problem = Some("There is no server in it.".into()),
                    Ok(given) => {
                        for server in given {
                            put(&mut servers, server);
                        }
                        boxes.paste.clear();
                        boxes.problem = None;
                        changed = true;
                    }
                    Err(why) => boxes.problem = Some(format!("It cannot be read: {why}")),
                }
            }
        });
    if let Some(problem) = &boxes.problem {
        ui.colored_label(p.bad, problem);
    }
    ui.add_space(4.0);
    if ui
        .small_button("Edit the list as JSON")
        .on_hover_text("The whole list as text, as it is kept")
        .clicked()
    {
        boxes.raw = Some(text.to_string());
    }

    changed.then(|| Reply::Set(Value::Str(mcp::to_text(&servers))))
}

/// The whole list as text: kept when it reads as a list, and what is wrong said under it while it does not.
fn raw_editor(
    ui: &mut egui::Ui,
    text: &str,
    boxes: &mut Boxes,
    problem: Option<String>,
) -> Option<Reply> {
    let raw = boxes.raw.get_or_insert_with(|| text.to_string());
    ui.add(
        egui::TextEdit::multiline(raw)
            .font(egui::TextStyle::Monospace)
            .desired_rows(10)
            .desired_width(f32::INFINITY)
            .hint_text("{\"mcpServers\": {\"files\": {\"command\": \"npx\", \"args\": [\"-y\", \"@modelcontextprotocol/server-filesystem\", \"/home/me\"]}}}"),
    );
    let now = mcp::parse(raw);
    match (&now, &problem) {
        (Err(why), _) => {
            ui.colored_label(
                theme::palette().bad,
                format!("Not used until it can be read: {why}"),
            );
        }
        (Ok(_), Some(_)) => {}
        (Ok(_), None) => {}
    }
    let mut reply = None;
    ui.horizontal(|ui| {
        let readable = now.is_ok();
        if ui
            .add_enabled(readable, egui::Button::new("Done"))
            .on_disabled_hover_text("The text cannot be read as a list of servers yet")
            .clicked()
        {
            if let Ok(servers) = &now {
                reply = Some(Reply::Set(Value::Str(mcp::to_text(servers))));
            }
            boxes.raw = None;
        }
        if problem.is_none() && ui.button("Cancel").clicked() {
            boxes.raw = None;
        }
    });
    reply
}

/// Whether what was typed is an address rather than a command.
fn is_address(line: &str) -> bool {
    let line = line.trim().to_ascii_lowercase();
    line.starts_with("http://") || line.starts_with("https://")
}

/// The boxes hold the server at `i` of the list, to be changed.
fn load(boxes: &mut Boxes, i: usize, server: &Server) {
    boxes.editing = Some(i);
    boxes.name = server.name.clone();
    boxes.problem = None;
    match &server.reach {
        Reach::Command { env, .. } => {
            boxes.line = server.summary();
            boxes.pairs = env
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("\n");
        }
        Reach::Url { url, headers } => {
            boxes.line = url.clone();
            boxes.pairs = headers
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
}

/// A server of what the boxes hold.
fn server_from(boxes: &Boxes) -> Result<Server, String> {
    let name = boxes.name.trim().to_string();
    let line = boxes.line.trim();
    let mut pairs = std::collections::BTreeMap::new();
    for raw in boxes
        .pairs
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        // a header is `Name: value`, a variable `NAME=value`; either is read in either box
        let split = if is_address(line) {
            raw.split_once(':').or_else(|| raw.split_once('='))
        } else {
            raw.split_once('=').or_else(|| raw.split_once(':'))
        };
        let Some((k, v)) = split.filter(|(k, _)| !k.trim().is_empty()) else {
            return Err(format!("\"{raw}\" is not NAME=value"));
        };
        pairs.insert(k.trim().to_string(), v.trim().to_string());
    }
    let reach = if is_address(line) {
        Reach::Url {
            url: line.to_string(),
            headers: pairs,
        }
    } else {
        let mut words = quickai_core::plan::args::split_args(line).into_iter();
        let Some(command) = words.next() else {
            return Err("Give the command that starts the server.".into());
        };
        Reach::Command {
            command,
            args: words.collect(),
            env: pairs,
        }
    };
    Ok(Server {
        name,
        reach,
        disabled: false,
    })
}

/// Adds a server, or replaces the one of the same name.
fn put(servers: &mut Vec<Server>, server: Server) {
    match servers.iter_mut().find(|s| s.name == server.name) {
        Some(same) => *same = server,
        None => servers.push(server),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_boxes_hold_becomes_a_server() {
        let boxes = Boxes {
            name: "files".into(),
            line: "npx -y @modelcontextprotocol/server-filesystem \"/home/me/My Notes\"".into(),
            pairs: "DEBUG=1\n# a comment\n".into(),
            ..Boxes::default()
        };
        let server = server_from(&boxes).unwrap();
        assert_eq!(
            server.reach,
            Reach::Command {
                command: "npx".into(),
                args: vec![
                    "-y".into(),
                    "@modelcontextprotocol/server-filesystem".into(),
                    "/home/me/My Notes".into()
                ],
                env: [("DEBUG".to_string(), "1".to_string())].into(),
            }
        );
        let boxes = Boxes {
            name: "web".into(),
            line: "https://example.com/mcp".into(),
            pairs: "Authorization: Bearer abc".into(),
            ..Boxes::default()
        };
        let server = server_from(&boxes).unwrap();
        assert!(
            matches!(&server.reach, Reach::Url { headers, .. } if headers["Authorization"] == "Bearer abc")
        );
        let bad = Boxes {
            pairs: "nothing".into(),
            ..boxes
        };
        assert!(server_from(&bad).is_err());
    }

    #[test]
    fn a_server_loaded_into_the_boxes_comes_back_the_same() {
        let text = r#"{"mcpServers": {"web": {"url": "https://x/mcp", "headers": {"K": "v"}}, "f": {"command": "uvx", "args": ["a b", "c"], "env": {"X": "1"}}}}"#;
        for (i, server) in mcp::parse(text).unwrap().iter().enumerate() {
            let mut boxes = Boxes::default();
            load(&mut boxes, i, server);
            assert_eq!(&server_from(&boxes).unwrap(), server);
        }
    }

    #[test]
    fn the_line_that_says_a_server_gave_no_tools_is_told_from_the_others() {
        assert_eq!(
            server_without_tools(
                "0.00.646.887 I srv         start: MCP warmup: 'wiki' discovered 0 tools"
            ),
            Some("wiki")
        );
        for other in [
            "0.00.592.906 I srv         start: MCP warmup: 'files' discovered 14 tools",
            "0.00.027.012 W srv         start: MCP warmup: failed to spawn 'broken': ",
            "0.00.646.887 I srv  setup: Added 0 MCP tools",
        ] {
            assert_eq!(server_without_tools(other), None, "{other}");
        }
    }

    /// The Log tab says why a server at an address gave no tools; a command, or a server not on the list, is not asked.
    #[test]
    fn a_server_at_an_address_that_gave_no_tools_is_asked_why_and_the_log_says_it() {
        use crate::config::{build_registry, ConfigDirs};
        use crate::settings::Settings;
        let dir = crate::testkit::scratch("mcp-why");
        let mut settings = Settings::default();
        settings.set_chosen_engine("llamacpp");
        let mut app = App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * 1024 * 1024 * 1024,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        );
        // nothing listens here
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed = format!("http://{}/mcp", listener.local_addr().unwrap());
        drop(listener);
        app.values.set(
            "mcp_servers",
            format!(
                r#"{{"mcpServers": {{"wiki": {{"url": "{closed}"}}, "files": {{"command": "npx"}}}}}}"#
            ),
        );
        let ctx = egui::Context::default();

        app.explain_mcp_line("I srv start: MCP warmup: 'files' discovered 0 tools", &ctx);
        app.explain_mcp_line("I srv start: MCP warmup: 'other' discovered 0 tools", &ctx);
        app.explain_mcp_line("I srv start: MCP warmup: 'wiki' discovered 3 tools", &ctx);
        assert!(app.mcp_checks.is_empty());

        app.explain_mcp_line("I srv start: MCP warmup: 'wiki' discovered 0 tools", &ctx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !app.mcp_checks.is_empty() {
            assert!(std::time::Instant::now() < deadline, "no answer");
            app.poll_mcp_checks();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let line = app.log_lines.back().expect("a line in the log");
        assert!(line.starts_with("[MCP] wiki gave no tools: "), "{line}");
        assert!(
            !line.contains(&closed),
            "the address can hold a key: {line}"
        );
    }
}
