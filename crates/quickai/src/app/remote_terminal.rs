//! The Terminal tab under the page (C228): a shell on the computer QuickAI sets up that is shown, beside the log, when
//! Settings › Other computers has it on (it is off unless the person turns it on). The shell is reached as the runs are,
//! by the person's own `ssh` with their keys and `~/.ssh/config`, and never asks a question (BatchMode). It is a shell
//! reading commands, not a terminal screen: what a command writes comes back line by line, and a program that draws on
//! the screen (top, vim) or asks for a password does not work. One shell a computer, kept while the window is open;
//! Restart ends it (and what runs in it) and opens another.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use eframe::egui;

use crate::remote::{ssh, RemoteSpec};

use super::icons::{self, Icon};
use super::theme;
use super::App;

/// The lines kept of what a shell said: the oldest go first.
const MOST_LINES: usize = 4000;

/// What a shell there said, as its readers pass it on.
enum Said {
    Line(String),
    /// Its output ended: the shell, or the connection, is gone.
    Ended,
}

/// A line of the tab: what the person typed (`typed`), or what came back.
struct Line {
    text: String,
    typed: bool,
}

/// A shell on a computer QuickAI sets up, over ssh.
pub struct Terminal {
    lines: VecDeque<Line>,
    input: String,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    said: Receiver<Said>,
    /// What was typed, the latest last, for Up and Down.
    history: Vec<String>,
    /// Where Up and Down are in the history (`history.len()`: past its end, the line being typed).
    back: usize,
    /// The shell has ended.
    ended: bool,
}

impl Terminal {
    /// Opens a shell on `spec` with the ssh program `program`; `windows` when that computer is Windows (its shell is
    /// `cmd`). `wake` is called when a line comes.
    fn open(
        program: &Path,
        spec: &RemoteSpec,
        windows: bool,
        wake: impl Fn() + Send + Sync + Clone + 'static,
    ) -> Terminal {
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut terminal = Terminal {
            lines: VecDeque::new(),
            input: String::new(),
            child: None,
            stdin: None,
            said: rx,
            history: Vec::new(),
            back: 0,
            ended: false,
        };
        if let Some(problem) = spec.problem() {
            terminal.say(problem);
            terminal.ended = true;
            return terminal;
        }
        // the shell reads the commands from what is typed here, and what they write to either stream comes back in order
        let command = if windows { "cmd /Q" } else { "exec sh -s 2>&1" };
        let spawned = Command::new(program)
            .args(ssh::command_args(spec, command))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                terminal.say(format!("Could not start ssh ({}): {e}", program.display()));
                terminal.ended = true;
                return terminal;
            }
        };
        terminal.stdin = child.stdin.take();
        if let Some(out) = child.stdout.take() {
            read_lines(out, tx.clone(), wake.clone(), true);
        }
        // what ssh itself says (a connection refused, a key not taken)
        if let Some(err) = child.stderr.take() {
            read_lines(err, tx, wake, false);
        }
        terminal.child = Some(child);
        terminal.say(format!(
            "A shell on {} ({}). Type a command and press Enter.",
            spec.name, spec.host
        ));
        terminal
    }

    /// A line of the window's own, among what came back.
    fn say(&mut self, text: impl Into<String>) {
        self.push(Line {
            text: text.into(),
            typed: false,
        });
    }

    fn push(&mut self, line: Line) {
        if self.lines.len() >= MOST_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    /// Takes what the shell said since the last frame.
    fn take(&mut self, computer: &str) {
        loop {
            match self.said.try_recv() {
                Ok(Said::Line(text)) => self.say(text),
                Ok(Said::Ended) => {
                    if !self.ended {
                        self.ended = true;
                        self.stdin = None;
                        self.say(format!(
                            "The shell on {computer} ended. Restart opens another."
                        ));
                    }
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
    }

    /// Sends what was typed to the shell.
    fn send(&mut self) {
        let line = std::mem::take(&mut self.input);
        self.push(Line {
            text: format!("$ {line}"),
            typed: true,
        });
        if !line.trim().is_empty() && self.history.last() != Some(&line) {
            self.history.push(line.clone());
        }
        self.back = self.history.len();
        let sent = self.stdin.as_mut().map(|stdin| {
            stdin
                .write_all(format!("{line}\n").as_bytes())
                .and_then(|()| stdin.flush())
        });
        if !matches!(sent, Some(Ok(()))) {
            self.say("The shell is not there to take it. Restart opens another.");
        }
    }

    /// Up (`older`) or Down in what was typed.
    fn browse(&mut self, older: bool) {
        if self.history.is_empty() {
            return;
        }
        if older {
            self.back = self.back.saturating_sub(1);
        } else if self.back < self.history.len() {
            self.back += 1;
        }
        self.input = self.history.get(self.back).cloned().unwrap_or_default();
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.stdin = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Passes on each line `from` writes, on a thread of its own; `last` says the end when it ends.
fn read_lines<R: Read + Send + 'static>(
    from: R,
    to: Sender<Said>,
    wake: impl Fn() + Send + 'static,
    last: bool,
) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(from);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let text = text.trim_end_matches(['\n', '\r']).replace('\t', "    ");
                    if to.send(Said::Line(text)).is_err() {
                        return;
                    }
                    wake();
                }
            }
        }
        if last {
            let _ = to.send(Said::Ended);
            wake();
        }
    });
}

impl App {
    /// Whether the dock has the Terminal tab: Settings has it on, and a computer QuickAI sets up is shown.
    pub(super) fn terminal_shown(&self) -> bool {
        self.stored.remote_terminal() && self.setup_in_use().is_some()
    }

    /// What the Terminal tab says when the pointer rests on it.
    pub(super) fn terminal_hover(&self) -> String {
        let name = self
            .setup_in_use()
            .map_or_else(|| "that computer".to_string(), |s| s.name.clone());
        format!(
            "Commands on {name}, over SSH with your own login. Programs that draw on the screen (top, vim) do not work \
             here."
        )
    }

    /// The Terminal tab: what the shell there said, and the line to type a command in, with Clear and Restart.
    pub(super) fn show_terminal_tab(&mut self, ui: &mut egui::Ui) {
        let Some(spec) = self.setup_in_use().cloned() else {
            return;
        };
        let p = theme::palette();
        let windows = self
            .facts_there()
            .is_some_and(|f| f.system.to_ascii_lowercase().starts_with("windows"));
        if !self.remote.there.terminals.contains_key(&spec.id) {
            let ctx = ui.ctx().clone();
            let terminal = Terminal::open(&self.remote.ssh, &spec, windows, move || {
                ctx.request_repaint()
            });
            self.remote
                .there
                .terminals
                .insert(spec.id.clone(), terminal);
        }
        let mut restart = false;
        egui::Frame::new()
            .fill(p.log)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let Some(terminal) = self.remote.there.terminals.get_mut(&spec.id) else {
                    return;
                };
                terminal.take(&spec.name);
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let row = ui.fonts_mut(|f| f.row_height(&font));
                // the line to type in at the foot, what came back over it
                egui::Panel::bottom(egui::Id::new(("terminal_input", &spec.id)))
                    .frame(egui::Frame::new().inner_margin(egui::Margin {
                        left: 0,
                        right: 0,
                        top: 6,
                        bottom: 0,
                    }))
                    .show_separator_line(false)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{} $", spec.name))
                                    .monospace()
                                    .color(p.muted),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                restart = icons::icon_button(
                                    ui,
                                    Icon::Refresh,
                                    egui::vec2(26.0, 24.0),
                                    true,
                                    false,
                                )
                                .on_hover_text(
                                    "Restart: end this shell, and what runs in it, and open another.",
                                )
                                .clicked();
                                if ui
                                    .small_button("Clear")
                                    .on_hover_text("Clear what is shown. The shell goes on.")
                                    .clicked()
                                {
                                    terminal.lines.clear();
                                }
                                let field = ui.add_enabled(
                                    !terminal.ended,
                                    egui::TextEdit::singleline(&mut terminal.input)
                                        .id(egui::Id::new(("terminal_line", &spec.id)))
                                        .font(egui::TextStyle::Monospace)
                                        .hint_text("a command, then Enter")
                                        .desired_width(ui.available_width()),
                                );
                                if field.has_focus() {
                                    let (up, down) = ui.input(|i| {
                                        (
                                            i.key_pressed(egui::Key::ArrowUp),
                                            i.key_pressed(egui::Key::ArrowDown),
                                        )
                                    });
                                    if up || down {
                                        terminal.browse(up);
                                    }
                                }
                                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    terminal.send();
                                    field.request_focus();
                                } else if !terminal.ended
                                    && ui.ctx().memory(|m| m.focused().is_none())
                                {
                                    // the line takes the keys as soon as the tab shows, unless another field has them
                                    field.request_focus();
                                }
                            });
                        });
                    });
                egui::ScrollArea::both()
                    .id_salt(("terminal_lines", &spec.id))
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show_rows(ui, row, terminal.lines.len(), |ui, range| {
                        for line in terminal.lines.range(range) {
                            let color = if line.typed { p.accent_text } else { p.body };
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&line.text).monospace().color(color),
                                )
                                .extend(),
                            );
                        }
                    });
            });
        if restart {
            // the old one is dropped, and with it its ssh
            self.remote.there.terminals.remove(&spec.id);
        }
    }
}
