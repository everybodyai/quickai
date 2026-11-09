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

/// The count in the corner of the Tools button: the servers that are on, and whether one of them did not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Badge {
    pub count: usize,
    pub failed: bool,
}

/// Draws `badge` over the bottom right of `button`, a small round of the accent (of a warning when a server did not
/// start) with the number in it, ringed in the box's colour so that it stands off the wrench under it.
fn paint_badge(ui: &egui::Ui, button: egui::Rect, badge: &Badge) {
    let p = theme::palette();
    let (fill, ink) = if badge.failed {
        (p.warn, p.page)
    } else {
        (p.accent, p.on_accent)
    };
    let text = if badge.count > 9 {
        "9+".to_string()
    } else {
        badge.count.to_string()
    };
    let font = egui::FontId::new(9.5, theme::semibold(ui));
    let galley = ui.painter().layout_no_wrap(text, font, ink);
    let high = 13.0;
    let size = egui::vec2((galley.size().x + 6.0).max(high), high);
    let rect = egui::Rect::from_center_size(
        egui::pos2(
            button.right() - size.x / 2.0,
            button.bottom() - size.y / 2.0,
        ),
        size,
    );
    let painter = ui.painter();
    painter.rect(
        rect,
        high / 2.0,
        fill,
        egui::Stroke::new(1.5, p.well),
        egui::StrokeKind::Outside,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, ink);
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

    /// The tools of the chat's box: a wrench beside the clip, with how many servers are on in its corner (in the
    /// colour of a warning when one of them did not start), and a menu that switches each on or off.
    pub(super) fn show_tools_button(&mut self, ui: &mut egui::Ui) {
        let servers = self.mcp_list();
        let badge = self.tools_badge(&servers);
        let button = icons::icon_button(ui, Icon::Wrench, egui::vec2(30.0, 28.0), true, false);
        if let Some(badge) = &badge {
            paint_badge(ui, button.rect, badge);
        }
        let button = button.on_hover_text(self.tools_hover(&servers));
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

    /// What the corner of the Tools button says: nothing while no server is on, else how many are, and whether
    /// one of them did not start.
    pub(super) fn tools_badge(&self, servers: &[Server]) -> Option<Badge> {
        let on: Vec<&Server> = servers.iter().filter(|s| !s.disabled).collect();
        if on.is_empty() {
            return None;
        }
        let failed = on.iter().any(|s| {
            matches!(
                self.tools.status.get(&s.name),
                Some(ServerStatus::Failed(_))
            )
        });
        Some(Badge {
            count: on.len(),
            failed,
        })
    }

    /// What the Tools button says when the pointer rests on it: the servers that are on, with how many tools each
    /// has, or that none is.
    fn tools_hover(&self, servers: &[Server]) -> String {
        let on: Vec<String> = servers
            .iter()
            .filter(|s| !s.disabled)
            .map(|s| match self.tools.status.get(&s.name) {
                Some(ServerStatus::Ready(tools)) => match tools.len() {
                    1 => format!("{} (1 tool)", s.name),
                    n => format!("{} ({n} tools)", s.name),
                },
                Some(ServerStatus::Failed(_)) => format!("{} (did not start)", s.name),
                Some(ServerStatus::Starting) => format!("{} (starting)", s.name),
                None => s.name.clone(),
            })
            .collect();
        match on.len() {
            0 => "Tools: no MCP server is on. Click to choose the ones the model can use in this chat.".to_string(),
            1 => format!("Tools: 1 MCP server on, {}. Click to switch it off or choose others.", on[0]),
            n => format!(
                "Tools: {n} MCP servers on, {}. Click to switch them on or off.",
                on.join(", ")
            ),
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
    #[cfg(unix)]
    use std::path::Path;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    use quickai_core::value::Value;

    use crate::config::{build_registry, ConfigDirs};
    use crate::events::ServerState;
    use crate::settings::Settings;
    #[cfg(unix)]
    use crate::testkit::fake_ollama::{FakeOllama, ToolsBehave};
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;

    /// A program that speaks MCP on its input and output: `say` gives back the words it is given, louder; `count`
    /// counts their letters. In Python, which the computer that runs the tests may not have: `None` then.
    #[cfg(unix)]
    fn echo_server(dir: &Path) -> Option<Server> {
        let python = ["python3", "python"].into_iter().find(|p| {
            std::process::Command::new(p)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        })?;
        let script = dir.join("echo_mcp.py");
        std::fs::write(
            &script,
            r#"import json, sys
for line in sys.stdin:
    m = json.loads(line)
    if "id" not in m:
        continue
    if m["method"] == "initialize":
        r = {"protocolVersion": "2025-03-26", "capabilities": {"tools": {}}, "serverInfo": {"name": "echo"}}
    elif m["method"] == "tools/list":
        r = {"tools": [
            {"name": "say", "description": "Says the words back, louder", "inputSchema": {"type": "object", "properties": {"words": {"type": "string"}}}},
            {"name": "count", "description": "Counts letters", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}}]}
    elif m["method"] == "tools/call":
        a = m["params"]["arguments"]
        if m["params"]["name"] == "say":
            r = {"content": [{"type": "text", "text": a.get("words", "").upper() + "!"}]}
        else:
            r = {"content": [{"type": "text", "text": str(len(a.get("text", "")))}]}
    else:
        r = {}
    print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": r}), flush=True)
"#,
        )
        .unwrap();
        Some(Server {
            name: "echo".into(),
            reach: mcp::Reach::Command {
                command: python.into(),
                args: vec![script.display().to_string()],
                env: BTreeMap::new(),
            },
            disabled: false,
        })
    }

    /// An application with `engine`, its settings in `dir`, the MCP servers given (and, for Ollama, attached to the
    /// fake on `ollama_port`).
    fn app_with(dir: &Path, engine: &str, ollama_port: Option<u16>, servers: &[Server]) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine(engine);
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        if let Some(port) = ollama_port {
            let registry = build_registry(&ConfigDirs::default());
            let values: BTreeMap<String, Value> = [
                ("ollama.port".to_string(), Value::Int(port as i64)),
                ("ollama.mode".to_string(), Value::Str("attach".into())),
            ]
            .into_iter()
            .collect();
            settings
                .set_engine_values(registry.engine("ollama").unwrap(), &values)
                .unwrap();
        }
        settings.set_mcp_servers_text(&mcp::to_text(servers));
        settings.save_to(&dir.join("settings.json"));
        let mut app = App::build(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
        );
        app.data_dir_override = Some(dir.join("data").display().to_string());
        app.load_engine_state();
        app
    }

    /// Starts Ollama as Start does, attached to the fake, with `model` loaded.
    #[cfg(unix)]
    fn start_with(app: &mut App, model: &str) {
        let selection = crate::launch::selection_from_values(
            "ollama",
            Some(quickai_core::plan::ModelInfo {
                id: model.to_string(),
                display: model.to_string(),
                path: String::new(),
                size_bytes: 0,
            }),
            app.values.map(),
            false,
        );
        let plan = crate::launch::plan_selection(
            &app.registry,
            &selection,
            Path::new("/nowhere/ollama"),
            app.machine.clone(),
            None,
        )
        .unwrap_or_else(|e| panic!("{e:#?}"));
        app.session.start(&plan, model, || {});
    }

    /// What a frame does for the server, the chat and the tools.
    #[cfg(unix)]
    fn frame(app: &mut App, ctx: &egui::Context) {
        let mut said = Vec::new();
        app.session.poll(&mut |line| said.push(line), &mut |_| {});
        for line in said {
            app.push_log(line);
        }
        app.poll_named_models(ctx);
        app.poll_chat();
        app.poll_tools(ctx);
    }

    #[cfg(unix)]
    fn until(app: &mut App, ctx: &egui::Context, what: &str, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !done(app) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}; chat: {:#?}, error: {:?}, tools: {:?}",
                app.chat.turns,
                app.chat.error,
                app.tools.status
            );
            frame(app, ctx);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The last call of the conversation.
    fn last_call(app: &App) -> Option<&ToolCall> {
        app.chat.turns.iter().rev().find_map(|t| t.calls.last())
    }

    /// The text of the last reply, when it has ended.
    fn last_reply(app: &App) -> Option<&str> {
        app.chat
            .turns
            .last()
            .filter(|t| !t.from_person && !app.chat.replying())
            .map(|t| t.text.as_str())
    }

    /// What the whole window draws, as text, at the width the suites use.
    fn painted(app: &mut App, ctx: &egui::Context) -> String {
        fn walk(shape: &egui::epaint::Shape, into: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, into)),
                egui::epaint::Shape::Text(t) => into.push(t.galley.text().to_string()),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for _ in 0..3 {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(980.0, 680.0),
                )),
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw, |ui| app.show_window(ui));
            output.textures_delta.clear();
            texts.clear();
            for clipped in &output.shapes {
                walk(&clipped.shape, &mut texts);
            }
        }
        texts.join("\n")
    }

    /// I46 (C204): with Ollama, the chat offers the tools of the MCP servers QuickAI runs, named by their server; a call
    /// is a card that waits for the person; allowed, the tool runs on its server and the model is told what it gave back,
    /// and answers with it. Always runs it without asking; Off takes it out of what is offered; Deny tells the model.
    #[cfg(unix)]
    #[test]
    fn p3_i46_with_ollama_a_tool_is_asked_for_allowed_run_and_answered_with() {
        let dir = scratch("u05");
        let Some(echo) = echo_server(&dir) else {
            return; // no Python on this computer to run the MCP server with
        };
        let fake = FakeOllama::start();
        fake.add_model("qwen3:4b", 2 * GB);
        fake.behave(|b| b.tools = ToolsBehave::Call);
        let mut app = app_with(&dir, "ollama", Some(fake.port()), &[echo]);
        let ctx = egui::Context::default();
        start_with(&mut app, "qwen3:4b");
        until(&mut app, &ctx, "Ollama to run", |a| {
            a.session.state() == ServerState::Running
        });

        // the first message starts the server, and waits for it
        app.chat.input = "hello".into();
        app.send_chat(&ctx);
        until(&mut app, &ctx, "the card that asks", |a| {
            last_call(a).is_some_and(|c| c.state == CallState::Asking)
        });
        let call = last_call(&app).unwrap().clone();
        assert_eq!((call.server.as_str(), call.tool.as_str()), ("echo", "say"));
        assert_eq!(call.arguments, "{\"words\":\"hello\"}");
        let asked = fake.requests_to("/v1/chat/completions");
        let tools = asked[0].body.as_ref().unwrap()["tools"].clone();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["echo_say", "echo_count"]);
        // nothing runs until the person says so
        std::thread::sleep(Duration::from_millis(200));
        frame(&mut app, &ctx);
        assert_eq!(last_call(&app).unwrap().state, CallState::Asking);
        assert_eq!(fake.requests_to("/v1/chat/completions").len(), 1);

        // allowed once: it runs, and the model answers with what it gave back
        let at = app.chat.turns.len() - 1;
        app.allow_call(at, 0, false);
        until(
            &mut app,
            &ctx,
            "the reply with what the tool gave back",
            |a| last_reply(a).is_some_and(|t| t.contains("HELLO!")),
        );
        assert_eq!(
            app.chat.turns[at].calls[0].state,
            CallState::Done("HELLO!".into())
        );
        let second = fake.requests_to("/v1/chat/completions")[1]
            .body
            .clone()
            .unwrap();
        let messages = second["messages"].as_array().unwrap();
        assert_eq!(
            messages[messages.len() - 2]["tool_calls"][0]["id"],
            "call_fake_1"
        );
        assert_eq!(messages[messages.len() - 1]["role"], "tool");
        assert_eq!(messages[messages.len() - 1]["content"], "HELLO!");
        assert_eq!(
            app.tool_permission("echo", "say"),
            Permission::Ask,
            "once is once"
        );

        // always: the next call runs without asking
        app.set_tool_permission("echo", "say", Permission::Always);
        app.chat.input = "again".into();
        app.send_chat(&ctx);
        until(&mut app, &ctx, "the reply, without asking", |a| {
            a.chat.turns.len() > at + 3 && last_reply(a).is_some_and(|t| t.contains("HELLO!"))
        });

        // denied: the model is told so, and goes on
        app.set_tool_permission("echo", "say", Permission::Ask);
        app.chat.input = "once more".into();
        app.send_chat(&ctx);
        until(&mut app, &ctx, "the card", |a| {
            last_call(a).is_some_and(|c| c.state == CallState::Asking)
        });
        let at = app.chat.turns.len() - 1;
        app.deny_call(at, 0);
        until(&mut app, &ctx, "the reply after a no", |a| {
            last_reply(a).is_some_and(|t| t.contains("did not allow"))
        });

        // off: what is off is not offered, and with every tool off the request has none
        app.set_tool_permission("echo", "say", Permission::Off);
        app.set_tool_permission("echo", "count", Permission::Off);
        let before = fake.requests_to("/v1/chat/completions").len();
        app.chat.input = "no tools".into();
        app.send_chat(&ctx);
        until(&mut app, &ctx, "a plain reply", |a| {
            last_reply(a).is_some_and(|t| t.starts_with("Hello!"))
        });
        let plain = fake.requests_to("/v1/chat/completions")[before]
            .body
            .clone()
            .unwrap();
        assert!(plain.get("tools").is_none(), "{plain}");
        app.session.stop_blocking();
    }

    /// I47 (C204): an engine that refuses the tools (Ollama, for a model whose template has none) is asked again without
    /// them, and the chat says so under the conversation.
    #[cfg(unix)]
    #[test]
    fn p3_i47_an_engine_that_refuses_the_tools_is_asked_again_without_them() {
        let dir = scratch("u06");
        let Some(echo) = echo_server(&dir) else {
            return;
        };
        let fake = FakeOllama::start();
        fake.add_model("gemma3:270m", GB);
        fake.behave(|b| b.tools = ToolsBehave::Refuse);
        let mut app = app_with(&dir, "ollama", Some(fake.port()), &[echo]);
        let ctx = egui::Context::default();
        start_with(&mut app, "gemma3:270m");
        until(&mut app, &ctx, "Ollama to run", |a| {
            a.session.state() == ServerState::Running
        });
        app.chat.input = "hello".into();
        app.send_chat(&ctx);
        until(&mut app, &ctx, "the reply without tools", |a| {
            last_reply(a).is_some_and(|t| t.starts_with("Hello!"))
        });
        let notice = app.chat.notice.clone().expect("the chat says so");
        assert!(
            notice.contains("did not take the tools") && notice.contains("does not support tools"),
            "{notice}"
        );
        assert!(app.chat.error.is_none(), "{:?}", app.chat.error);
        let asked = fake.requests_to("/v1/chat/completions");
        assert_eq!(asked.len(), 2);
        assert!(asked[1].body.as_ref().unwrap().get("tools").is_none());
        app.session.stop_blocking();
    }

    /// I49 (C204): the MCP servers are one list, whatever the engine: what is added with Ollama is llama.cpp's field (its
    /// chat page has them too), and a server switched off in the chat's menu is off there.
    #[test]
    fn p3_i49_the_mcp_servers_are_one_list_whatever_the_engine() {
        let dir = scratch("u08");
        let mut app = app_with(&dir, "ollama", None, &[]);
        let ctx = egui::Context::default();
        assert!(app.mcp_list().is_empty());
        let wiki = Server {
            name: "wiki".into(),
            reach: mcp::Reach::Url {
                url: "https://mcp.deepwiki.com/mcp".into(),
                headers: BTreeMap::new(),
            },
            disabled: false,
        };
        app.set_mcp_text(mcp::to_text(std::slice::from_ref(&wiki)), &ctx);
        assert_eq!(app.mcp_list(), std::slice::from_ref(&wiki));
        assert_eq!(app.mcp_server_count(), Some(1));
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert!(
            file["values"]["llamacpp.mcp_servers"]
                .as_str()
                .is_some_and(|t| t.contains("mcp.deepwiki.com")),
            "{file}"
        );
        // llama.cpp, chosen, has it in its field
        app.switch_engine("llamacpp");
        assert_eq!(app.engine, "llamacpp");
        assert_eq!(app.mcp_list(), std::slice::from_ref(&wiki));
        assert!(app
            .values
            .text("mcp_servers")
            .is_some_and(|t| t.contains("mcp.deepwiki.com")));
        // switched off (the chat's menu), it is kept and off, and counted out
        app.set_server_on("wiki", false, &ctx);
        assert!(app.mcp_list()[0].disabled);
        assert_eq!(app.mcp_server_count(), Some(0));
    }

    /// I50 (C204): what the person said of a tool is kept: a window started again knows it.
    #[test]
    fn p3_i50_what_the_person_said_of_a_tool_is_kept() {
        let dir = scratch("u09");
        let mut app = app_with(&dir, "llamacpp", None, &[]);
        assert_eq!(app.tool_permission("files", "write_file"), Permission::Ask);
        app.set_tool_permission("files", "write_file", Permission::Off);
        app.set_tool_permission("files", "read_text_file", Permission::Always);
        let again = App::build(
            Settings::load_from(&dir.join("settings.json")),
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
        );
        assert_eq!(
            again.tool_permission("files", "write_file"),
            Permission::Off
        );
        assert_eq!(
            again.tool_permission("files", "read_text_file"),
            Permission::Always
        );
        assert_eq!(
            again.tool_permission("files", "list_directory"),
            Permission::Ask
        );
        // and Ask again takes it out of what is kept
        app.set_tool_permission("files", "write_file", Permission::Ask);
        assert!(!app
            .stored
            .tool_permissions()
            .contains_key("files.write_file"));
    }

    /// I51 (C203, C204): while the model runs, the panel at the right is Connect an app (with Ollama's own address), and
    /// the chat's box has Tools with how many servers are on, whose menu lists them.
    #[cfg(unix)]
    #[test]
    fn p3_i51_while_it_runs_the_panel_is_connect_an_app_and_the_box_has_tools() {
        let dir = scratch("u10");
        let Some(echo) = echo_server(&dir) else {
            return;
        };
        let fake = FakeOllama::start();
        fake.add_model("qwen3:4b", 2 * GB);
        let mut app = app_with(&dir, "ollama", Some(fake.port()), &[echo]);
        // Ollama's program is there (a file is all the window looks for): the page is the model's, not the installer
        std::fs::write(dir.join("ollama"), "").unwrap();
        app.binary = dir.join("ollama");
        app.models = vec![crate::models::ModelEntry {
            filename: "qwen3:4b".into(),
            path: std::path::PathBuf::new(),
            display: "qwen3:4b".into(),
            note: String::new(),
            bundled: false,
            size_bytes: 2 * GB,
            companions: Vec::new(),
            mode: None,
        }];
        app.selected = 0;
        let ctx = egui::Context::default();
        let stopped = painted(&mut app, &ctx);
        assert!(stopped.contains("How it runs"), "{stopped}");
        start_with(&mut app, "qwen3:4b");
        until(&mut app, &ctx, "Ollama to run", |a| {
            a.session.state() == ServerState::Running
        });
        until(&mut app, &ctx, "the server to be ready", |a| {
            matches!(a.tools.status.get("echo"), Some(ServerStatus::Ready(_)))
        });
        let running = painted(&mut app, &ctx);
        for text in [
            "Connect an app",
            "Address",
            "Ollama API",
            "MCP servers",
            "1 on",
        ] {
            assert!(running.contains(text), "{text}: {running}");
        }
        // the wrench of the chat's box has the one server that is on in its corner
        let servers = app.mcp_list();
        assert_eq!(
            app.tools_badge(&servers),
            Some(Badge {
                count: 1,
                failed: false
            })
        );
        app.session.stop_blocking();
    }

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
