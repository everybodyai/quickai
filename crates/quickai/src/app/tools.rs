//! The tools of QuickAI's own chat (C204; the user: "lets now fix the mcp-ollama stuff", of MCP that worked with
//! llama.cpp only, in its chat page): the person's MCP servers run by QuickAI itself on this computer
//! (`quickai_runtime::mcp_client`), so that the chat offers their tools with every engine that chats (llama.cpp, Ollama)
//! and with a model on another computer. Each server has a thread of its own: it is started when the chat or the page of
//! the MCP servers first needs it, says its tools, and calls them. A tool runs when the person allows it, once or always,
//! and never by itself until they said always; what it gave back goes to the model, which answers with it.
//!
//! The list is the one llama.cpp's field keeps (`settings::MCP_SERVERS_KEY`), so llama.cpp's own chat page has the same
//! servers; what the person said of each tool is kept in the settings (`settings::TOOL_PERMISSIONS_KEY`).

use std::collections::BTreeMap;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use quickai_core::mcp::{self, Server};
use quickai_runtime::mcp_client::{Client, Tool, ToolOutput};
use serde_json::{json, Value as Json};

use super::chat::{CallState, ToolCall};
use super::icons::{self, Icon};
use super::theme::{self, Role};
use super::App;

/// How many replies in a row may ask for tools after one message of the person's: then the chat stops asking for more.
const MOST_TOOL_ROUNDS: u32 = 8;
/// The longest description of a tool the model is given.
const MOST_DESCRIPTION: usize = 1024;

/// What a server is doing.
#[derive(Clone, Debug)]
pub enum ServerStatus {
    Starting,
    Ready(Vec<Tool>),
    Failed(String),
}

/// What the person said of a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    /// QuickAI asks each time the model wants it.
    Ask,
    /// It runs without asking.
    Always,
    /// The model is not offered it.
    Off,
}

impl Permission {
    fn word(self) -> Option<&'static str> {
        match self {
            Permission::Ask => None,
            Permission::Always => Some("always"),
            Permission::Off => Some("off"),
        }
    }
}

enum Command {
    Call {
        ticket: u64,
        tool: String,
        arguments: Json,
    },
}

enum Event {
    Status {
        server: String,
        status: ServerStatus,
    },
    Result {
        ticket: u64,
        result: Result<ToolOutput, String>,
    },
}

/// A server's thread, as the window holds it: dropping it ends the thread, and the server's program with it.
struct Worker {
    server: Server,
    commands: Sender<Command>,
}

/// The servers QuickAI runs, what each is doing, and the calls on their way.
#[derive(Default)]
pub struct Tools {
    workers: BTreeMap<String, Worker>,
    channel: Option<(Sender<Event>, Receiver<Event>)>,
    pub status: BTreeMap<String, ServerStatus>,
    next_ticket: u64,
    /// Started (by the chat or the page of the servers): from then on a change of the list is followed.
    pub started: bool,
}

impl Tools {
    /// Runs the servers of `servers` that are on and not running yet (or changed), and ends those no longer there.
    fn sync(&mut self, servers: &[Server], ctx: &egui::Context) {
        let events = self
            .channel
            .get_or_insert_with(crossbeam_channel::unbounded)
            .0
            .clone();
        let wanted: BTreeMap<String, Server> = servers
            .iter()
            .filter(|s| !s.disabled)
            .map(|s| (s.name.clone(), s.clone()))
            .collect();
        self.workers
            .retain(|name, worker| wanted.get(name) == Some(&worker.server));
        let running: Vec<String> = self.workers.keys().cloned().collect();
        self.status.retain(|name, _| running.contains(name));
        for (name, server) in wanted {
            if self.workers.contains_key(&name) {
                continue;
            }
            let wake: Arc<dyn Fn() + Send + Sync> = {
                let ctx = ctx.clone();
                Arc::new(move || ctx.request_repaint())
            };
            self.status.insert(name.clone(), ServerStatus::Starting);
            let worker = spawn_worker(server, events.clone(), wake);
            self.workers.insert(name, worker);
        }
        self.started = true;
    }

    /// Asks `server` to run `tool`: the ticket its result comes back with, or `None` when the server is not running.
    fn call(&mut self, server: &str, tool: &str, arguments: Json) -> Option<u64> {
        let worker = self.workers.get(server)?;
        self.next_ticket += 1;
        let ticket = self.next_ticket;
        worker
            .commands
            .send(Command::Call {
                ticket,
                tool: tool.to_string(),
                arguments,
            })
            .ok()?;
        Some(ticket)
    }

    /// What the servers said since the last frame: their state is kept, the results of the calls come back.
    fn poll(&mut self) -> Vec<(u64, Result<ToolOutput, String>)> {
        let Some((_, events)) = &self.channel else {
            return Vec::new();
        };
        let mut results = Vec::new();
        while let Ok(event) = events.try_recv() {
            match event {
                Event::Status { server, status } => {
                    // a server taken out of the list meanwhile is not put back
                    if self.workers.contains_key(&server) {
                        self.status.insert(server, status);
                    }
                }
                Event::Result { ticket, result } => results.push((ticket, result)),
            }
        }
        results
    }

    /// Whether a server is still starting.
    fn any_starting(&self) -> bool {
        self.status
            .values()
            .any(|s| matches!(s, ServerStatus::Starting))
    }
}

/// The thread of `server`: it starts it, says its tools (or why it has none), and runs the calls it is given until the
/// window lets go of it.
fn spawn_worker(
    server: Server,
    events: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> Worker {
    let (commands, received) = crossbeam_channel::unbounded::<Command>();
    let given = server.clone();
    std::thread::spawn(move || {
        let tell = |event: Event| {
            let _ = events.send(event);
            wake();
        };
        let name = given.name.clone();
        let started = Client::start(&given).and_then(|mut client| {
            let tools = client.tools()?;
            Ok((client, tools))
        });
        let (mut client, why) = match started {
            Ok((client, tools)) => {
                tell(Event::Status {
                    server: name.clone(),
                    status: ServerStatus::Ready(tools),
                });
                (Some(client), String::new())
            }
            Err(why) => {
                tell(Event::Status {
                    server: name.clone(),
                    status: ServerStatus::Failed(why.clone()),
                });
                (None, why)
            }
        };
        for command in received {
            match command {
                Command::Call {
                    ticket,
                    tool,
                    arguments,
                } => {
                    let result = match client.as_mut() {
                        Some(client) => client.call(&tool, &arguments),
                        None => Err(format!("{name} did not start: {why}")),
                    };
                    tell(Event::Result { ticket, result });
                }
            }
        }
        // the window let go of the server: dropping the client ends its program
        drop(client.take());
    });
    Worker { server, commands }
}

/// The name a model knows a tool by: the server's and the tool's, in the letters a function's name may have.
fn tool_name(server: &str, tool: &str) -> String {
    let joined = format!("{}_{tool}", mcp::tool_prefix(server));
    joined
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

/// `text` cut to `most` characters, with an ellipsis when it was longer.
fn cut(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let mut kept: String = text.chars().take(most).collect();
    kept.push('…');
    kept
}

impl App {
    /// The person's MCP servers, as text, whatever the engine: what llama.cpp's field keeps.
    pub(super) fn mcp_text(&self) -> String {
        if self.engine == crate::settings::DEFAULT_ENGINE {
            self.values
                .text("mcp_servers")
                .unwrap_or_default()
                .to_string()
        } else {
            self.stored.mcp_servers_text()
        }
    }

    /// The servers of the list (none when its text cannot be read).
    pub(super) fn mcp_list(&self) -> Vec<Server> {
        mcp::parse(&self.mcp_text()).unwrap_or_default()
    }

    /// Keeps a new list, and runs what it says when the servers run already.
    pub(super) fn set_mcp_text(&mut self, text: String, ctx: &egui::Context) {
        if self.engine == crate::settings::DEFAULT_ENGINE {
            self.values.set("mcp_servers", text);
            self.save_settings();
        } else {
            let mut stored = self.settings_now();
            stored.set_mcp_servers_text(&text);
            self.write_settings(&stored);
            self.stored = stored;
        }
        if self.tools.started {
            self.start_tools(ctx);
        }
    }

    /// Switches one server of the list on or off.
    pub(super) fn set_server_on(&mut self, name: &str, on: bool, ctx: &egui::Context) {
        let mut servers = self.mcp_list();
        if let Some(server) = servers.iter_mut().find(|s| s.name == name) {
            server.disabled = !on;
        }
        self.set_mcp_text(mcp::to_text(&servers), ctx);
    }

    /// Runs the servers of the list that are on.
    pub(super) fn start_tools(&mut self, ctx: &egui::Context) {
        let servers = self.mcp_list();
        self.tools.sync(&servers, ctx);
    }

    /// What the person said of a tool.
    pub(super) fn tool_permission(&self, server: &str, tool: &str) -> Permission {
        match self
            .stored
            .tool_permissions()
            .get(&format!("{server}.{tool}"))
            .map(String::as_str)
        {
            Some("always") => Permission::Always,
            Some("off") => Permission::Off,
            _ => Permission::Ask,
        }
    }

    /// Keeps what the person said of a tool.
    pub(super) fn set_tool_permission(&mut self, server: &str, tool: &str, permission: Permission) {
        let mut stored = self.settings_now();
        let mut all = stored.tool_permissions();
        let key = format!("{server}.{tool}");
        match permission.word() {
            Some(word) => {
                all.insert(key, word.to_string());
            }
            None => {
                all.remove(&key);
            }
        }
        stored.set_tool_permissions(&all);
        self.write_settings(&stored);
        self.stored = stored;
    }

    /// The tools the chat offers the model now, as the request gives them (`None` when there are none), and the name the
    /// model knows each by, to its server and tool.
    pub(super) fn offered_tools(&self) -> (Option<Json>, BTreeMap<String, (String, String)>) {
        let mut names = BTreeMap::new();
        let mut offered = Vec::new();
        for (server, status) in &self.tools.status {
            let ServerStatus::Ready(tools) = status else {
                continue;
            };
            for tool in tools {
                if self.tool_permission(server, &tool.name) == Permission::Off {
                    continue;
                }
                let mut name = tool_name(server, &tool.name);
                while names.contains_key(&name) {
                    name.push('_');
                }
                offered.push(json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": cut(&tool.description, MOST_DESCRIPTION),
                        "parameters": tool.schema,
                    },
                }));
                names.insert(name, (server.clone(), tool.name.clone()));
            }
        }
        ((!offered.is_empty()).then_some(Json::Array(offered)), names)
    }

    /// Whether the chat should wait for servers that are still starting before it asks for a reply.
    pub(super) fn tools_starting(&self) -> bool {
        self.tools.any_starting()
    }

    /// Each frame: the servers start once the chat can be used (or the page of the servers was opened), the results of
    /// the calls go to the chat, and the model is asked to go on once each call of its last reply is settled.
    pub(super) fn poll_tools(&mut self, ctx: &egui::Context) {
        if !self.tools.started
            && self.can_chat()
            && self.chat_running()
            && self.mcp_list().iter().any(|s| !s.disabled)
        {
            self.start_tools(ctx);
        }
        for (ticket, result) in self.tools.poll() {
            let found = self
                .chat
                .turns
                .iter_mut()
                .flat_map(|t| t.calls.iter_mut())
                .find(|c| c.ticket == Some(ticket));
            if let Some(call) = found {
                call.state = match result {
                    Ok(output) if output.is_error => CallState::Failed(output.text),
                    Ok(output) => CallState::Done(output.text),
                    Err(why) => CallState::Failed(why),
                };
            }
        }
        self.go_on_after_tools(ctx);
        // a message that waits for the servers to start is sent once they did (or could not)
        if self.chat.waiting_for_tools && !self.tools.any_starting() {
            self.chat.waiting_for_tools = false;
            self.ask_for_reply(ctx);
        }
    }

    /// The calls of the reply that just ended: those the person allowed always run now, the others wait for them.
    pub(super) fn settle_new_calls(&mut self) {
        let Some(at) = self.chat.turns.len().checked_sub(1) else {
            return;
        };
        let count = self.chat.turns[at].calls.len();
        if count == 0 || self.chat.turns[at].from_person {
            return;
        }
        self.chat.tool_rounds += 1;
        self.chat.tools_waiting = true;
        for i in 0..count {
            let (server, tool, state) = {
                let call = &self.chat.turns[at].calls[i];
                (call.server.clone(), call.tool.clone(), call.state.clone())
            };
            if state != CallState::Asking {
                continue;
            }
            if server.is_empty() {
                self.chat.turns[at].calls[i].state =
                    CallState::Failed("the model asked for a tool it was not offered".to_string());
                continue;
            }
            match self.tool_permission(&server, &tool) {
                Permission::Always => self.run_call(at, i),
                Permission::Off => {
                    self.chat.turns[at].calls[i].state = CallState::Denied;
                }
                Permission::Ask => {}
            }
        }
    }

    /// The person allowed a call (`always`: and every call of that tool from now on).
    pub(super) fn allow_call(&mut self, turn: usize, call: usize, always: bool) {
        let Some(asked) = self
            .chat
            .turns
            .get(turn)
            .and_then(|t| t.calls.get(call))
            .cloned()
        else {
            return;
        };
        if asked.state != CallState::Asking {
            return;
        }
        if always {
            self.set_tool_permission(&asked.server, &asked.tool, Permission::Always);
        }
        self.run_call(turn, call);
    }

    /// The person did not allow a call: the model is told so.
    pub(super) fn deny_call(&mut self, turn: usize, call: usize) {
        if let Some(asked) = self
            .chat
            .turns
            .get_mut(turn)
            .and_then(|t| t.calls.get_mut(call))
        {
            if asked.state == CallState::Asking {
                asked.state = CallState::Denied;
            }
        }
    }

    /// Runs a call on its server.
    fn run_call(&mut self, turn: usize, call: usize) {
        let Some(asked) = self
            .chat
            .turns
            .get(turn)
            .and_then(|t| t.calls.get(call))
            .cloned()
        else {
            return;
        };
        let arguments: Json = serde_json::from_str(&asked.arguments).unwrap_or_else(|_| json!({}));
        let ticket = self.tools.call(&asked.server, &asked.tool, arguments);
        let state = match ticket {
            Some(_) => CallState::Running,
            None => CallState::Failed(format!("{} is not running", asked.server)),
        };
        if let Some(c) = self
            .chat
            .turns
            .get_mut(turn)
            .and_then(|t| t.calls.get_mut(call))
        {
            c.ticket = ticket;
            c.state = state;
        }
    }

    /// The model is asked to go on once each call of its last reply is settled (ran, failed or was not allowed).
    fn go_on_after_tools(&mut self, ctx: &egui::Context) {
        if !self.chat.tools_waiting || self.chat.replying() {
            return;
        }
        let settled = self
            .chat
            .turns
            .last()
            .is_some_and(|t| !t.from_person && t.calls.iter().all(ToolCall::settled));
        if !settled {
            return;
        }
        self.chat.tools_waiting = false;
        if self.chat.tool_rounds >= MOST_TOOL_ROUNDS {
            self.chat.error = Some(format!(
                "The model asked for tools {MOST_TOOL_ROUNDS} times in a row; the chat stopped there. Send a message to go on."
            ));
            return;
        }
        self.ask_for_reply(ctx);
    }

    // ---- what is drawn ------------------------------------------------------------------------------------

    /// The Tools button of the chat's box: how many servers are on, and a menu that switches each on or off.
    pub(super) fn show_tools_button(&mut self, ui: &mut egui::Ui) {
        let servers = self.mcp_list();
        let on = servers.iter().filter(|s| !s.disabled).count();
        let label = if on > 0 {
            format!("Tools · {on}")
        } else {
            "Tools".to_string()
        };
        let button = theme::button_sized(ui, Role::Quiet, true, label, egui::vec2(0.0, 26.0))
            .on_hover_text(
                "The tools the model can use in this chat: the MCP servers that are on.",
            );
        let mut switch: Option<(String, bool)> = None;
        let mut manage = false;
        egui::Popup::menu(&button)
            .align(egui::RectAlign::TOP_START)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(300.0)
            .show(|ui| {
                ui.set_width(300.0);
                ui.label(theme::muted("Tools the model can use in this chat").small());
                if servers.is_empty() {
                    ui.label(theme::muted("No MCP servers yet."));
                }
                for server in &servers {
                    let mut ticked = !server.disabled;
                    let said = match self.tools.status.get(&server.name) {
                        _ if server.disabled => "off".to_string(),
                        Some(ServerStatus::Ready(tools)) => match tools.len() {
                            1 => "1 tool · asks before it runs".to_string(),
                            n => format!("{n} tools · asks before it runs"),
                        },
                        Some(ServerStatus::Starting) => "starting…".to_string(),
                        Some(ServerStatus::Failed(why)) => {
                            format!("did not start: {}", cut(why, 60))
                        }
                        None => "starts with the chat".to_string(),
                    };
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut ticked, server.name.as_str()).changed() {
                            switch = Some((server.name.clone(), ticked));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(26.0);
                        ui.label(theme::muted(said).small());
                    });
                }
                ui.separator();
                if icons::menu_item_keyed(ui, Some(Icon::Wrench), "Manage MCP servers…", "")
                    .clicked()
                {
                    manage = true;
                    ui.close();
                }
            });
        if let Some((name, on)) = switch {
            self.set_server_on(&name, on, ui.ctx());
        }
        if manage {
            self.open_mcp_servers();
        }
    }

    /// The page of the MCP servers (MCP servers in the sidebar): the list and the ways to change it, then what each
    /// server that is on offers, with what the person said of each of its tools.
    pub(super) fn show_mcp_page(&mut self, ui: &mut egui::Ui) {
        if !self.tools.started && self.mcp_list().iter().any(|s| !s.disabled) {
            self.start_tools(ui.ctx());
        }
        let p = theme::palette();
        super::settings_window::page_head(
            ui,
            "MCP servers",
            Some(
                "Tools the model can use when you chat in QuickAI, with llama.cpp or Ollama, and with a model on \
                 another computer too: QuickAI runs them on this computer, and asks you before a tool runs unless you \
                 chose Always. llama.cpp's own chat page (Open Chat UI) gets them as well.",
            ),
        );
        ui.hyperlink_to(
            "How to add and use them",
            "https://github.com/everybodyai/quickai/blob/main/docs/mcp-servers.md",
        );
        ui.add_space(6.0);
        let text = self.mcp_text();
        let reply = super::settings_window::group(ui, |ui| {
            super::mcp_servers::editor_text(ui, "app", &text)
        });
        if let Some(new) = reply {
            self.set_mcp_text(new, ui.ctx());
        }
        let servers: Vec<Server> = self
            .mcp_list()
            .into_iter()
            .filter(|s| !s.disabled)
            .collect();
        if !servers.is_empty() {
            ui.add_space(14.0);
            ui.label(
                egui::RichText::new("WHAT THEY OFFER")
                    .size(11.5)
                    .family(theme::semibold(ui))
                    .color(p.muted),
            );
            ui.label(
                theme::muted(
                    "Ask: QuickAI asks each time the model wants the tool. Always: it runs without asking. Off: the \
                     model is not offered it.",
                )
                .small(),
            );
            ui.add_space(4.0);
        }
        let mut changed: Option<(String, String, Permission)> = None;
        for server in &servers {
            super::settings_window::group(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&server.name)
                            .family(theme::semibold(ui))
                            .color(p.text),
                    );
                    match self.tools.status.get(&server.name) {
                        Some(ServerStatus::Ready(tools)) => {
                            theme::dot(ui, 8.0, 3.5, p.ok);
                            ui.label(theme::muted(match tools.len() {
                                1 => "Ready · 1 tool".to_string(),
                                n => format!("Ready · {n} tools"),
                            }));
                        }
                        Some(ServerStatus::Starting) | None => {
                            theme::dot(ui, 8.0, 3.5, p.warn);
                            ui.label(theme::muted("Starting…"));
                        }
                        Some(ServerStatus::Failed(_)) => {
                            theme::dot(ui, 8.0, 3.5, p.bad);
                            ui.label(egui::RichText::new("Did not start").color(p.bad));
                        }
                    }
                });
                ui.label(
                    egui::RichText::new(server.summary())
                        .monospace()
                        .small()
                        .color(p.muted),
                );
                match self.tools.status.get(&server.name) {
                    Some(ServerStatus::Failed(why)) => {
                        ui.add(
                            egui::Label::new(egui::RichText::new(why).small().color(p.bad)).wrap(),
                        );
                    }
                    Some(ServerStatus::Ready(tools)) => {
                        egui::CollapsingHeader::new(format!("Its tools ({})", tools.len()))
                            .id_salt(("server_tools", &server.name))
                            .show(ui, |ui| {
                                for tool in tools {
                                    let now = self.tool_permission(&server.name, &tool.name);
                                    ui.horizontal(|ui| {
                                        let name =
                                            ui.label(egui::RichText::new(&tool.name).monospace());
                                        if !tool.description.is_empty() {
                                            name.on_hover_text(cut(&tool.description, 600));
                                        }
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                for (permission, word) in [
                                                    (Permission::Off, "Off"),
                                                    (Permission::Always, "Always"),
                                                    (Permission::Ask, "Ask"),
                                                ] {
                                                    if ui
                                                        .selectable_label(now == permission, word)
                                                        .clicked()
                                                        && now != permission
                                                    {
                                                        changed = Some((
                                                            server.name.clone(),
                                                            tool.name.clone(),
                                                            permission,
                                                        ));
                                                    }
                                                }
                                            },
                                        );
                                    });
                                }
                            });
                    }
                    _ => {}
                }
            });
            ui.add_space(6.0);
        }
        if let Some((server, tool, permission)) = changed {
            self.set_tool_permission(&server, &tool, permission);
        }
        ui.add_space(8.0);
        for line in [
            "A server that is a command runs on this computer, as you, and can do whatever its program does: add only ones \
             you trust. Commands such as npx and uvx need Node.js or uv.",
            "Apps you connect to the model do not get these tools: they bring their own.",
        ] {
            ui.add(egui::Label::new(egui::RichText::new(line).small().color(p.muted)).wrap());
        }
        if self.runs_alone() {
            ui.add_space(4.0);
            ui.colored_label(
                p.warn,
                format!(
                    "{} has no chat, so it does not use them.",
                    self.engine_name()
                ),
            );
        }
    }
}

/// What can be done with a call of the chat, from its card.
#[derive(Clone, Copy)]
pub enum CardAction {
    Allow,
    Always,
    Deny,
    Toggle,
}

/// A call the model asked for, as the chat shows it: the server and the tool, what came of it, and while it waits for the
/// person, what it would be given and the buttons that say yes or no.
pub fn tool_card(
    ui: &mut egui::Ui,
    call: &ToolCall,
    model: &str,
    width: f32,
) -> Option<CardAction> {
    let p = theme::palette();
    let mut action = None;
    let asking = call.state == CallState::Asking;
    egui::Frame::new()
        .fill(p.card)
        .stroke(egui::Stroke::new(1.0, if asking { p.warn } else { p.line }))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            // its parts one under the other, whatever the row it is drawn in
            ui.vertical(|ui| {
            ui.set_width(width.min(560.0));
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                icons::paint(ui.painter(), rect, Icon::Wrench, p.muted);
                ui.label(egui::RichText::new(&call.server).family(theme::semibold(ui)).color(p.text));
                ui.label(egui::RichText::new(format!("· {}", call.tool)).color(p.body));
                ui.add_space(10.0);
                match &call.state {
                    CallState::Asking => {
                        ui.label(egui::RichText::new("Waiting for you").color(p.warn).small());
                    }
                    CallState::Running => {
                        ui.add(egui::Spinner::new().size(12.0));
                        ui.label(theme::muted("Running…").small());
                    }
                    CallState::Done(_) => {
                        ui.label(egui::RichText::new("Used").color(p.ok).small());
                        if ui
                            .small_button(if call.open { "Hide" } else { "Show" })
                            .on_hover_text("What it was given and what it gave back.")
                            .clicked()
                        {
                            action = Some(CardAction::Toggle);
                        }
                    }
                    CallState::Failed(_) => {
                        ui.label(egui::RichText::new("Failed").color(p.bad).small());
                    }
                    CallState::Denied => {
                        ui.label(theme::muted("Not allowed").small());
                    }
                }
            });
            let arguments = pretty(&call.arguments);
            match &call.state {
                CallState::Asking => {
                    ui.label(theme::muted(format!("{model} wants to use this tool, with:")).small());
                    theme::well_frame(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(cut(&arguments, 800)).monospace().small()).wrap());
                    });
                    ui.horizontal(|ui| {
                        if theme::button_sized(ui, Role::Primary, true, "Allow once", egui::vec2(0.0, 26.0))
                            .clicked()
                        {
                            action = Some(CardAction::Allow);
                        }
                        if theme::button_sized(ui, Role::Normal, true, "Always allow", egui::vec2(0.0, 26.0))
                            .on_hover_text("Run this tool without asking from now on (the page of the MCP servers changes it back).")
                            .clicked()
                        {
                            action = Some(CardAction::Always);
                        }
                        if theme::button_sized(ui, Role::Quiet, true, "Deny", egui::vec2(0.0, 26.0))
                            .on_hover_text("Do not run it: the model is told so.")
                            .clicked()
                        {
                            action = Some(CardAction::Deny);
                        }
                    });
                    ui.label(theme::muted("It runs on this computer, as you.").small());
                }
                CallState::Failed(why) => {
                    ui.add(egui::Label::new(egui::RichText::new(cut(why, 600)).small().color(p.bad)).wrap());
                }
                CallState::Done(result) if call.open => {
                    theme::well_frame(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(egui::RichText::new(cut(&arguments, 800)).monospace().small()).wrap());
                    });
                    theme::well_frame(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(egui::RichText::new(cut(result, 2000)).monospace().small()).wrap());
                    });
                }
                _ => {}
            }
            });
        });
    action
}

/// The arguments of a call, laid out to be read; as they came when they are not JSON.
fn pretty(arguments: &str) -> String {
    serde_json::from_str::<Json>(arguments)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| arguments.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_is_named_by_its_server_and_itself_in_the_letters_a_function_may_have() {
        assert_eq!(tool_name("files", "read_text_file"), "files_read_text_file");
        assert_eq!(tool_name("my server", "do.it"), "my_server_do_it");
        assert_eq!(tool_name("a", &"x".repeat(80)).len(), 64);
    }

    #[test]
    fn a_long_text_is_cut_with_an_ellipsis() {
        assert_eq!(cut("short", 10), "short");
        assert_eq!(cut("abcdef", 3), "abc…");
    }
}
