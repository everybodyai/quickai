//! Help (C191): a window of its own, which **Help** at the foot of the sidebar (C206) and F1 open.
//! Its pages are in a sidebar under a search box and Getting started; each is a title, a line on what it is for, a
//! line on what is so now, a picture of the part of QuickAI it is about with numbered marks on it, and its steps under
//! the picture with the same numbers. A step that names a button can **Show me** it: the button is ringed in the main
//! window for a moment, and the main window comes to the front. A step that can be done for the person has a button
//! that does it. **Take the tour** goes round the main window one part at a time.
//!
//! The pictures are real screenshots of QuickAI, in the dark theme and the light, taken from the window
//! itself; the marks are drawn on them here, in their pixels. They are in the program, so Help needs no network.
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
    /// Talking to QuickAI (C216): the microphone, reading aloud, hands-free.
    Talk,
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
            Group::Using => "Using QuickAI",
            Group::Further => "Advanced",
            Group::Wrong => "Troubleshooting",
            Group::Know => "Reference",
        }
    }
}

/// A part of the main window that Help can point at. The main window says where each is as it draws it (`mark_spot`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Spot {
    EngineMenu,
    /// The models of the list in the sidebar (C213: the tour stops at the parts of the window one by one).
    Models,
    AddModel,
    Play,
    Plug,
    Change,
    ChatBox,
    Clip,
    /// The microphone in the chat's box (C216).
    Mic,
    ChatMenu,
    LogTab,
    Computer,
    Settings,
    Help,
    /// The parts of the panel at the right (C213): the configuration's box, Context, Graphics, Extras, Tools and This
    /// computer at its foot.
    Configuration,
    Context,
    Graphics,
    Extras,
    Tools,
    ThisComputer,
}

impl Spot {
    /// Whether it is in the sidebar, which can be put away.
    fn in_sidebar(self) -> bool {
        matches!(
            self,
            Spot::EngineMenu
                | Spot::Models
                | Spot::AddModel
                | Spot::Computer
                | Spot::Settings
                | Spot::Help
        )
    }

    /// Whether it is in the panel at the right, which can be put away.
    fn in_panel(self) -> bool {
        matches!(
            self,
            Spot::Change
                | Spot::Plug
                | Spot::Configuration
                | Spot::Context
                | Spot::Graphics
                | Spot::Extras
                | Spot::Tools
                | Spot::ThisComputer
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
    /// The Voice page of the Settings (C216).
    Voice,
}

impl Deed {
    fn label(self) -> &'static str {
        match self {
            Deed::AddModel => "Open Add a model",
            Deed::Recommended => "Open Recommended",
            Deed::Settings => "Open Settings",
            Deed::Configurations => "Open Configurations",
            Deed::Log => "Show the log",
            Deed::ModelsFolder => "Open the models folder",
            Deed::Privacy => "Read the privacy policy",
            Deed::Tools => "Open MCP servers",
            Deed::Voice => "Open Voice settings",
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
            Deed::Voice => Icon::Mic,
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
        lead: "Three steps to your first response from a model running on this computer.",
        picture: Some(Picture {
            name: "main",
            marks: &[(1, 165.0, 124.0), (2, 188.0, 175.0), (3, 776.0, 28.0)],
            ring: None,
        }),
        steps: &[
            step(
                "Choose an **engine** under **This computer** at the top of the sidebar. llama.cpp is the default; \
                 Ollama and ComfyUI can be installed with one click.",
            )
            .at(Spot::EngineMenu),
            step("**Add a model** with the {plus} beside **MODELS**. The suggestions are matched to this computer.")
                .at(Spot::AddModel)
                .does(Deed::AddModel),
            step("Press **Start** in the toolbar above the page, then type a message in the box at the bottom.")
                .at(Spot::Play),
        ],
        numbered: true,
        tip: Some("New to QuickAI? **Take the tour**: a two-minute introduction to each part of the window."),
    },
    Page {
        page: HelpPage::FindModel,
        group: Group::Using,
        icon: Icon::Search,
        title: "Find the right model",
        lead: "How to choose a model that suits this computer, and how to download it.",
        picture: Some(Picture {
            name: "add-model",
            marks: &[(2, 591.0, 186.0), (3, 284.0, 28.0), (4, 660.0, 28.0), (5, 394.0, 393.0)],
            ring: Some([604.0, 172.0, 96.0, 28.0]),
        }),
        steps: &[
            step("Press {plus} beside **MODELS**, then **Add a model…**, or press Ctrl+K.")
                .at(Spot::AddModel)
                .does(Deed::AddModel),
            step(
                "With the search box empty, QuickAI suggests models that fit this computer's memory, one row each. \
                 **Get** downloads the size that suits this computer; its ▾ lists the other sizes, such as **Higher \
                 quality** or **Smaller**.",
            ),
            step(
                "Type keywords to search online: Hugging Face for llama.cpp, ollama.com for Ollama. The best matches \
                 come first, and a repository name such as owner/model also finds the GGUF versions others have made \
                 of it. Pressing Enter in the empty box lists the most popular models.",
            ),
            step("Paste a link to a .gguf file to download it, or press the folder {folder} to add a file you already have."),
            step(
                "To browse on your own, **Hugging Face** below the list opens its model directory in your browser. \
                 Paste the link of the model you choose into the box.",
            ),
            step(
                "**Recommended** in the sidebar lists every model QuickAI has verified, by type, with the same **Get** \
                 button. When nothing there matches your search, it offers to search Hugging Face.",
            )
            .does(Deed::Recommended),
        ],
        numbered: true,
        tip: Some(
            "Smaller files (Q4 in the name) are faster and use less memory; larger ones (Q8) give slightly better answers.",
        ),
    },
    Page {
        page: HelpPage::Chat,
        group: Group::Using,
        icon: Icon::Chat,
        title: "Chat, images and files",
        lead: "Chat with the running model, and give it an image or a document to read.",
        picture: Some(Picture {
            name: "chat",
            marks: &[(2, 420.0, 629.0), (3, 34.0, 596.0), (4, 640.0, 240.0), (5, 673.0, 52.0)],
            ring: None,
        }),
        steps: &[
            step("Start the model with **Start** in the toolbar above the page.").at(Spot::Play),
            step("Type in the box at the bottom. **Enter** sends the message; **Shift+Enter** starts a new line.")
                .at(Spot::ChatBox),
            step("The paper clip {clip} attaches an image, for models that support vision, or a text file to read.")
                .at(Spot::Clip),
            step("Below each response: **Copy**, **Regenerate**, and the generation speed."),
            step(
                "The {more} at the right end of the toolbar lets you keep chats on this computer, set instructions and \
                 the response style in **Options…** (in a separate window), or export a chat to a file. The {pencil} \
                 beside it starts a new chat.",
            )
            .at(Spot::ChatMenu),
        ],
        numbered: true,
        tip: Some(
            "QuickAI is designed to keep what you type on this computer, unless the model runs on another computer you \
             have added.",
        ),
    },
    Page {
        page: HelpPage::Talk,
        group: Group::Using,
        icon: Icon::Mic,
        title: "Talk to QuickAI (experimental)",
        lead: "Speak your message instead of typing it, have responses read aloud, or hold a hands-free conversation. \
               Your speech is transcribed, and responses are read aloud, on this computer, unless you choose a speech \
               server elsewhere. This feature is new and still being refined, so it is off until you enable it.",
        // Settings › Voice with voice on: its switch, the language spoken, the speech model, and what answers are read in
        picture: Some(Picture {
            name: "voice",
            marks: &[(1, 18.0, 75.0), (3, 18.0, 221.0), (4, 18.0, 272.0), (5, 18.0, 553.0)],
            ring: None,
        }),
        steps: &[
            step(
                "Enable **Use voice** in **Settings › Voice**. Until then, QuickAI shows no microphone and does not \
                 access it.",
            )
            .does(Deed::Voice),
            step(
                "Press the microphone {mic} in the chat box and speak; press ■ when you have finished. Your words \
                 appear in the box, where you can edit them before sending. Alternatively, hold **Ctrl+Space** while \
                 you speak.",
            )
            .at(Spot::Mic),
            step(
                "**Language** is the language you speak. Its list shows English first, then the most widely spoken \
                 languages and the languages of each part of the world; type a few letters of a language's name, in \
                 English or in the language itself, to find it. Whisper recognizes 100 of its 135 languages, and \
                 **Omnilingual** all of them: when you select a language, Settings names the most accurate speech \
                 model for it.",
            ),
            step(
                "On first use, QuickAI downloads its speech recognition model (82 MB), unless your package already \
                 includes it. In **Speech model** you can choose a larger Whisper model, which handles accents and \
                 background noise better, or **Omnilingual**, which is more accurate for Urdu, Arabic, Persian and \
                 most Indian languages.",
            ),
            step(
                "QuickAI does not include a voice. **Read in** sets the language responses are read aloud in (English \
                 unless you choose another), and **Voice** lists its voices, the most natural first; the line below \
                 them names what reads, its size and its license. Each voice is downloaded on request: **Kokoro** for \
                 English and six more languages, and voices for 98 languages in all. For a language without a voice \
                 yet, Kokoro reads in the meantime, and you can use a voice of your own: a voice folder, a speech \
                 server or a program.",
            ),
            step(
                "**Read aloud** below a response reads it and highlights the sentence being read. **Read answers \
                 aloud**, in Settings › Voice, reads every response as it arrives; it is off by default.",
            ),
            step(
                "**Hands-free**, in the {more} at the right end of the toolbar or by pressing and holding the microphone, \
                 listens, sends what you said when you pause, reads the response and listens again. Press **Esc** to \
                 end it.",
            )
            .at(Spot::ChatMenu),
            step(
                "**Settings › Voice** also covers the microphone, the pause that sends your message, and the speed of \
                 reading.",
            ),
        ],
        numbered: true,
        tip: Some("Your speech is processed on this computer, and QuickAI is designed to keep it there."),
    },
    Page {
        page: HelpPage::ConnectApp,
        group: Group::Using,
        icon: Icon::Plug,
        title: "Connect an app",
        lead: "Use your model from other applications, such as a code editor, a notes app or your own scripts. Any \
               software that supports OpenAI's API can connect to it.",
        picture: Some(Picture {
            name: "connect",
            marks: &[(1, 233.0, 84.0), (2, 13.0, 126.0), (3, 13.0, 161.0), (4, 165.0, 287.0)],
            ring: Some([247.0, 68.0, 32.0, 32.0]),
        }),
        steps: &[
            step(
                "Start the model: the panel on the right of the page becomes **Connect an app**. The {hide} at its \
                 top hides it, and the {show} at the right end of the toolbar restores it.",
            )
            .at(Spot::Plug),
            step(
                "Copy the **Address**, the first line of the box, into the other application. It may be called the \
                 base URL or the endpoint.",
            ),
            step("Copy the **Model** name below it as well. Any **API key** is accepted, or none."),
            step("**Open Chat UI** {globe} below them opens a chat in your browser, when the engine provides one."),
        ],
        numbered: true,
        tip: Some(
            "The address is reachable from this computer only; other computers on your network cannot connect to it. \
             **Example** copies a sample request in curl, Python or JavaScript, and its arrow displays it.",
        ),
    },
    Page {
        page: HelpPage::Pictures,
        group: Group::Using,
        icon: Icon::Image,
        title: "Create pictures and video",
        lead: "ComfyUI creates images and video from workflows you build in its own web page.",
        picture: Some(Picture {
            name: "engine-menu",
            marks: &[(1, 165.0, 68.0), (2, 140.0, 205.0)],
            ring: None,
        }),
        steps: &[
            step("Open the engine menu under **This computer** at the top of the sidebar.").at(Spot::EngineMenu),
            step(
                "Choose **ComfyUI**. Its page names the version it installs; leave **ComfyUI-Manager** ticked and click \
                 **Install**: about 17 GB, most of which is the computation library it relies on.",
            ),
            step(
                "Get models: **Start and open the Manager** opens ComfyUI's page with its Model Manager, about 500 \
                 models. **Recommended** lists the ones QuickAI has verified, and **Search or paste…** searches Hugging \
                 Face.",
            ),
            step("Press **Start**, then **Open ComfyUI** to open its page in your browser."),
            step("The **Download** button on that page saves a model directly to QuickAI's models folder."),
        ],
        numbered: true,
        tip: Some("ComfyUI runs on this computer and, unless you change its settings, works offline."),
    },
    Page {
        page: HelpPage::Remote,
        group: Group::Further,
        icon: Icon::Computer,
        title: "Use a more powerful computer",
        lead: "Run large models on another computer of yours and use them from here.",
        picture: Some(Picture {
            name: "computer-menu",
            marks: &[(2, 180.0, 126.0), (3, 175.0, 63.0)],
            ring: None,
        }),
        steps: &[
            step("Make sure you can connect to the other computer over SSH from here, using your own key."),
            step(
                "Click **This computer** at the top of the sidebar, then **Add a computer…**, and enter its SSH \
                 address (user@host, or a Host entry from your SSH configuration). QuickAI tests the connection and \
                 explains how to resolve any problem it finds.",
            )
            .at(Spot::Computer),
            step(
                "Select it in the same menu. **Start** then runs the model there: QuickAI copies itself to that \
                 computer and installs anything missing, or uses an engine that is already running there.",
            ),
        ],
        numbered: true,
        tip: Some("QuickAI opens nothing to the network: it connects through your own SSH client and keys."),
    },
    Page {
        page: HelpPage::Tools,
        group: Group::Further,
        icon: Icon::Plug,
        title: "Give your model tools",
        lead: "MCP servers let a model read your files, fetch web pages and more. QuickAI runs them on this computer \
               and offers their tools in the chat, with llama.cpp or Ollama, including for models on another computer.",
        picture: None,
        steps: &[
            step(
                "Press **MCP servers** in the sidebar. Enter a name for the server and the command or address from its \
                 instructions, then press **Add**.",
            )
            .does(Deed::Tools),
            step(
                "Start a model and chat. The wrench in the chat box shows, in its corner, how many servers the model \
                 may use; press it to enable or disable each one. **Tools** in the panel on the right also has a \
                 switch for each server.",
            ),
            step(
                "When the model requests a tool, a card shows what the tool would receive: press **Allow once**, \
                 **Always allow**, **Allow all tools** or **Deny**. The model then responds using what the tool \
                 returned.",
            ),
            step(
                "On the **MCP servers** page, **Ask before a tool runs** can be turned off so that tools run without asking, and \
                 **Tool calls in a row** sets how many replies in a row may use tools after one message (8 unless you \
                 choose more, or no limit). Under **What they offer**, each tool can be set to **Ask**, **Always** or \
                 **Off**. \
                 With llama.cpp, its own chat page (**Open Chat UI**) can use the servers as well.",
            ),
        ],
        numbered: true,
        tip: Some(
            "A server can do anything its program can do on this computer, so add only servers you trust. If one fails \
             to start, the MCP servers page shows the reason.",
        ),
    },
    Page {
        page: HelpPage::Engines,
        group: Group::Further,
        icon: Icon::Sliders,
        title: "Switch engines",
        lead: "The engine is the program that runs your models. Each one has its own strengths.",
        picture: Some(Picture {
            name: "engine-menu",
            marks: &[(1, 165.0, 68.0)],
            ring: None,
        }),
        steps: &[
            step("Choose the engine under **This computer** at the top of the sidebar.").at(Spot::EngineMenu),
            step("**llama.cpp** runs .gguf files from a folder. It is QuickAI's default engine."),
            step("**Ollama** downloads models by name and manages them itself."),
            step("**ComfyUI** creates images and video."),
            step("Each engine keeps its own models and settings. An engine that is not installed shows an **Install** button."),
        ],
        numbered: true,
        tip: None,
    },
    Page {
        page: HelpPage::Configurations,
        group: Group::Further,
        icon: Icon::Save,
        title: "Save and share a configuration",
        lead: "A configuration is a model together with its run settings, saved under a name so that you can reuse it \
               or share it.",
        picture: Some(Picture {
            name: "change",
            marks: &[
                (1, 180.0, 84.0),
                (2, 126.0, 175.0),
                (3, 132.0, 334.0),
                (4, 128.0, 420.0),
                (5, 240.0, 125.0),
            ],
            ring: None,
        }),
        steps: &[
            step(
                "The panel on the right of the page shows how the model runs. The {hide} at its top hides it, and the \
                 {show} at the right end of the toolbar restores it.",
            )
            .at(Spot::Change),
            step(
                "**Context** is how much of the conversation the model remembers: choose a length from **4K** to \
                 **256K**, or **Other…** for a custom value and the longer sizes, **512K** to **2M**. A dot on each \
                 shows where it fits: green on the graphics card, amber partly in system memory (slower), red too \
                 large. The underlined length is the one best suited to this model on this computer.",
            )
            .at(Spot::Context),
            step(
                "**Graphics** is where the model runs: **Card**, **Card + processor** (part of the model on the card) \
                 or **Processor**. QuickAI fits each model to the card until you choose otherwise. With several cards, \
                 select the ones to use. **This computer**, at the bottom, shows how much of each card and of system \
                 memory the model uses. Ollama places models itself: Graphics then lists the cards it found and how \
                 much of the model it placed on them.",
            )
            .at(Spot::Graphics),
            step(
                "**Extras** shows what is added at startup: a vision model's projector, with a switch, and your own \
                 arguments and environment variables, each as a chip you can edit or remove. **Tools** shows a switch \
                 for each MCP server. The gear at the top of the panel opens the remaining startup settings, including \
                 the port.",
            )
            .at(Spot::Extras),
            step(
                "**Configuration** at the top of the panel saves the run settings under a name: **Save as a new \
                 configuration…**. Choose a saved configuration from the same list to use it again. Its box is \
                 outlined in amber when the panel contains unsaved changes. Choosing another model leaves the saved \
                 configuration unchanged: the panel then holds an **Unnamed** configuration until you save it.",
            )
            .at(Spot::Configuration),
            step(
                "**Configurations…** in the **QuickAI** menu lists them all: use, rename or delete a configuration, or \
                 export it to a file to share.",
            )
            .does(Deed::Configurations),
        ],
        numbered: true,
        tip: Some("A configuration file you receive shows exactly what it would change before anything is applied."),
    },
    Page {
        page: HelpPage::Settings,
        group: Group::Further,
        icon: Icon::Gear,
        title: "Settings overview",
        lead: "Settings apply to the selected engine. Open them with **Settings…** in the **QuickAI** menu at the top \
               left, or press Ctrl+,.",
        picture: Some(Picture {
            name: "settings",
            marks: &[
                (1, 196.0, 85.0),
                (2, 196.0, 127.0),
                (3, 196.0, 169.0),
                (4, 196.0, 211.0),
                (5, 196.0, 253.0),
                (6, 196.0, 295.0),
            ],
            ring: None,
        }),
        steps: &[
            step("**General**: where models are stored, and Live traffic, which shows each request and response.")
                .at(Spot::Settings)
                .does(Deed::Settings),
            step("**Program**: the builds of the engine's program. Switch to another build, look for a newer one, or use your own."),
            step(
                "**Starting it**: the port, the additional arguments passed to the program, and the full command line \
                 that Start runs. The gear at the top of the panel opens this page.",
            ),
            step("**MCP servers**: the servers whose tools a model may use, as in **MCP servers** in the sidebar."),
            step("**Other computers**: whether QuickAI may set up a computer you add, in order to run models there."),
            step("**Voice**: speaking to QuickAI and having responses read aloud, off until you enable it.")
                .does(Deed::Voice),
        ],
        numbered: true,
        tip: Some(
            "Rest the pointer on a setting's name to see what it does. With Ollama, **Open WebUI** has its own page: a \
             chat interface in your browser.",
        ),
    },
    Page {
        page: HelpPage::WontStart,
        group: Group::Wrong,
        icon: Icon::Warn,
        title: "The model won't start",
        lead: "What to check when the model does not start, or stops unexpectedly.",
        picture: Some(Picture {
            name: "port-taken",
            marks: &[(2, 312.0, 90.0)],
            ring: None,
        }),
        steps: &[
            step("Open the log with **Log** at the bottom of the page: the engine states there why it stopped.")
                .at(Spot::LogTab)
                .does(Deed::Log),
            step(
                "**Port is taken**: another program is using the port. **Use port** selects the next free one, \
                 **Stop it** stops the other program, and **Override** stops it and starts QuickAI's.",
            ),
            step(
                "**Not enough memory**: choose a shorter context in the panel on the right (one with a green dot), or \
                 a smaller model.",
            )
            .at(Spot::Change),
            step("**The program is missing**: the **Program** page of the Settings lists the available builds; select one.")
                .does(Deed::Settings),
        ],
        numbered: true,
        tip: Some("Still having trouble? The log shows the engine's output verbatim; include it when you ask for help."),
    },
    Page {
        page: HelpPage::Slow,
        group: Group::Wrong,
        icon: Icon::Bolt,
        title: "Slow or out of memory",
        lead: "A model runs quickly when it fits in the memory of the graphics card, or of the computer.",
        picture: Some(Picture {
            name: "change",
            marks: &[(1, 24.0, 214.0)],
            ring: None,
        }),
        steps: &[
            step(
                "In the panel on the right, a shorter context (**4K**, **8K**) uses less memory, although the model \
                 then remembers less of a long conversation.",
            )
            .at(Spot::Change),
            step(
                "A smaller model is faster: fewer parameters (4B rather than 9B), or a more compact file of the same \
                 model (Q4 rather than Q8).",
            ),
            step("**Add a model** suggests models that fit this computer's memory.").does(Deed::AddModel),
            step(
                "**Graphics** in the panel: when the whole model does not fit on the card, **Card + processor** places \
                 as many of its layers on the card as will fit. **This computer**, at the bottom of the panel, shows \
                 the memory used. With Ollama, which makes this decision itself, Graphics shows how much of the model \
                 is on the card while it runs.",
            )
            .at(Spot::Graphics),
            step("Other programs that use the graphics card, such as a game or another model, leave less room for it."),
        ],
        numbered: true,
        tip: None,
    },
    Page {
        page: HelpPage::Files,
        group: Group::Know,
        icon: Icon::Folder,
        title: "Where your files are stored",
        lead: "Everything QuickAI stores is kept in the .quickai folder in your home directory, and in its settings.",
        picture: None,
        steps: &[
            step("**Models**: .quickai/models, for llama.cpp. The location can be changed on the **General** page of the Settings.")
                .does(Deed::ModelsFolder),
            step("**Ollama, ComfyUI and Open WebUI**: each in its own folder alongside it, with its models and builds."),
            step(
                "**Chats**: in .quickai/chats, and only when **Keep chats** is enabled, under the {more} at the right \
                 end of the toolbar.",
            ),
            step("**Settings and configurations**: in your system's settings folder (.config/quickai on Linux)."),
        ],
        numbered: false,
        tip: None,
    },
    Page {
        page: HelpPage::Privacy,
        group: Group::Know,
        icon: Icon::Lock,
        title: "Privacy and network use",
        lead: "QuickAI is designed to send nothing from this computer unless you ask it to.",
        picture: None,
        steps: &[
            step(
                "Your messages and the model's responses are kept on this computer: the engine accepts connections \
                 from this computer only.",
            ),
            step("QuickAI uses the internet only at your request: to search for or download a model or an engine."),
            step(
                "There is no account, no analytics and no update check. When a problem occurs, QuickAI displays a \
                 report that you may choose to submit; it sends nothing automatically.",
            ),
            step("A model on another computer you have added receives your messages through your own SSH connection.")
                .does(Deed::Privacy),
        ],
        numbered: false,
        tip: Some(
            "We take great care to ensure this, but as no software is free of defects, it cannot be guaranteed. The \
             privacy policy describes the details and its limitations.",
        ),
    },
    Page {
        page: HelpPage::Shortcuts,
        group: Group::Know,
        icon: Icon::Keyboard,
        title: "Keyboard shortcuts",
        lead: "Start, stop and chat without using the mouse.",
        picture: None,
        steps: &[
            step("Ctrl+Enter|Start or stop the model"),
            step("Ctrl+K|Add a model"),
            step("Ctrl+L|Move to the chat box"),
            step("Enter|Send a message"),
            step("Ctrl+Space|Hold to speak, when voice is enabled (your words appear in the chat box)"),
            step("Ctrl+Shift+Space|Start or end hands-free mode, when voice is enabled"),
            step("Shift+Enter|Insert a new line in a message"),
            step("Ctrl+,|Open Settings"),
            step("F1|Open Help"),
            step("Ctrl+F|Search Help (in this window)"),
            step("Alt+Left|Go back to the previous page (in this window)"),
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

/// A stop of the tour: the part of the main window, and what is said beside it. It goes round the window as it is
/// used: where the model runs and which, how it runs (the panel, part by part), then the chat, the log and Help. A part
/// the window does not show now (the panel's parts with no model, Connect an app while it is stopped) is passed.
const TOUR: &[(Spot, &str, &str)] = &[
    (
        Spot::Computer,
        "Where it runs",
        "This computer, or a more powerful one you add. Everything below applies to the computer selected here.",
    ),
    (
        Spot::EngineMenu,
        "The engine",
        "The program that runs your models. llama.cpp is the default; Ollama and ComfyUI can be installed from here.",
    ),
    (
        Spot::Models,
        "Your models",
        "Select a model to use it. Its dot turns green while it runs, and its ⋯ menu has its details and Delete.",
    ),
    (
        Spot::AddModel,
        "Add a model",
        "Suggestions that fit this computer, an online search, or a file you already have. Also Ctrl+K.",
    ),
    (
        Spot::Configuration,
        "Configuration",
        "The run settings, saved under a name. An amber outline indicates unsaved changes; choosing another model \
         creates an Unnamed configuration and leaves the saved one unchanged.",
    ),
    (
        Spot::Context,
        "Context",
        "How much of the conversation the model remembers. The dot shows whether each length fits: green fits, amber \
         is tight (with Card, partly in system memory), red is too large. Other… offers 512K to 2M and custom values.",
    ),
    (
        Spot::Graphics,
        "Graphics",
        "Where the model runs: on the graphics card, split between the card and the processor, or on the processor \
         alone. When split, as many layers as fit the chosen context go on the card. With Ollama, it shows what \
         Ollama chose.",
    ),
    (
        Spot::Extras,
        "Extras",
        "What is added at startup: a vision model's projector, and your own arguments and environment variables, each \
         as a chip.",
    ),
    (
        Spot::Tools,
        "Tools",
        "The MCP servers the model can use in the chat, each with a switch. QuickAI asks before a tool runs, unless \
         you turned that off in MCP servers.",
    ),
    (
        Spot::ThisComputer,
        "This computer",
        "How much of each card and of system memory the model and its context use, and the expected speed.",
    ),
    (
        Spot::Play,
        "Start",
        "Starts the model selected in the list. Ctrl+Enter also starts and stops it.",
    ),
    (
        Spot::Plug,
        "Connect an app",
        "While the model runs, the panel provides its address, the model name and a sample request for other \
         applications.",
    ),
    (
        Spot::ChatBox,
        "Chat",
        "Type here once the model is running. The paper clip attaches an image or a file, and the wrench shows the \
         available tools.",
    ),
    (
        Spot::Mic,
        "Talk",
        "Press the microphone to speak instead of typing. Press and hold it for hands-free mode: it listens, responds \
         aloud and listens again.",
    ),
    (
        Spot::LogTab,
        "Log and Live traffic",
        "The engine's output, verbatim, and the requests other applications send to it. Check here when something \
         goes wrong.",
    ),
    (
        Spot::Help,
        "Help",
        "At the bottom of the sidebar, beside About and the theme switch: every feature explained, with illustrations. \
         F1 also opens it.",
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
    "voice",
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
                        let tucked = (spot.in_sidebar() && !self.sidebar_open)
                            || (spot.in_panel() && !self.panel_open);
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
                                if spot.in_sidebar() {
                                    self.sidebar_open = true;
                                } else {
                                    self.panel_open = true;
                                }
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
                        "Start a model first: press Start in the toolbar above the page."
                            .to_string(),
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
                "The last start failed; the log shows why.".to_string(),
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
            Deed::Voice => {
                self.open_settings();
                self.settings_page = super::settings_window::SettingsPage::Voice;
            }
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
        // what it is about is in the sidebar and the panel
        self.sidebar_open = true;
        self.panel_open = true;
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
        // the stops the window shows now: the count is of them, and Back goes to the one shown before
        let shown: Vec<usize> = (0..TOUR.len())
            .filter(|i| spot_rect(ctx, TOUR[*i].0).is_some())
            .collect();
        let Some(n) = shown.iter().copied().find(|i| *i >= at) else {
            self.help.tour = None;
            return;
        };
        let (spot, title, said) = &TOUR[n];
        let place = shown.iter().position(|i| *i == n).unwrap_or(0);
        let before = place.checked_sub(1).map(|p| shown[p]);
        let after = shown.get(place + 1).copied();
        // the arrows and Enter go round it too
        let (next_key, back_key) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::ArrowLeft),
            )
        });
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
                            egui::RichText::new(format!("{} OF {}", place + 1, shown.len()))
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
                                    let last = after.is_none();
                                    if theme::button_sized(
                                        ui,
                                        Role::Primary,
                                        true,
                                        if last { "Done" } else { "Next" },
                                        egui::vec2(64.0, 26.0),
                                    )
                                    .on_hover_text(if last { "Enter" } else { "Enter or →" })
                                    .clicked()
                                    {
                                        go = Some(after);
                                    }
                                    if let Some(before) = before {
                                        if theme::button_sized(
                                            ui,
                                            Role::Normal,
                                            true,
                                            "Back",
                                            egui::vec2(56.0, 26.0),
                                        )
                                        .on_hover_text("←")
                                        .clicked()
                                        {
                                            go = Some(Some(before));
                                        }
                                    }
                                },
                            );
                        });
                    });
            });
        if go.is_none() && next_key {
            go = Some(after);
        }
        if go.is_none() && back_key {
            if let Some(before) = before {
                go = Some(Some(before));
            }
        }
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
        "mic" => Icon::Mic,
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
        // (the foot of the sidebar is where it settles from the second frame: the first lays it out before the panels know
        // the window's height)
        frame(&mut app, &ctx, Vec::new(), |_| ());
        let help = frame(&mut app, &ctx, Vec::new(), |ctx| spot_rect(ctx, Spot::Help))
            .expect("Help is at the foot of the sidebar (C206)");
        assert!(
            titles.borrow().is_empty(),
            "no window before it is asked for"
        );
        for events in click(help.center()) {
            frame(&mut app, &ctx, events, |_| ());
        }
        // its menu opens upward; the first item, Help, is the one with its key
        let item = text_at(&mut app, &ctx, "F1").expect("Help, with F1, in the menu of Help");
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
        // the port's pages: the one of what to do when it is taken, and the two that say where it is set (C212)
        assert_eq!(
            found("port"),
            [
                HelpPage::Configurations,
                HelpPage::Settings,
                HelpPage::WontStart
            ]
        );
        assert!(
            !found("port").contains(&HelpPage::Privacy),
            "\"report\" is not \"port\""
        );
        assert!(found("report").contains(&HelpPage::Privacy));
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
        // with a model, How it runs is there and the tour stops at its parts (C213: the configuration's box first)
        let mut app = app_in(&dir.join("model"), &["Small-Q4_K_M.gguf"]);
        frame(&mut app, &ctx, Vec::new(), |_| ());
        let change = TOUR
            .iter()
            .position(|(spot, _, _)| *spot == Spot::Configuration)
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
