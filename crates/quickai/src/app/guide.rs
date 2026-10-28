//! The guides (UI v2, C178): what the main window said in paragraphs ("Getting started") is a menu of its own under the
//! ? at the right of the menu bar. Getting started is three steps that tick themselves as they are done (an engine
//! that is ready, a model, a first reply in the chat); each guide is a few short steps in a window of its own, which
//! the menu and the Help menu open.

use eframe::egui;

use super::icons::{self, Icon};
use super::theme;
use super::App;

/// One guide of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Guide {
    GettingStarted,
    FindModel,
    Chat,
    ConnectApp,
    Remote,
    Engines,
    Shortcuts,
}

impl Guide {
    /// Every guide, in the order of the menu.
    pub(super) const ALL: [Guide; 7] = [
        Guide::GettingStarted,
        Guide::FindModel,
        Guide::Chat,
        Guide::ConnectApp,
        Guide::Remote,
        Guide::Engines,
        Guide::Shortcuts,
    ];

    pub(super) fn title(self) -> &'static str {
        match self {
            Guide::GettingStarted => "Getting started",
            Guide::FindModel => "Find the right model",
            Guide::Chat => "Chat, pictures and files",
            Guide::ConnectApp => "Connect an app",
            Guide::Remote => "Use a more powerful computer",
            Guide::Engines => "Switch engines",
            Guide::Shortcuts => "Keyboard shortcuts",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Guide::GettingStarted => Icon::Check,
            Guide::FindModel => Icon::Search,
            Guide::Chat => Icon::Chat,
            Guide::ConnectApp => Icon::Send,
            Guide::Remote => Icon::Computer,
            Guide::Engines => Icon::Sliders,
            Guide::Shortcuts => Icon::More,
        }
    }

    /// The steps of the guide, each a short sentence.
    pub(super) fn steps(self) -> &'static [&'static str] {
        match self {
            Guide::GettingStarted => &[
                "Choose an engine at the top right. llama.cpp is ready to use; Ollama and ComfyUI are installed with \
                 one click.",
                "Add a model with the button at the top of the list. The suggestions suit this computer.",
                "Press the green play button beside the model's name, then type a message in the Chat tab.",
            ],
            Guide::FindModel => &[
                "Press Add a model. With the box empty, it suggests models that fit this computer's memory.",
                                "Type words to search online (Hugging Face for llama.cpp, ollama.com for Ollama), or press Enter in the \
                 empty box to see the most popular ones.",
                "Paste a link to a .gguf file to download it, or press the folder to add a file you already have.",
                "Smaller files (Q4) are faster and need less memory. Bigger ones (Q8) are a little better.",
                "Recommended, in the Models menu, has the whole list QuickAI checked, by kind and size.",
            ],
            Guide::Chat => &[
                "Start the model, then type in the box under Chat. Enter sends; Shift+Enter starts a new line.",
                "The paper clip adds a picture, for a model that sees them, or a text file to read.",
                "Under ... beside New chat: keep chats on this computer, set instructions and how free the answers \
                 are, or save a chat as a file.",
                "Nothing you type leaves this computer, unless the model runs on another one you added.",
            ],
            Guide::ConnectApp => &[
                "While a model runs, press the plug button beside its name (Connect an app).",
                "Copy the address into any app that works with OpenAI's API, with the model's name. Any API key \
                 works.",
                "The globe button beside it opens a chat in your browser, when the engine has one.",
            ],
            Guide::Remote => &[
                "Install Ollama on the other computer, start it there, and check that you can reach that computer \
                 with ssh from here.",
                "Click This computer at the bottom right, then Manage computers, and add it: its SSH address \
                 (user@host, or a Host of your SSH config). QuickAI tests it and says what to do about anything that fails.",
                "Choose it in the same menu. Start then connects to it.",
                "Nothing is opened to the network: QuickAI uses your own ssh and keys.",
            ],
            Guide::Engines => &[
                "The menu at the top right chooses the engine that runs the models.",
                "llama.cpp runs .gguf files from a folder. Ollama pulls models by name. ComfyUI makes pictures.",
                "Each engine keeps its own models and settings. One that is not installed has an Install button.",
            ],
            Guide::Shortcuts => &[
                "Ctrl+Enter: start or stop the model.",
                "Ctrl+K: add a model.",
                "Ctrl+L: go to the chat's box.",
                "Ctrl+,: Settings.",
                "Enter sends a message; Shift+Enter starts a new line.",
                "Escape closes a window.",
            ],
        }
    }
}

impl App {
    /// Opens the window of the guides on `guide`.
    pub(super) fn open_guide(&mut self, guide: Guide) {
        self.guide = Some(guide);
    }

    /// The three steps of Getting started, each with whether it is done.
    pub(super) fn getting_started(&self) -> [(&'static str, bool); 3] {
        let engine_ready = !self.needs_install();
        [
            ("Choose an engine", engine_ready),
            (
                "Add a model",
                engine_ready && (!self.models.is_empty() || self.runs_alone()),
            ),
            (
                "Start it and say hello",
                self.stored.said_hello() || self.chat.turns.iter().any(|t| !t.from_person),
            ),
        ]
    }

    /// The menu of the ? button: the steps of Getting started, the guides, and the documentation.
    pub(super) fn show_guide_menu(&mut self, ui: &mut egui::Ui) {
        let steps = self.getting_started();
        let done = steps.iter().filter(|(_, d)| *d).count();
        ui.horizontal(|ui| {
            ui.label(small_heading(ui, "GETTING STARTED"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(theme::muted(format!("{done} of {}", steps.len())).small());
            });
        });
        let mut open: Option<Guide> = None;
        for (words, done) in steps {
            if menu_row(ui, if done { Some(Icon::Check) } else { None }, words, done).clicked() {
                open = Some(Guide::GettingStarted);
            }
        }
        ui.separator();
        ui.label(small_heading(ui, "GUIDES"));
        for guide in Guide::ALL.into_iter().skip(1) {
            if menu_row(ui, Some(guide.icon()), guide.title(), false).clicked() {
                open = Some(guide);
            }
        }
        ui.separator();
        if menu_row(ui, Some(Icon::Help), "Documentation", false).clicked() {
            let _ = webbrowser::open(super::shell::DOCS_URL);
        }
        if let Some(guide) = open {
            self.open_guide(guide);
        }
    }

    /// The window of the guides: the list on the left, the steps of the one chosen on the right.
    pub(super) fn show_guide_window(&mut self, ctx: &egui::Context) {
        let Some(mut current) = self.guide else {
            return;
        };
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.guide = None;
            return;
        }
        let mut open = true;
        let steps = self.getting_started();
        egui::Window::new("Guides")
            .id(egui::Id::new("guides"))
            .open(&mut open)
            .collapsible(false)
            // as tall as what it holds; only its width can be changed
            .resizable([true, false])
            .default_width(620.0)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(200.0);
                        for guide in Guide::ALL {
                            if ui
                                .selectable_label(current == guide, guide.title())
                                .clicked()
                            {
                                current = guide;
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(current.title())
                                .size(17.0)
                                .family(theme::semibold(ui))
                                .color(theme::palette().text),
                        );
                        ui.add_space(6.0);
                        for (i, step) in current.steps().iter().enumerate() {
                            ui.horizontal_top(|ui| {
                                let ticked = current == Guide::GettingStarted
                                    && steps.get(i).is_some_and(|(_, d)| *d);
                                let mark = if ticked {
                                    egui::RichText::new("✓").color(theme::palette().ok)
                                } else if current == Guide::Shortcuts {
                                    egui::RichText::new("·").color(theme::palette().muted)
                                } else {
                                    egui::RichText::new(format!("{}.", i + 1))
                                        .color(theme::palette().muted)
                                };
                                ui.add_sized([18.0, 18.0], egui::Label::new(mark));
                                ui.add(egui::Label::new(*step).wrap());
                            });
                            ui.add_space(4.0);
                        }
                    });
                });
            });
        self.guide = open.then_some(current);
    }
}

/// The small capitals over a part of the menu.
fn small_heading(ui: &egui::Ui, text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(11.0)
        .family(theme::semibold(ui))
        .color(theme::palette().faint)
}

/// A row of the menu: a picture (or the room of one), its words, and the whole row to click.
fn menu_row(ui: &mut egui::Ui, icon: Option<Icon>, text: &str, done: bool) -> egui::Response {
    let p = theme::palette();
    let width = ui.available_width().clamp(230.0, 280.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter().rect_filled(rect, 4.0, p.raised_hover);
        }
        let square = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 6.0, rect.center().y - 7.5),
            egui::vec2(15.0, 15.0),
        );
        match icon {
            Some(icon) => icons::paint(
                ui.painter(),
                square,
                icon,
                if done { p.ok } else { p.muted },
            ),
            None => {
                ui.painter()
                    .circle_stroke(square.center(), 6.5, egui::Stroke::new(1.4, p.outline));
            }
        }
        let font = egui::TextStyle::Button.resolve(ui.style());
        let color = if done { p.muted } else { p.text };
        let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
        ui.painter().galley(
            egui::pos2(
                square.right() + 10.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }
    response
}
