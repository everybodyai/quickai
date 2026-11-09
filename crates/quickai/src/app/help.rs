//! Help (C191): a window of its own, which **Help** at the foot of the sidebar (C206) and F1 open.
//! Its pages are in a sidebar under a search box and Getting started; each is a title, a line on what it is for, a
//! line on what is so now, a picture of the part of QuickAI it is about with numbered marks on it, and its steps under
//! the picture with the same numbers. A step that names a button can **Show me** it: the button is ringed in the main
//! window for a moment, and the main window comes to the front. A step that can be done for the person has a button
//! that does it. **Take the tour** goes round the main window one part at a time.
//!
//! The pictures are real screenshots of QuickAI, in the dark theme and the light, that `a script kept outside this repository`
//! takes; the marks are drawn on them here, in their pixels. They are in the program, so Help needs no network.
//!
//! The words are written for a person who has never run a model: short sentences, one action to a step, a button
//! by the name the window shows it by, in bold.

use std::collections::HashMap;
use std::time::Instant;

use eframe::egui;
use egui::text::{LayoutJob, TextFormat};
use egui::Color32;

use super::icons::{self, Icon};
use super::theme::{self, Role, Tone};
use super::App;
use crate::events::ServerState;

/// What the window is called to the window manager: its title bar, and what a taskbar lists.
pub(super) const HELP_TITLE: &str = "QuickAI Help";

/// The size the window opens at, and the least it can be made. A page that is taller than the window scrolls.
const OPENS_AT: [f32; 2] = [900.0, 700.0];
const LEAST: [f32; 2] = [600.0, 400.0];
/// The sidebar's width, and its width in a window narrower than `NARROW`, where the page keeps room for its pictures.
const SIDEBAR: f32 = 240.0;
const SIDEBAR_NARROW: f32 = 210.0;
const NARROW: f32 = 760.0;
/// The widest a page's column is: wider, a line is too long to read.
const COLUMN: f32 = 660.0;
/// How long Show me rings a part of the main window.
const SHOW_FOR: f32 = 4.0;

/// Which window Help is: the id the display knows it by.
pub(super) fn help_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("quickai_help")
}

/// The pages, in the order of the sidebar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum HelpPage {
    #[default]
    GettingStarted,
    FindModel,
    Chat,
    ConnectApp,
    Pictures,
    Remote,
    Tools,
    Engines,
    Configurations,
    Settings,
    WontStart,
    Slow,
    Files,
    Privacy,
    Shortcuts,
}

/// The headings of the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Start,
    Using,
    Further,
    Wrong,
    Know,
}

impl Group {
    fn heading(self) -> &'static str {
        match self {
            Group::Start => "Start here",
            Group::Using => "Using it",
            Group::Further => "Going further",
            Group::Wrong => "Something's wrong",
            Group::Know => "Good to know",
        }
    }
}

/// A part of the main window that Help can point at. The main window says where each is as it draws it (`mark_spot`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Spot {
    EngineMenu,
    AddModel,
    Play,
    Plug,
    Change,
    ChatBox,
    Clip,
    ChatMenu,
    LogTab,
    Computer,
    Settings,
    Help,
}

impl Spot {
    /// Whether it is in the sidebar, which can be put away.
    fn in_sidebar(self) -> bool {
        matches!(
            self,
            Spot::EngineMenu | Spot::AddModel | Spot::Computer | Spot::Settings | Spot::Help
        )
    }
}

/// What a step's button does for the person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Deed {
    AddModel,
    Recommended,
    Settings,
    Configurations,
    Log,
    ModelsFolder,
    Privacy,
    /// The page of the MCP servers.
    Tools,
}

impl Deed {
    fn label(self) -> &'static str {
        match self {
            Deed::AddModel => "Open Add a model",
            Deed::Recommended => "Open Recommended",
            Deed::Settings => "Open Settings",
            Deed::Configurations => "Open Configurations",
            Deed::Log => "Show the log",
            Deed::ModelsFolder => "Open the folder",
            Deed::Privacy => "Read the privacy policy",
            Deed::Tools => "Open MCP servers",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Deed::AddModel => Icon::Plus,
            Deed::Recommended => Icon::Bulb,
            Deed::Settings => Icon::Gear,
            Deed::Configurations => Icon::Save,
            Deed::Log => Icon::Document,
            Deed::ModelsFolder => Icon::Folder,
            Deed::Privacy => Icon::Lock,
            Deed::Tools => Icon::Wrench,
        }
    }
}

/// A step of a page: a sentence (`**bold**` for a name the window shows, `{play}` for a picture of that button), the
/// part of the main window it is about, and what its button does.
struct Step {
    text: &'static str,
    spot: Option<Spot>,
    deed: Option<Deed>,
}

const fn step(text: &'static str) -> Step {
    Step {
        text,
        spot: None,
        deed: None,
    }
}

impl Step {
    const fn at(self, spot: Spot) -> Step {
        Step {
            spot: Some(spot),
            ..self
        }
    }

    const fn does(self, deed: Deed) -> Step {
        Step {
            deed: Some(deed),
            ..self
        }
    }
}

/// A picture of QuickAI: its name in `assets/help/<theme>/`, the numbers drawn on it (where, in its pixels), and the
/// part of it ringed.
struct Picture {
    name: &'static str,
    marks: &'static [(u8, f32, f32)],
    ring: Option<[f32; 4]>,
}

/// A page: where it is in the sidebar, its words, its picture and its steps.
struct Page {
    page: HelpPage,
    group: Group,
    icon: Icon,
    title: &'static str,
    lead: &'static str,
    picture: Option<Picture>,
    steps: &'static [Step],
    /// Steps that are not a sequence (things to know, keys) have no numbers.
    numbered: bool,
    tip: Option<&'static str>,
}

const PAGES: &[Page] = &[
    Page {
        page: HelpPage::GettingStarted,
        group: Group::Start,
        icon: Icon::Check,
        title: "Getting started",
        lead: "Three steps to a first answer from a model that runs on this computer.",
        picture: Some(Picture {
            name: "main",
            marks: &[(1, 165.0, 124.0), (2, 188.0, 257.0), (3, 703.0, 28.0)],
            ring: None,
        }),
        steps: &[
            step(
                "Choose an **engine** under **This computer**, at the top of the sidebar. llama.cpp is ready to use; \
                 Ollama and ComfyUI install with one click.",
            )
            .at(Spot::EngineMenu),
            step("**Add a model** with the {plus} beside **MODELS**. The suggestions suit this computer.")
                .at(Spot::AddModel)
                .does(Deed::AddModel),
            step("Press **Start** in the bar over the page, then type a message in the box at the bottom.")
                .at(Spot::Play),
        ],
        numbered: true,
        tip: Some("New here? **Take the tour**: two minutes, one part of the window at a time."),
    },
    Page {
        page: HelpPage::FindModel,
        group: Group::Using,
        icon: Icon::Search,
        title: "Find the right model",
        lead: "Which model fits this computer, and how to get it.",
        picture: Some(Picture {
            name: "add-model",
            marks: &[(2, 618.0, 177.0), (3, 284.0, 28.0), (4, 660.0, 28.0), (5, 394.0, 369.0)],
            ring: Some([681.0, 165.0, 24.0, 24.0]),
        }),
        steps: &[
            step("Press {plus} beside **MODELS**, then **Add a model…**, or Ctrl+K.")
                .at(Spot::AddModel)
                .does(Deed::AddModel),
            step(
                "With the box empty, it suggests models that fit this computer's memory. Press {download} beside one \
                 to get it.",
            ),
            step(
                "Type words to search online: Hugging Face for llama.cpp, ollama.com for Ollama. Enter in the empty \
                 box shows the most popular.",
            ),
            step("Paste a link to a .gguf file to download it, or press the folder {folder} to add a file you have."),
            step(
                "Or find one yourself: **Hugging Face** under the list opens its page of models in the browser. Paste \
                 the link of the one you choose in the box.",
            ),
            step("**Recommended** in the sidebar has the whole list QuickAI checked, by kind and size.")
                .does(Deed::Recommended),
        ],
        numbered: true,
        tip: Some(
            "Smaller files (Q4 in the name) are faster and need less memory. Bigger ones (Q8) answer a little better.",
        ),
    },
    Page {
        page: HelpPage::Chat,
        group: Group::Using,
        icon: Icon::Chat,
        title: "Chat, pictures and files",
        lead: "Talk to the model that runs, and give it a picture or a document to read.",
        picture: Some(Picture {
            name: "chat",
            marks: &[(2, 420.0, 647.0), (3, 37.0, 612.0), (4, 450.0, 229.0), (5, 714.0, 52.0)],
            ring: None,
        }),
        steps: &[
            step("Start the model with **Start**, in the bar over the page.").at(Spot::Play),
            step("Type in the box at the bottom. **Enter** sends; **Shift+Enter** starts a new line.").at(Spot::ChatBox),
            step("The paper clip {clip} adds a picture, for a model that sees them, or a text file to read.")
                .at(Spot::Clip),
            step("Under each answer: **Copy**, **Regenerate**, and how fast it came."),
            step(
                "The {more} at the right end of the bar: keep chats on this computer, set instructions and how free \
                 the answers are, or save a chat as a file. The {pencil} beside it starts a new chat.",
            )
            .at(Spot::ChatMenu),
        ],
        numbered: true,
        tip: Some("What you type stays on this computer, unless the model runs on another one you added."),
    },
    Page {
        page: HelpPage::ConnectApp,
        group: Group::Using,
        icon: Icon::Plug,
        title: "Connect an app",
        lead: "Use your model from other programs: a code editor, a notes app or a script of your own. Anything that \
               works with OpenAI's API can talk to it.",
        picture: Some(Picture {
            name: "connect",
            marks: &[(1, 233.0, 66.0), (2, 85.0, 162.0), (3, 75.0, 230.0), (4, 165.0, 568.0)],
            ring: Some([218.0, 12.0, 32.0, 32.0]),
        }),
        steps: &[
            step(
                "Start the model: the panel at the right of the page becomes **Connect an app**. The {hide} at its \
                 top puts it away, and the {show} at the right end of the bar brings it back.",
            )
            .at(Spot::Plug),
            step("Copy the **Address** into the other app. It may call it the base URL or the endpoint."),
            step("Copy the **Model** name too. Type anything as the **API key**, or leave it blank."),
            step("**Open Chat UI** {globe} under it opens a chat in your browser, when the engine has one."),
        ],
        numbered: true,
        tip: Some(
            "The address works on this computer only: other computers on your network cannot reach it. The panel \
             has examples for curl, Python and JavaScript.",
        ),
    },
    Page {
        page: HelpPage::Pictures,
        group: Group::Using,
        icon: Icon::Image,
        title: "Make pictures",
        lead: "ComfyUI makes pictures and video from workflows you build in its own page.",
        picture: Some(Picture {
            name: "engine-menu",
            marks: &[(1, 165.0, 68.0), (2, 140.0, 205.0)],
            ring: None,
        }),
        steps: &[
            step("Open the engine menu under **This computer**, at the top of the sidebar.").at(Spot::EngineMenu),
            step(
                "Choose **ComfyUI**. The first time, it installs with one click: about 17 GB, most of it the \
                 library it computes with.",
            ),
            step("Get a model: **Recommended** lists the ones QuickAI checked, **Look up online** searches for more."),
            step("Press **Start**, then **Open ComfyUI**: its page opens in your browser."),
            step("The **Download** button of that page puts a model straight into QuickAI's models folder."),
        ],
        numbered: true,
        tip: Some("ComfyUI runs on this computer and, unless you change it, works offline."),
    },
    Page {
        page: HelpPage::Remote,
        group: Group::Further,
        icon: Icon::Computer,
        title: "A more powerful computer",
        lead: "Run big models on another computer of yours, and use them from here.",
        picture: Some(Picture {
            name: "computer-menu",
            marks: &[(2, 180.0, 126.0), (3, 175.0, 63.0)],
            ring: None,
        }),
        steps: &[
            step("Check that you can reach the other computer with ssh from here, with your own key."),
            step(
                "Click **This computer** at the top of the sidebar, then **Add a computer…**, and add it \
                 by its SSH address (user@host, or a Host of your SSH config). QuickAI tests it and says what to do about \
                 anything that fails.",
            )
            .at(Spot::Computer),
            step(
                "Choose it in the same menu. **Start** then runs the model there: QuickAI copies itself there and \
                 installs what is missing, or uses an engine that already runs there.",
            ),
        ],
        numbered: true,
        tip: Some("Nothing is opened to the network: QuickAI uses your own ssh and keys."),
    },
    Page {
        page: HelpPage::Tools,
        group: Group::Further,
        icon: Icon::Plug,
        title: "Give your model tools",
        lead: "MCP servers let a model read your files, fetch a web page and more. QuickAI runs them on this \
               computer and offers their tools when you chat, with llama.cpp or Ollama, and with a model on another \
               computer too.",
        picture: None,
        steps: &[
            step(
                "Press **MCP servers** in the sidebar. Give the server a name and the command or address from its \
                 instructions, then press **Add**.",
            )
            .does(Deed::Tools),
            step(
                "Start a model and chat. **Tools** in the chat's box says which servers the model may use, and switches \
                 each on or off.",
            ),
            step(
                "When the model wants a tool, a card shows what it would be given: press **Allow once**, **Always \
                 allow** or **Deny**. The model answers with what the tool gave back.",
            ),
            step(
                "Under **What they offer** on the same page, each tool can be set to **Ask**, **Always** or **Off**. \
                 With llama.cpp, its own chat page (**Open Chat UI**) has the servers too.",
            ),
        ],
        numbered: true,
        tip: Some(
            "A server can do whatever its program does on this computer, so add only ones you trust. One that does not \
             start says why on the page of the MCP servers.",
        ),
    },
    Page {
        page: HelpPage::Engines,
        group: Group::Further,
        icon: Icon::Sliders,
        title: "Switch engines",
        lead: "The engine is the program that runs the models. Each is good at something.",
        picture: Some(Picture {
            name: "engine-menu",
            marks: &[(1, 165.0, 68.0)],
            ring: None,
        }),
        steps: &[
            step("The engine is chosen under **This computer**, at the top of the sidebar.").at(Spot::EngineMenu),
            step("**llama.cpp** runs .gguf files from a folder. It comes with QuickAI."),
            step("**Ollama** gets models by name and keeps them itself."),
            step("**ComfyUI** makes pictures and video."),
            step("Each engine keeps its own models and settings. One that is not installed has an **Install** button."),
        ],
        numbered: true,
        tip: None,
    },
    Page {
        page: HelpPage::Configurations,
        group: Group::Further,
        icon: Icon::Save,
        title: "Save and share a setup",
        lead: "A configuration is a model and how it runs, kept under a name, to use again or to give to someone.",
        picture: Some(Picture {
            name: "change",
            marks: &[(1, 233.0, 64.0), (2, 140.0, 287.0), (3, 264.0, 155.0)],
            ring: None,
        }),
        steps: &[
            step("The panel at the right of the page says how the model runs. The {hide} at its top puts it away, and the {show} at the right end of the bar brings it back.")
                .at(Spot::Change),
            step(
                "Choose how much of the chat it remembers: **Light**, **Balanced** or **Long**. **Set the numbers \
                 yourself…** is for numbers of your own. QuickAI keeps your choice for the next model and the next start.",
            ),
            step(
                "**Configuration** at the top of the panel keeps how it runs under a name: **Save as a new \
                 configuration…**. Choose one from the same list to use it again.",
            ),
            step("**Configurations…** in the **QuickAI** menu has them all: use, rename or delete one, or save it as a file to share.")
                .does(Deed::Configurations),
        ],
        numbered: true,
        tip: Some("A file you are given says what it would change before anything is used."),
    },
    Page {
        page: HelpPage::Settings,
        group: Group::Further,
        icon: Icon::Gear,
        title: "What the settings do",
        lead: "The settings are the engine's. **Settings…** in the **QuickAI** menu at the top left opens them, and so does Ctrl+,.",
        picture: Some(Picture {
            name: "settings",
            marks: &[
                (1, 196.0, 85.0),
                (2, 196.0, 127.0),
                (3, 196.0, 169.0),
                (4, 196.0, 211.0),
                (5, 196.0, 253.0),
            ],
            ring: None,
        }),
        steps: &[
            step("**General**: where the models are kept, and the live traffic, which shows each request and answer.")
                .at(Spot::Settings)
                .does(Deed::Settings),
            step("**Program**: the builds of the engine's program. Use another, look up a newer one, or use your own."),
            step(
                "**Starting it**: what the program is given besides what QuickAI gives it, and the whole command \
                 line Start runs.",
            ),
            step("**MCP servers**: the servers whose tools a model may use, as on **MCP servers** in the sidebar."),
            step("**Other computers**: whether QuickAI may set up a computer you add, to run models there."),
        ],
        numbered: true,
        tip: Some("With Ollama, **Open WebUI** has a page of its own: a chat page in your browser."),
    },
    Page {
        page: HelpPage::WontStart,
        group: Group::Wrong,
        icon: Icon::Warn,
        title: "It won't start",
        lead: "What to look at when the model doesn't start, or stops by itself.",
        picture: Some(Picture {
            name: "port-taken",
            marks: &[(2, 312.0, 90.0)],
            ring: None,
        }),
        steps: &[
            step("Open the log with {log} in the bar: the engine says there why it stopped.")
                .at(Spot::LogTab)
                .does(Deed::Log),
            step(
                "**Port is taken**: another program listens there. **Use port** picks the next free one, **Stop it** \
                 stops the other program, **Override** stops it and starts QuickAI's.",
            ),
            step("**Not enough memory**: choose **Light** in the panel at the right, or a smaller model.").at(Spot::Change),
            step("**The program is missing**: the **Program** page of the Settings lists the builds. Use one.")
                .does(Deed::Settings),
        ],
        numbered: true,
        tip: Some("Still stuck? The log says what the engine said, word for word: it is what to ask about."),
    },
    Page {
        page: HelpPage::Slow,
        group: Group::Wrong,
        icon: Icon::Bolt,
        title: "Slow, or out of memory",
        lead: "A model is quick when it fits in the memory of the graphics card, or of the computer.",
        picture: Some(Picture {
            name: "change",
            marks: &[(1, 100.0, 367.0)],
            ring: None,
        }),
        steps: &[
            step("In the panel at the right, **Light** uses the least memory. It remembers less of a long chat.").at(Spot::Change),
            step(
                "A smaller model is faster: fewer billions of parameters (4B rather than 9B), or a smaller file of \
                 the same one (Q4 rather than Q8).",
            ),
            step("**Add a model** suggests what fits this computer's memory.").does(Deed::AddModel),
            step("Other programs that use the graphics card (a game, another model) leave it less room."),
        ],
        numbered: true,
        tip: None,
    },
    Page {
        page: HelpPage::Files,
        group: Group::Know,
        icon: Icon::Folder,
        title: "Where your files are kept",
        lead: "Everything QuickAI keeps is in a folder of your home, .quickai, and in its settings.",
        picture: None,
        steps: &[
            step("**Models**: .quickai/models, for llama.cpp. The **General** page of the Settings can move it.")
                .does(Deed::ModelsFolder),
            step("**Ollama, ComfyUI and Open WebUI**: each in a folder of its own beside it, with its models and builds."),
            step("**Chats**: in .quickai/chats, and only when **Keep chats** is on, under the {more} at the right end of the bar."),
            step("**Settings and configurations**: in your system's folder of settings (.config/quickai on Linux)."),
        ],
        numbered: false,
        tip: None,
    },
    Page {
        page: HelpPage::Privacy,
        group: Group::Know,
        icon: Icon::Lock,
        title: "What leaves this computer",
        lead: "Nothing, unless you ask for it.",
        picture: None,
        steps: &[
            step("What you type and the answers stay on this computer: the engine listens on this computer only."),
            step("QuickAI uses the internet when you ask it to: to search for or download a model, or an engine."),
            step("There is no account, no analytics and no update check. When something breaks, QuickAI shows a report you can post if you want; it sends nothing by itself."),
            step("A model on another computer you added gets your messages through your own ssh.")
                .does(Deed::Privacy),
        ],
        numbered: false,
        tip: None,
    },
    Page {
        page: HelpPage::Shortcuts,
        group: Group::Know,
        icon: Icon::Keyboard,
        title: "Keyboard shortcuts",
        lead: "Start, stop and chat without the mouse.",
        picture: None,
        steps: &[
            step("Ctrl+Enter|Start or stop the model"),
            step("Ctrl+K|Add a model"),
            step("Ctrl+L|Go to the chat's box"),
            step("Enter|Send a message"),
            step("Shift+Enter|A new line in a message"),
            step("Ctrl+,|Settings"),
            step("F1|Help"),
            step("Ctrl+F|Search Help, in this window"),
            step("Alt+Left|Back to the page before, in this window"),
            step("Escape|Close a window"),
        ],
        numbered: false,
        tip: None,
    },
];

/// Every page, in the order of the sidebar.
#[cfg(test)]
pub(super) fn every_page() -> Vec<HelpPage> {
    PAGES.iter().map(|p| p.page).collect()
}

fn page_of(page: HelpPage) -> &'static Page {
    PAGES
        .iter()
        .find(|p| p.page == page)
        .expect("every page is written")
}

/// A stop of the tour: the part of the main window, and what is said beside it.
const TOUR: &[(Spot, &str, &str)] = &[
    (
        Spot::EngineMenu,
        "The engine",
        "The program that runs your models. llama.cpp is ready; Ollama and ComfyUI install from here.",
    ),
    (
        Spot::AddModel,
        "Add a model",
        "The + of the list: suggestions that fit this computer, a search online, or a file you already have. Ctrl+K too.",
    ),
    (
        Spot::Play,
        "Start",
        "Starts the model chosen in the list. Ctrl+Enter does it too, and stops it.",
    ),
    (
        Spot::Change,
        "How it runs",
        "The panel at the right: Light, Balanced or Long, how much of the chat it remembers. Keep it as a configuration.",
    ),
    (
        Spot::ChatBox,
        "Chat",
        "Type here once the model runs. The paper clip adds a picture or a file.",
    ),
    (
        Spot::Computer,
        "Where it runs",
        "This computer, or a more powerful one you add.",
    ),
    (
        Spot::Help,
        "Help",
        "At the foot of the sidebar, beside About and the theme: everything is explained, with pictures. F1 opens it too.",
    ),
];

/// What Help keeps between frames.
#[derive(Default)]
pub(super) struct HelpState {
    pub open: bool,
    /// Asked for again while open: it comes to the front.
    pub raise: bool,
    pub page: HelpPage,
    /// The pages before, for Alt+Left.
    back: Vec<HelpPage>,
    search: String,
    focus_search: bool,
    /// Show me: the part of the main window that is ringed, since when.
    shown: Option<(Spot, Instant)>,
    /// The stop of the tour that is shown.
    pub tour: Option<usize>,
    /// The pictures, made into textures when a page first shows them: by name and theme.
    pictures: HashMap<(&'static str, bool), egui::TextureHandle>,
}

/// The main window says where `spot` is, as it draws it: Help reads it in the same frame.
pub(super) fn mark_spot(ui: &egui::Ui, spot: Spot, rect: egui::Rect) {
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx()
        .data_mut(|d| d.insert_temp(spot_id(spot), (rect, pass)));
}

fn spot_id(spot: Spot) -> egui::Id {
    egui::Id::new(("help_spot", spot))
}

/// Where `spot` is in the main window, if the main window drew it in this frame.
pub(super) fn spot_rect(ctx: &egui::Context, spot: Spot) -> Option<egui::Rect> {
    let now = ctx.cumulative_pass_nr_for(egui::ViewportId::ROOT);
    ctx.data(|d| d.get_temp::<(egui::Rect, u64)>(spot_id(spot)))
        .filter(|(_, pass)| *pass == now)
        .map(|(rect, _)| rect)
}

macro_rules! pictures {
    ($($name:literal),* $(,)?) => {
        /// The pictures of the pages, by name, in the dark theme or the light.
        fn picture_bytes(name: &str, dark: bool) -> Option<&'static [u8]> {
            match (name, dark) {
                $(
                    ($name, true) => Some(include_bytes!(concat!("../../../../assets/help/dark/", $name, ".png"))),
                    ($name, false) => Some(include_bytes!(concat!("../../../../assets/help/light/", $name, ".png"))),
                )*
                _ => None,
            }
        }

        /// Every picture the program carries.
        #[cfg(test)]
        const PICTURE_NAMES: &[&str] = &[$($name),*];
    };
}

pictures!(
    "main",
    "add-model",
    "chat",
    "connect",
    "engine-menu",
    "computer-menu",
    "change",
    "settings",
    "port-taken",
);

impl App {
    /// The three steps of Getting started, each with whether it is done: an engine that is ready, a model, a first reply
    /// in the chat.
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

    /// Opens Help on the page it was on, or brings it to the front.
    pub(super) fn open_help(&mut self) {
        self.help.raise = self.help.open;
        self.help.open = true;
    }

    /// Opens Help on `page` (Keyboard shortcuts, from the menu of Help at the foot of the sidebar).
    pub(super) fn open_help_at(&mut self, page: HelpPage) {
        self.help_go(page);
        self.open_help();
    }

    fn help_go(&mut self, page: HelpPage) {
        if page != self.help.page {
            self.help.back.push(self.help.page);
            self.help.page = page;
        }
    }

    /// Draws Help, if it is open: a window of its own, as the Settings are.
    pub(super) fn show_help_window(&mut self, ctx: &egui::Context) {
        if !self.help.open {
            return;
        }
        let id = help_viewport();
        if std::mem::take(&mut self.help.raise) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        let mut close = false;
        let builder = theme::own_window(HELP_TITLE, OPENS_AT, LEAST);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if self.take_help_keys(ui) {
                close = true;
            }
            if class == egui::ViewportClass::EmbeddedWindow {
                // a context that cannot open a window (a test's) shows it inside the main one: the pages in a row
                // over the page
                ui.horizontal_wrapped(|ui| {
                    for page in PAGES {
                        if ui
                            .selectable_label(self.help.page == page.page, page.title)
                            .clicked()
                        {
                            self.help_go(page.page);
                        }
                    }
                });
                let room = (ui.ctx().content_rect().height() - 150.0).max(200.0);
                egui::ScrollArea::vertical()
                    .id_salt("help_scroll")
                    .max_height(room)
                    .auto_shrink([false, true])
                    .show(ui, |ui| self.show_help_page(ui));
                if ui.button("Close").clicked() {
                    close = true;
                }
            } else {
                if ui.input(|input| input.viewport().close_requested()) {
                    close = true;
                }
                egui::Panel::left("help_sidebar")
                    .exact_size(if ui.available_width() < NARROW {
                        SIDEBAR_NARROW
                    } else {
                        SIDEBAR
                    })
                    .resizable(false)
                    .frame(theme::bar_frame(egui::Margin::symmetric(8, 12)))
                    .show(ui, |ui| self.show_help_sidebar(ui));
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme::palette().page))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(("help_page", self.help.page))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let width = ui.available_width().min(COLUMN + 60.0);
                                let side = ((ui.available_width() - width) / 2.0).max(0.0);
                                ui.horizontal_top(|ui| {
                                    ui.add_space(side + 30.0);
                                    ui.vertical(|ui| {
                                        ui.set_width(width - 60.0);
                                        ui.add_space(20.0);
                                        self.show_help_page(ui);
                                        ui.add_space(24.0);
                                    });
                                });
                            });
                    });
            }
        });
        if close {
            self.help.open = false;
            self.help.search.clear();
        }
    }

    /// The keys of the window: Escape clears the search, or closes the window when it is empty; Ctrl+F goes to the
    /// search; Alt+Left goes back a page. True when the window is to close.
    fn take_help_keys(&mut self, ui: &egui::Ui) -> bool {
        let (escape, find, back) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::F),
                i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft),
            )
        });
        if find {
            self.help.focus_search = true;
        }
        if back {
            if let Some(page) = self.help.back.pop() {
                self.help.page = page;
            }
        }
        if escape && !self.help.search.is_empty() {
            self.help.search.clear();
            return false;
        }
        escape
    }

    /// The sidebar: the search, Getting started with how far it is done, the pages under their headings (those the
    /// search finds, while it has words), the tour and the documentation online.
    fn show_help_sidebar(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let search_id = egui::Id::new("help_search");
        let search = ui
            .add(
                egui::TextEdit::singleline(&mut self.help.search)
                    .id(search_id)
                    .hint_text("Search help")
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin {
                        left: 28,
                        right: 8,
                        top: 6,
                        bottom: 6,
                    }),
            )
            .on_hover_text("Words of a page or of its steps (Ctrl+F)");
        icons::paint(
            ui.painter(),
            egui::Rect::from_min_size(
                egui::pos2(search.rect.left() + 8.0, search.rect.center().y - 7.0),
                egui::vec2(14.0, 14.0),
            ),
            Icon::Search,
            p.muted,
        );
        if std::mem::take(&mut self.help.focus_search) {
            ui.memory_mut(|m| m.request_focus(search_id));
        }
        let query = self.help.search.trim().to_lowercase();
        let found: Vec<&Page> = PAGES.iter().filter(|page| matches(page, &query)).collect();
        // Enter in the box opens the first page it finds
        if search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(first) = found.first() {
                self.help_go(first.page);
            }
        }
        // the room the foot keeps: the tour and the documentation
        let foot = 68.0;
        egui::ScrollArea::vertical()
            .id_salt("help_sidebar_list")
            .max_height((ui.available_height() - foot).max(60.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                if query.is_empty() {
                    ui.add_space(8.0);
                    if self.getting_started_card(ui).clicked() {
                        self.help_go(HelpPage::GettingStarted);
                    }
                } else if found.is_empty() {
                    ui.add_space(10.0);
                    ui.label(theme::muted(format!(
                        "No page has \u{201c}{}\u{201d}.",
                        self.help.search.trim()
                    )));
                }
                let mut heading = None;
                for page in found {
                    if query.is_empty() && page.group == Group::Start {
                        continue;
                    }
                    if heading != Some(page.group) {
                        heading = Some(page.group);
                        ui.add_space(9.0);
                        ui.label(
                            egui::RichText::new(page.group.heading().to_uppercase())
                                .size(11.0)
                                .family(theme::semibold(ui))
                                .color(p.faint),
                        );
                        ui.add_space(2.0);
                    }
                    if nav_row(ui, page.icon, page.title, self.help.page == page.page).clicked() {
                        self.help_go(page.page);
                    }
                }
            });
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if nav_row(ui, Icon::Document, "Online documentation", false)
                .on_hover_text(super::shell::DOCS_URL)
                .clicked()
            {
                let _ = webbrowser::open(super::shell::DOCS_URL);
            }
            if nav_row(ui, Icon::Route, "Take the tour", false)
                .on_hover_text("Two minutes round the main window, one part at a time.")
                .clicked()
            {
                self.start_tour(ui.ctx());
            }
            ui.add_space(2.0);
            let y = ui.cursor().bottom();
            let wide = ui.max_rect();
            ui.painter().hline(
                (wide.left() - 8.0)..=(wide.right() + 8.0),
                y,
                egui::Stroke::new(1.0, p.line),
            );
        });
    }

    /// Getting started in the sidebar: a ring of how much of it is done, and its name.
    fn getting_started_card(&self, ui: &mut egui::Ui) -> egui::Response {
        let p = theme::palette();
        let steps = self.getting_started();
        let done = steps.iter().filter(|(_, d)| *d).count();
        let chosen = self.help.page == HelpPage::GettingStarted;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 50.0), egui::Sense::click());
        if ui.is_rect_visible(rect) {
            let fill = if chosen || response.hovered() {
                p.raised
            } else {
                p.card
            };
            ui.painter().rect(
                rect,
                8.0,
                fill,
                egui::Stroke::new(1.0, p.line),
                egui::StrokeKind::Inside,
            );
            let centre = egui::pos2(rect.left() + 24.0, rect.center().y);
            ui.painter()
                .circle_stroke(centre, 13.0, egui::Stroke::new(3.0, p.raised_hover));
            let share = done as f32 / steps.len() as f32;
            if share > 0.0 {
                let points: Vec<egui::Pos2> = (0..=40)
                    .map(|i| {
                        let a = -std::f32::consts::FRAC_PI_2
                            + share * std::f32::consts::TAU * i as f32 / 40.0;
                        centre + 13.0 * egui::vec2(a.cos(), a.sin())
                    })
                    .collect();
                ui.painter().add(egui::Shape::line(
                    points,
                    egui::Stroke::new(3.0, p.accent_text),
                ));
            }
            if done == steps.len() {
                icons::paint(
                    ui.painter(),
                    egui::Rect::from_center_size(centre, egui::vec2(14.0, 14.0)),
                    Icon::Check,
                    p.accent_text,
                );
            } else {
                ui.painter().text(
                    centre,
                    egui::Align2::CENTER_CENTER,
                    format!("{done}/{}", steps.len()),
                    egui::FontId::new(10.5, theme::semibold(ui)),
                    p.text,
                );
            }
            ui.painter().text(
                egui::pos2(rect.left() + 48.0, rect.center().y - 8.0),
                egui::Align2::LEFT_CENTER,
                "Getting started",
                egui::FontId::new(14.0, theme::semibold(ui)),
                p.text,
            );
            ui.painter().text(
                egui::pos2(rect.left() + 48.0, rect.center().y + 9.0),
                egui::Align2::LEFT_CENTER,
                if done == steps.len() {
                    "All done".to_string()
                } else {
                    format!("{done} of {} done", steps.len())
                },
                egui::TextStyle::Small.resolve(ui.style()),
                p.accent_text,
            );
        }
        response
    }

    /// The page that is chosen.
    fn show_help_page(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette();
        let page = page_of(self.help.page);
        if page.group != Group::Start {
            ui.label(
                egui::RichText::new(format!("{}  ›  {}", page.group.heading(), page.title))
                    .small()
                    .color(p.faint),
            );
            ui.add_space(4.0);
        }
        ui.label(
            egui::RichText::new(page.title)
                .size(23.0)
                .family(theme::semibold(ui))
                .color(p.text),
        );
        ui.add_space(4.0);
        sentence(ui, page.lead, p.muted, 14.5);
        if let Some((tone, said)) = self.help_now(page.page) {
            ui.add_space(10.0);
            now_line(ui, tone, &said);
        }
        if let Some(picture) = &page.picture {
            ui.add_space(14.0);
            self.show_picture(ui, picture);
        }
        ui.add_space(14.0);
        if page.page == HelpPage::Shortcuts {
            show_keys(ui, page.steps);
        } else {
            let ticks = (page.page == HelpPage::GettingStarted).then(|| self.getting_started());
            for (i, step) in page.steps.iter().enumerate() {
                let done = ticks
                    .as_ref()
                    .and_then(|t| t.get(i))
                    .is_some_and(|(_, d)| *d);
                self.show_step(ui, page, i, step, done);
                ui.add_space(10.0);
            }
        }
        if let Some(tip) = page.tip {
            ui.add_space(4.0);
            theme::card_frame()
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        icons::paint(ui.painter(), rect, Icon::Bulb, p.warn);
                        ui.add_space(4.0);
                        ui.vertical(|ui| sentence(ui, tip, p.body, 14.0));
                    });
                    if page.page == HelpPage::GettingStarted {
                        ui.add_space(8.0);
                        if icons::labelled_button(
                            ui,
                            Role::Primary,
                            true,
                            Icon::Route,
                            "Take the tour",
                            28.0,
                        )
                        .clicked()
                        {
                            self.start_tour(ui.ctx());
                        }
                    }
                });
        }
        self.show_previous_and_next(ui, page.page);
    }

    /// A step: its number (a tick once Getting started's is done), its sentence, and under it Show me and what it does.
    fn show_step(&mut self, ui: &mut egui::Ui, page: &Page, i: usize, step: &Step, done: bool) {
        let p = theme::palette();
        ui.horizontal_top(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
            if page.numbered {
                number(ui.painter(), rect.center(), i + 1, done, 1.0);
            } else {
                ui.painter()
                    .circle_filled(rect.center(), 3.0, p.accent_text);
            }
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.add_space(2.0);
                sentence(ui, step.text, if done { p.muted } else { p.body }, 14.5);
                if step.spot.is_none() && step.deed.is_none() {
                    return;
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if let Some(spot) = step.spot {
                        // a part of the sidebar put away is there once the sidebar is shown again
                        let tucked = spot.in_sidebar() && !self.sidebar_open;
                        let there = tucked || spot_rect(ui.ctx(), spot).is_some();
                        let showing = self.help.shown.is_some_and(|(s, _)| s == spot);
                        let shown = icons::labelled_button(
                            ui,
                            if showing { Role::Primary } else { Role::Normal },
                            there,
                            Icon::Eye,
                            "Show me",
                            24.0,
                        )
                        .on_hover_text(
                            "Ring it in the main window, and bring the main window to the front.",
                        )
                        .on_disabled_hover_text("It is not on the main window now.");
                        if shown.clicked() {
                            if tucked {
                                self.sidebar_open = true;
                            }
                            self.help.shown = Some((spot, Instant::now()));
                            ui.ctx().send_viewport_cmd_to(
                                egui::ViewportId::ROOT,
                                egui::ViewportCommand::Focus,
                            );
                        }
                    }
                    if let Some(deed) = step.deed {
                        if icons::labelled_button(
                            ui,
                            Role::Quiet,
                            true,
                            deed.icon(),
                            deed.label(),
                            24.0,
                        )
                        .clicked()
                        {
                            self.do_deed(ui.ctx(), deed);
                        }
                    }
                });
            });
        });
    }

    /// The picture of a page, as wide as the column or its own width, whichever is less, in the theme of the window,
    /// with its marks and its ring.
    fn show_picture(&mut self, ui: &mut egui::Ui, picture: &Picture) {
        let dark = theme::is_dark();
        let Some(texture) = self.picture_texture(ui.ctx(), picture.name, dark) else {
            return;
        };
        let p = theme::palette();
        let full = texture.size_vec2();
        let scale = (ui.available_width() / full.x).min(1.0);
        let (rect, _) = ui.allocate_exact_size(full * scale, egui::Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter_at(rect.expand(2.0));
        painter.image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(1.0, p.outline),
            egui::StrokeKind::Outside,
        );
        let at = |x: f32, y: f32| rect.min + egui::vec2(x, y) * scale;
        if let Some([x, y, w, h]) = picture.ring {
            let ring = egui::Rect::from_min_size(at(x, y), egui::vec2(w, h) * scale);
            painter.rect_stroke(
                ring,
                6.0,
                egui::Stroke::new(4.0, p.accent_text.gamma_multiply(0.25)),
                egui::StrokeKind::Outside,
            );
            painter.rect_stroke(
                ring,
                6.0,
                egui::Stroke::new(2.0, p.accent_text),
                egui::StrokeKind::Outside,
            );
        }
        for &(n, x, y) in picture.marks {
            // a mark keeps to the size of what it marks, down to where its number can still be read
            number(&painter, at(x, y), usize::from(n), false, scale.max(0.7));
        }
    }

    /// The texture of a picture, made from the program's own bytes the first time it is shown.
    fn picture_texture(
        &mut self,
        ctx: &egui::Context,
        name: &'static str,
        dark: bool,
    ) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.help.pictures.get(&(name, dark)) {
            return Some(texture.clone());
        }
        let icon = eframe::icon_data::from_png_bytes(picture_bytes(name, dark)?).ok()?;
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [icon.width as usize, icon.height as usize],
            &icon.rgba,
        );
        let texture = ctx.load_texture(
            format!("help-{name}-{dark}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.help.pictures.insert((name, dark), texture.clone());
        Some(texture)
    }

    /// What is so now that the page is about: the model runs or not, the engine in use, the last start that failed.
    fn help_now(&self, page: HelpPage) -> Option<(Option<Tone>, String)> {
        let state = self.session.state();
        let running = state == ServerState::Running;
        let model = self.session.model_name().map(str::to_string);
        match page {
            HelpPage::Chat | HelpPage::ConnectApp if !self.runs_alone() => {
                Some(match (running, model) {
                    (true, Some(model)) => (
                        Some(Tone::Good),
                        format!("{model} is running, so you can do this now."),
                    ),
                    _ => (
                        Some(Tone::Warn),
                        "Start a model first: press Start in the bar over the page.".to_string(),
                    ),
                })
            }
            HelpPage::FindModel if !self.runs_alone() => Some(match self.models.len() {
                0 => (
                    Some(Tone::Warn),
                    "There is no model in the list yet.".to_string(),
                ),
                1 => (
                    Some(Tone::Good),
                    "There is one model in the list.".to_string(),
                ),
                n => (
                    Some(Tone::Good),
                    format!("There are {n} models in the list."),
                ),
            }),
            HelpPage::Engines | HelpPage::Pictures => Some((
                None,
                format!("The engine in use is {}.", self.engine_name()),
            )),
            HelpPage::Remote => Some((
                None,
                match self.runner_spec() {
                    Some(there) => format!("The models run on {}.", there.name),
                    None => "The models run on this computer.".to_string(),
                },
            )),
            HelpPage::WontStart if state == ServerState::Crashed => Some((
                Some(Tone::Bad),
                "The last start failed. The log says why.".to_string(),
            )),
            HelpPage::Slow if self.total_ram_bytes > 0 => Some((
                None,
                format!(
                    "This computer has {} GB of memory.",
                    (self.total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0)).round()
                ),
            )),
            _ => None,
        }
    }

    /// The page before and the page after, at the foot of a page.
    fn show_previous_and_next(&mut self, ui: &mut egui::Ui, page: HelpPage) {
        let at = PAGES.iter().position(|p| p.page == page).unwrap_or(0);
        let before = at.checked_sub(1).map(|i| &PAGES[i]);
        let after = PAGES.get(at + 1);
        ui.add_space(16.0);
        let y = ui.cursor().top();
        ui.painter().hline(
            ui.max_rect().x_range(),
            y,
            egui::Stroke::new(1.0, theme::palette().line),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if let Some(before) = before {
                if ui.link(format!("‹ {}", before.title)).clicked() {
                    self.help_go(before.page);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(after) = after {
                    if ui.link(format!("{} ›", after.title)).clicked() {
                        self.help_go(after.page);
                    }
                }
            });
        });
    }

    /// Does what a step's button says.
    fn do_deed(&mut self, ctx: &egui::Context, deed: Deed) {
        let main =
            || ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        match deed {
            Deed::AddModel => {
                if !self.runs_alone() {
                    self.open_add_model();
                    main();
                }
            }
            Deed::Recommended => {
                if self.can_recommend() {
                    self.open_catalog();
                    main();
                }
            }
            Deed::Tools => {
                self.open_mcp_servers();
                main();
            }
            Deed::Settings => self.open_settings(),
            Deed::Configurations => {
                self.open_configurations();
                self.raise_configurations = true;
            }
            Deed::Log => {
                // the log opens under the page (C203)
                self.active_tab = super::Tab::Main;
                self.dock_open = true;
                main();
            }
            Deed::ModelsFolder => super::workbench::open_folder(&self.models_dir),
            Deed::Privacy => {
                let _ = webbrowser::open(super::shell::PRIVACY_URL);
            }
        }
    }

    pub(super) fn start_tour(&mut self, ctx: &egui::Context) {
        // most of what it is about is in the sidebar
        self.sidebar_open = true;
        self.help.tour = Some(0);
        self.help.shown = None;
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
    }

    /// What Help draws over the main window, after the main window: the ring of Show me, and the stop of the tour.
    pub(super) fn show_help_marks(&mut self, ctx: &egui::Context) {
        if let Some((spot, since)) = self.help.shown {
            let age = since.elapsed().as_secs_f32();
            match spot_rect(ctx, spot) {
                Some(rect) if age < SHOW_FOR => {
                    // it beats, twice a second, and fades over the last second
                    let beat = 0.5 + 0.5 * (age * std::f32::consts::TAU * 1.5).cos();
                    let fade = (SHOW_FOR - age).min(1.0);
                    ring(ctx, rect, beat, fade);
                    ctx.request_repaint();
                }
                _ => self.help.shown = None,
            }
        }
        if let Some(at) = self.help.tour {
            self.show_tour_stop(ctx, at);
        }
    }

    /// A stop of the tour: the main window darkened but for the part it is about, which is ringed, and beside it a
    /// card that says what it is, with Back, Next and End. A part the window does not show now is passed.
    fn show_tour_stop(&mut self, ctx: &egui::Context, at: usize) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.help.tour = None;
            return;
        }
        let Some((n, (spot, title, said))) = TOUR
            .iter()
            .enumerate()
            .skip(at)
            .find(|(_, (spot, _, _))| spot_rect(ctx, *spot).is_some())
        else {
            self.help.tour = None;
            return;
        };
        let rect = spot_rect(ctx, *spot).expect("it was found");
        let screen = ctx.content_rect();
        let hole = rect.expand(6.0);
        let shade = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("help_tour_shade"),
        ));
        let dim = Color32::from_black_alpha(150);
        for part in [
            egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, hole.min.y)),
            egui::Rect::from_min_max(egui::pos2(screen.min.x, hole.max.y), screen.max),
            egui::Rect::from_min_max(
                egui::pos2(screen.min.x, hole.min.y),
                egui::pos2(hole.min.x, hole.max.y),
            ),
            egui::Rect::from_min_max(
                egui::pos2(hole.max.x, hole.min.y),
                egui::pos2(screen.max.x, hole.max.y),
            ),
        ] {
            shade.rect_filled(part, 0.0, dim);
        }
        ring(ctx, rect, 1.0, 1.0);
        let width = 300.0;
        let below = hole.max.y + 12.0;
        let x =
            (rect.center().x - width / 2.0).clamp(screen.min.x + 10.0, screen.max.x - width - 10.0);
        // under the part when there is room, else over it
        let (pivot, y) = if below + 150.0 < screen.max.y {
            (egui::Align2::LEFT_TOP, below)
        } else {
            (egui::Align2::LEFT_BOTTOM, hole.min.y - 12.0)
        };
        let p = theme::palette();
        let mut go: Option<Option<usize>> = None;
        egui::Area::new(egui::Id::new("help_tour_card"))
            .order(egui::Order::Tooltip)
            .pivot(pivot)
            .fixed_pos(egui::pos2(x, y))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(p.card)
                    .stroke(egui::Stroke::new(1.0, p.accent_text))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(14))
                    .shadow(egui::Shadow {
                        offset: [0, 6],
                        blur: 18,
                        spread: 0,
                        color: Color32::from_black_alpha(110),
                    })
                    .show(ui, |ui| {
                        ui.set_width(width - 28.0);
                        ui.label(
                            egui::RichText::new(format!("{} OF {}", n + 1, TOUR.len()))
                                .size(11.0)
                                .family(theme::semibold(ui))
                                .color(p.accent_text),
                        );
                        ui.label(
                            egui::RichText::new(*title)
                                .size(17.0)
                                .family(theme::semibold(ui))
                                .color(p.text),
                        );
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new(*said).color(p.body));
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.link("End the tour").clicked() {
                                go = Some(None);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let last = n + 1 == TOUR.len();
                                    if theme::button_sized(
                                        ui,
                                        Role::Primary,
                                        true,
                                        if last { "Done" } else { "Next" },
                                        egui::vec2(64.0, 26.0),
                                    )
                                    .clicked()
                                    {
                                        go = Some((!last).then_some(n + 1));
                                    }
                                    if n > 0
                                        && theme::button_sized(
                                            ui,
                                            Role::Normal,
                                            true,
                                            "Back",
                                            egui::vec2(56.0, 26.0),
                                        )
                                        .clicked()
                                    {
                                        go = Some(Some(n - 1));
                                    }
                                },
                            );
                        });
                    });
            });
        match go {
            Some(next) => self.help.tour = next,
            None => self.help.tour = Some(n),
        }
    }
}

/// Whether a page has every word of `query` (lower case) at the start of a word of its title, its words or its steps:
/// "port" finds the port, not "report". An empty query is every page.
fn matches(page: &Page, query: &str) -> bool {
    let words = |text: &str| -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let mut all = words(page.title);
    all.extend(words(page.lead));
    all.extend(words(page.tip.unwrap_or("")));
    for step in page.steps {
        all.extend(words(step.text));
    }
    words(query)
        .iter()
        .all(|word| all.iter().any(|w| w.starts_with(word.as_str())))
}

/// A row of the sidebar: a picture and a name, on the grey of what is chosen.
fn nav_row(ui: &mut egui::Ui, icon: Icon, label: &str, chosen: bool) -> egui::Response {
    let p = theme::palette();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        if chosen || response.hovered() {
            let fill = if chosen {
                p.raised
            } else {
                p.raised.gamma_multiply(0.5)
            };
            ui.painter().rect_filled(rect, 6.0, fill);
        }
        icons::paint(
            ui.painter(),
            egui::Rect::from_min_size(
                egui::pos2(rect.left() + 9.0, rect.center().y - 7.5),
                egui::vec2(15.0, 15.0),
            ),
            icon,
            if chosen { p.accent_text } else { p.muted },
        );
        let font = egui::FontId::new(
            13.5,
            if chosen {
                theme::semibold(ui)
            } else {
                egui::FontFamily::Proportional
            },
        );
        ui.painter().text(
            egui::pos2(rect.left() + 33.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            font,
            if chosen { p.text } else { p.body },
        );
    }
    response
}

/// A line of what is so now: green when it is what the page needs, amber when something is to be done first, red
/// when something went wrong, plain when it is only so.
fn now_line(ui: &mut egui::Ui, tone: Option<Tone>, said: &str) {
    let p = theme::palette();
    let (fill, ink) = match tone {
        Some(tone) => (p.card.lerp_to_gamma(tone.color(), 0.14), tone.color()),
        None => (p.card, p.body),
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, ink);
                ui.add_space(4.0);
                ui.add(egui::Label::new(egui::RichText::new(said).color(ink)).wrap());
            });
        });
}

/// A number in a green circle: of a step, and of the same place on its picture. A step that is done has a tick.
fn number(painter: &egui::Painter, centre: egui::Pos2, n: usize, done: bool, size: f32) {
    let p = theme::palette();
    if done {
        painter.circle_filled(centre, 12.0, p.accent_soft);
        icons::paint(
            painter,
            egui::Rect::from_center_size(centre, egui::vec2(13.0, 13.0)),
            Icon::Check,
            p.accent_text,
        );
        return;
    }
    painter.circle_filled(centre, 13.0 * size, p.page);
    painter.circle_filled(centre, 11.0 * size, p.accent_text);
    painter.text(
        centre,
        egui::Align2::CENTER_CENTER,
        n.to_string(),
        egui::FontId::proportional(12.5 * size),
        p.page,
    );
}

/// The ring round a part of the main window: `beat` from 0 to 1 widens its glow, `fade` from 1 to 0 takes it away.
fn ring(ctx: &egui::Context, rect: egui::Rect, beat: f32, fade: f32) {
    let p = theme::palette();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("help_ring"),
    ));
    let rect = rect.expand(4.0);
    painter.rect_stroke(
        rect,
        8.0,
        egui::Stroke::new(4.0 + 6.0 * beat, p.accent_text.gamma_multiply(0.25 * fade)),
        egui::StrokeKind::Outside,
    );
    painter.rect_stroke(
        rect,
        8.0,
        egui::Stroke::new(2.5, p.accent_text.gamma_multiply(fade)),
        egui::StrokeKind::Outside,
    );
}

/// A sentence of a page, wrapped to the width there is: `**words**` in the bold of a name the window shows, `{play}`
/// a picture of that button between the words.
fn sentence(ui: &mut egui::Ui, text: &str, color: Color32, size: f32) {
    let p = theme::palette();
    let plain = egui::FontId::proportional(size);
    let bold = egui::FontId::new(size, theme::semibold(ui));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 3.0);
        let mut job = LayoutJob::default();
        let mut strong = false;
        for (k, part) in text.split(['{', '}']).enumerate() {
            if k % 2 == 1 {
                if !job.is_empty() {
                    ui.label(std::mem::take(&mut job));
                }
                button_picture(ui, part, size);
                continue;
            }
            for (j, run) in part.split("**").enumerate() {
                if j > 0 {
                    strong = !strong;
                }
                if run.is_empty() {
                    continue;
                }
                let format = if strong {
                    TextFormat::simple(bold.clone(), p.text)
                } else {
                    TextFormat::simple(plain.clone(), color)
                };
                job.append(run, 0.0, format);
            }
        }
        if !job.is_empty() {
            ui.label(job);
        }
    });
}

/// A picture of a button of the main window, in a sentence: `name` is its picture (`play`, `plug`...).
fn button_picture(ui: &mut egui::Ui, name: &str, size: f32) {
    let p = theme::palette();
    let icon = button_icon(name).unwrap_or(Icon::Help);
    let side = size + 6.0;
    ui.add_space(3.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let (fill, ink) = if icon == Icon::Play {
        (p.accent, p.on_accent)
    } else {
        (p.raised, p.text)
    };
    ui.painter().rect(
        rect,
        5.0,
        fill,
        egui::Stroke::new(1.0, p.outline),
        egui::StrokeKind::Inside,
    );
    icons::paint(ui.painter(), rect.shrink(5.0), icon, ink);
    ui.add_space(3.0);
}

/// The picture of a button a sentence names (`{play}`).
fn button_icon(name: &str) -> Option<Icon> {
    Some(match name {
        "play" => Icon::Play,
        "plug" => Icon::Plug,
        "globe" => Icon::Globe,
        "clip" => Icon::Clip,
        "more" => Icon::More,
        "gear" => Icon::Gear,
        "hide" => Icon::DoubleRight,
        "show" => Icon::DoubleLeft,
        "log" => Icon::Terminal,
        "plus" => Icon::Plus,
        "pencil" => Icon::Pencil,
        "folder" => Icon::Folder,
        "download" => Icon::Download,
        _ => return None,
    })
}

/// The keys of the shortcuts page: each key as a key, and what it does.
fn show_keys(ui: &mut egui::Ui, rows: &[Step]) {
    let p = theme::palette();
    let command = if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    };
    theme::card_frame()
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(14, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, row) in rows.iter().enumerate() {
                let (keys, does) = row.text.split_once('|').unwrap_or((row.text, ""));
                if i > 0 {
                    let y = ui.cursor().top();
                    ui.painter()
                        .hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0, p.line));
                }
                ui.horizontal(|ui| {
                    ui.set_min_height(36.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(170.0, 36.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            for (j, key) in keys.split('+').enumerate() {
                                if j > 0 {
                                    ui.label(theme::muted("+"));
                                }
                                key_cap(ui, if key == "Ctrl" { command } else { key });
                            }
                        },
                    );
                    ui.label(egui::RichText::new(does).color(p.body));
                });
            }
        });
}

/// A key, drawn as a key.
fn key_cap(ui: &mut egui::Ui, key: &str) {
    let p = theme::palette();
    let font = egui::FontId::new(12.5, theme::semibold(ui));
    let galley = ui.painter().layout_no_wrap(key.to_string(), font, p.text);
    let size = egui::vec2((galley.size().x + 14.0).max(26.0), 24.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect(
        rect,
        5.0,
        p.raised,
        egui::Stroke::new(1.0, p.outline),
        egui::StrokeKind::Inside,
    );
    ui.painter().hline(
        (rect.left() + 3.0)..=(rect.right() - 3.0),
        rect.bottom() - 1.0,
        egui::Stroke::new(1.5, p.outline),
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0 - egui::vec2(0.0, 1.0),
        galley,
        p.text,
    );
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::path::Path;
    use std::rc::Rc;

    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::settings::Settings;
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;

    /// llama.cpp with its program there (a file is all the window looks for), and `models` in its folder.
    fn app_in(dir: &Path, models: &[&str]) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine("llamacpp");
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        let mut app = App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        );
        app.bundled_models_dir = dir.join("bundled");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        app.binary = dir.join("bin/llama-server");
        std::fs::write(&app.binary, "").unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        for name in models {
            std::fs::File::create(dir.join("models").join(name))
                .unwrap()
                .set_len(GB)
                .unwrap();
        }
        app.models = app.scan_models();
        app.selected = 0;
        app
    }

    /// One frame of the main window as the application draws it, with Help over it and in its window, and `events`
    /// given to the main window; `look` is asked what it is to know while the frame is going (where the parts of the
    /// main window are is known only then).
    fn frame<R>(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        look: impl FnOnce(&egui::Context) -> R,
    ) -> R {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(980.0, 680.0),
            )),
            events,
            ..Default::default()
        };
        let mut seen = None;
        let mut look = Some(look);
        let mut output = ctx.run_ui(raw, |ui| {
            app.show_window(ui);
            app.show_help_marks(ui.ctx());
            app.show_help_window(ui.ctx());
            if let Some(look) = look.take() {
                seen = Some(look(ui.ctx()));
            }
        });
        output.textures_delta.clear();
        seen.expect("the frame was drawn")
    }

    /// Where `text` is drawn in a frame of the main window, if it is.
    fn text_at(app: &mut App, ctx: &egui::Context, text: &str) -> Option<egui::Rect> {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(980.0, 680.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| app.show_window(ui));
        output.textures_delta.clear();
        fn walk(shape: &egui::Shape, text: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Vec(inner) => inner.iter().find_map(|s| walk(s, text)),
                egui::Shape::Text(t) if t.galley.text() == text => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()))
                }
                _ => None,
            }
        }
        output.shapes.iter().find_map(|c| walk(&c.shape, text))
    }

    fn key(key: egui::Key) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]
    }

    fn click(at: egui::Pos2) -> Vec<Vec<egui::Event>> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![
            vec![egui::Event::PointerMoved(at)],
            vec![button(true)],
            vec![button(false)],
        ]
    }

    /// I33 (C191, C203): Help… of the QuickAI menu at the top of the sidebar opens Help, and so does F1; it is a window of
    /// its own, called so, and not a menu; asked for again, it opens on the page it is on.
    #[test]
    fn p3_i33_the_question_mark_and_f1_open_help_in_a_window_of_its_own() {
        let dir = scratch("i33");
        let mut app = app_in(&dir, &["Small-Q4_K_M.gguf"]);
        let ctx = egui::Context::default();
        // a display that opens a window of its own for a viewport, and says what each was called
        let titles = Rc::new(RefCell::new(Vec::<String>::new()));
        ctx.set_embed_viewports(false);
        let said = Rc::clone(&titles);
        egui::Context::set_immediate_viewport_renderer(move |ctx, viewport| {
            let egui::ImmediateViewport {
                ids,
                builder,
                mut viewport_ui_cb,
            } = viewport;
            said.borrow_mut()
                .push(builder.title.clone().unwrap_or_default());
            let mut raw = egui::RawInput {
                viewport_id: ids.this,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                ..Default::default()
            };
            raw.viewports
                .insert(ids.this, egui::ViewportInfo::default());
            let mut output = ctx.run_ui(raw, |ui| viewport_ui_cb(ui));
            output.textures_delta.clear();
        });
        let help = frame(&mut app, &ctx, Vec::new(), |ctx| spot_rect(ctx, Spot::Help))
            .expect("the QuickAI menu is at the top of the sidebar");
        assert!(
            titles.borrow().is_empty(),
            "no window before it is asked for"
        );
        for events in click(help.center()) {
            frame(&mut app, &ctx, events, |_| ());
        }
        let item = text_at(&mut app, &ctx, "Help…").expect("Help… in the QuickAI menu");
        for events in click(item.center()) {
            frame(&mut app, &ctx, events, |_| ());
        }
        assert!(app.help.open, "Help… opens Help");
        frame(&mut app, &ctx, Vec::new(), |_| ());
        assert!(
            titles.borrow().iter().any(|t| t == HELP_TITLE),
            "a window of its own: {:?}",
            titles.borrow()
        );
        // Escape in it closes it, and F1 in the main window opens it again
        app.help.open = false;
        frame(&mut app, &ctx, key(egui::Key::F1), |_| ());
        assert!(app.help.open, "F1 opens Help");
        // open already, the Help menu brings it to the front, on the page it is on
        app.help_go(HelpPage::Shortcuts);
        app.open_help();
        assert_eq!(app.help.page, HelpPage::Shortcuts);
        assert!(app.help.raise, "open already: it comes to the front");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// I34 (C191): every page's picture is in the program in the dark theme and the light, of one size, and every mark
    /// and ring drawn on it is inside it; a mark is the number of a step of its page; every picture the program carries
    /// is a page's; every sentence's bold is closed and every button it draws is one the window has.
    #[test]
    fn p3_i34_every_picture_is_there_in_both_themes_with_its_marks_inside_it() {
        let mut used = HashSet::new();
        for page in PAGES {
            let mut texts = vec![page.lead];
            texts.extend(page.tip);
            texts.extend(page.steps.iter().map(|s| s.text));
            for text in texts {
                assert_eq!(text.matches("**").count() % 2, 0, "{text}");
                for (k, part) in text.split(['{', '}']).enumerate() {
                    if k % 2 == 1 {
                        assert!(button_icon(part).is_some(), "{{{part}}} in {text}");
                    }
                }
            }
            let Some(picture) = &page.picture else {
                continue;
            };
            used.insert(picture.name);
            let size = |dark| {
                let bytes = picture_bytes(picture.name, dark).expect("in the program");
                let image = eframe::icon_data::from_png_bytes(bytes).expect("a PNG");
                (image.width as f32, image.height as f32)
            };
            let (w, h) = size(true);
            assert_eq!(
                size(false),
                (w, h),
                "{}: one size in both themes",
                picture.name
            );
            for &(n, x, y) in picture.marks {
                assert!(
                    (0.0..=w).contains(&x) && (0.0..=h).contains(&y),
                    "{}: mark {n} at {x},{y} is outside {w}x{h}",
                    picture.name
                );
                assert!(
                    (1..=page.steps.len()).contains(&usize::from(n)),
                    "{:?}: mark {n} is no step",
                    page.page
                );
            }
            if let Some([x, y, rw, rh]) = picture.ring {
                assert!(
                    x >= 0.0 && y >= 0.0 && x + rw <= w && y + rh <= h,
                    "{}",
                    picture.name
                );
            }
        }
        for name in PICTURE_NAMES {
            assert!(used.contains(name), "{name} is on no page");
        }
    }

    /// I35 (C191): the search finds the pages that have every word typed, at the start of a word: "port" is the port's
    /// page and not the one that says "report"; an empty box is every page, and a word no page has is none.
    #[test]
    fn p3_i35_the_search_finds_the_pages_by_the_start_of_their_words() {
        let found = |query: &str| -> Vec<HelpPage> {
            PAGES
                .iter()
                .filter(|p| matches(p, query))
                .map(|p| p.page)
                .collect()
        };
        assert_eq!(found("port"), [HelpPage::WontStart]);
        assert_eq!(found("api key"), [HelpPage::ConnectApp]);
        assert_eq!(found("").len(), PAGES.len());
        assert!(found("zebra").is_empty());
        // a page's own words, whatever their case
        assert!(found("PICTURES").contains(&HelpPage::Pictures));
    }

    /// I36 (C191): Show me is offered for a part the main window shows now, rings it while its time lasts, and stops for
    /// a part the window does not show (the plug, while the model is stopped).
    #[test]
    fn p3_i36_show_me_rings_a_part_the_main_window_shows_and_only_that() {
        let dir = scratch("i36");
        let mut app = app_in(&dir, &["Small-Q4_K_M.gguf"]);
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, Vec::new(), |_| ());
        let (play, plug) = frame(&mut app, &ctx, Vec::new(), |ctx| {
            (spot_rect(ctx, Spot::Play), spot_rect(ctx, Spot::Plug))
        });
        let play = play.expect("the play button is on the window");
        assert!(plug.is_none(), "no plug while the model is stopped");
        app.help.shown = Some((Spot::Play, Instant::now()));
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(980.0, 680.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            app.show_window(ui);
            app.show_help_marks(ui.ctx());
        });
        output.textures_delta.clear();
        assert!(app.help.shown.is_some(), "it lasts");
        // (the theme is the process's, and another test may have changed it)
        let green = [true, false].map(|dark| theme::palette_of(dark).accent_text);
        let ringed = output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Rect(r)
                if (r.rect.center() - play.center()).length() < 1.0
                    && (r.rect.width() - play.width() - 8.0).abs() < 1.0
                    && green.contains(&r.stroke.color))
        });
        assert!(ringed, "a ring round the play button");
        app.help.shown = Some((Spot::Plug, Instant::now()));
        frame(&mut app, &ctx, Vec::new(), |_| ());
        assert!(app.help.shown.is_none(), "nothing to ring: it stops");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// I37 (C191): the tour stops at the parts of the main window one after the other, passes a part the window does not
    /// show now, ends after the last, and Escape ends it.
    #[test]
    fn p3_i37_the_tour_passes_what_the_window_does_not_show_and_escape_ends_it() {
        let dir = scratch("i37");
        let ctx = egui::Context::default();
        // with no model the page is the first run's: whatever of the tour it lacks is passed
        let mut empty = app_in(&dir.join("empty"), &[]);
        frame(&mut empty, &ctx, Vec::new(), |_| ());
        empty.help.tour = Some(2);
        let shown = frame(&mut empty, &ctx, Vec::new(), |ctx| {
            TOUR.iter()
                .map(|(spot, _, _)| spot_rect(ctx, *spot).is_some())
                .collect::<Vec<_>>()
        });
        let at = empty.help.tour.expect("a part is left to show");
        assert!(at >= 2 && shown[at], "{at} {shown:?}");
        assert!(
            shown[2..at].iter().all(|s| !s),
            "only what is not shown is passed: {at} {shown:?}"
        );
        // with a model, How it runs is there and the tour stops at it
        let mut app = app_in(&dir.join("model"), &["Small-Q4_K_M.gguf"]);
        frame(&mut app, &ctx, Vec::new(), |_| ());
        let change = TOUR
            .iter()
            .position(|(spot, _, _)| *spot == Spot::Change)
            .unwrap();
        app.help.tour = Some(change);
        frame(&mut app, &ctx, Vec::new(), |_| ());
        assert_eq!(app.help.tour, Some(change));
        app.help.tour = Some(TOUR.len());
        frame(&mut app, &ctx, Vec::new(), |_| ());
        assert_eq!(app.help.tour, None, "after the last it is over");
        app.help.tour = Some(0);
        frame(&mut app, &ctx, key(egui::Key::Escape), |_| ());
        assert_eq!(app.help.tour, None, "Escape ends it");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
