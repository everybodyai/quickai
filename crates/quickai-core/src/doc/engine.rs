//! The typed form of an `engine` document: what a runner is, how to find it, how it starts and how
//! its settings turn into command-line arguments (docs/architecture.md, section 6.1).
//!
//! These types cover the sections the current build interprets. A document written for a later
//! build may carry more (`acquire`, `runtime`, `models`, `components`, ...); those load with a
//! warning that this build ignores them, so a newer document still runs on an older application.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct EngineDoc {
    pub engine: EngineInfo,
    #[serde(default)]
    pub provides: Provides,
    #[serde(default)]
    pub locate: Locate,
    #[serde(default)]
    pub launch: Launch,
    /// Facts read from the program itself (its version).
    pub runtime: Option<Runtime>,
    /// Where builds of the engine come from and where they are kept (the runtime manager).
    pub acquire: Option<Acquire>,
    /// For an engine that keeps its own models, named references it lists, pulls and removes.
    pub models: Option<Models>,
    /// Which of the engine's APIs the Live tab reads (default: all that have a `decode`).
    pub traffic: Option<Traffic>,
    /// What the API panel tells a person who connects an app.
    pub api_info: Option<ApiInfo>,
    /// Advice to show when the log or the way the engine ended says what went wrong.
    #[serde(default, rename = "diagnostic")]
    pub diagnostics: Vec<DiagnosticRule>,
    /// What the computer has to have for some configuration of this engine to work, how to tell, and how
    /// to put it right. A configuration says which of them it needs (`[[needs]] system = "<id>"`).
    #[serde(default, rename = "prerequisite")]
    pub prerequisites: Vec<Prerequisite>,
    #[serde(default, rename = "field")]
    pub fields: Vec<Field>,
    /// How each kind of file that goes with a model (a projector) is attached to the program.
    #[serde(default)]
    pub components: std::collections::BTreeMap<String, Component>,
}

/// How one kind of companion file reaches the program (`[components.mmproj]`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Component {
    /// Where its arguments stand among the fields': after those of a lower `order` and before those of
    /// a higher one. Default 80, which is before the person's own extra arguments (90).
    pub order: Option<i32>,
    #[serde(default)]
    pub bind: ComponentBind,
    /// The folder of the models folder this kind of file is kept in, for an engine that finds its
    /// files by where they are and not by an argument (`vae`, `text_encoders`): a plain name.
    pub place: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ComponentBind {
    /// Command-line arguments, templates over `${component.path}`: the file.
    #[serde(default)]
    pub arg: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct EngineInfo {
    pub name: String,
    pub server_name: Option<String>,
    pub summary: Option<String>,
    pub license: Option<String>,
    #[serde(default)]
    pub bundled: bool,
    pub homepage: Option<String>,
    /// Hosts the engine itself contacts; feeds the privacy table.
    #[serde(default)]
    pub egress: Vec<String>,
    /// Where the engine stands in the engine selector, lowest first (default 100); engines with the same
    /// number stand in the order of their ids.
    pub order: Option<i32>,
    /// The id of the engine this program goes with, for a program that is not an engine to choose but
    /// something that is started beside one (Open WebUI beside Ollama). It is not in the engine
    /// selector and is never the engine in use; the settings of the engine it goes with offer it, it is
    /// started when that engine is ready and stopped with it, and it is its own run: when it stops
    /// unasked, the engine's run goes on. Its documents are an engine's in every other way: it is
    /// installed, located, started and given settings the way an engine is, and keeps everything in
    /// a root of its own.
    pub companion_of: Option<String>,
    /// For a companion: the id of a hidden field of its own that holds the address of the engine it goes
    /// with, as it was when the program was first started. A program that keeps the address it is first
    /// given in a database of its own, and does not read the environment for it again (Open WebUI), goes on
    /// calling that one when the engine is somewhere else later: QuickAI writes the address down in this
    /// field at the first start, says in the log when a later start would give another, and says it in the
    /// notes of a configuration that would put the engine elsewhere.
    pub first_address_field: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Provides {
    /// API ids the running engine serves (defined in the vocabulary).
    #[serde(default)]
    pub apis: Vec<String>,
    pub web_ui: Option<WebUi>,
    pub accepts: Option<Accepts>,
    #[serde(default)]
    pub model_name_matters: bool,
    pub default_port: Option<u16>,
    /// Whether the engine has to be given a model to start (llama.cpp). An engine that says `false`
    /// runs on its own: its models are used by what a client asks of it (ComfyUI's workflows), not
    /// chosen when it starts, and Start needs no model.
    #[serde(default = "yes")]
    pub needs_model: bool,
}

impl Default for Provides {
    fn default() -> Self {
        Provides {
            apis: Vec::new(),
            web_ui: None,
            accepts: None,
            model_name_matters: false,
            default_port: None,
            needs_model: true,
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
pub struct WebUi {
    pub path: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Accepts {
    #[serde(default)]
    pub formats: Vec<String>,
    /// The kinds of named reference a model can be, for an engine whose models are not files
    /// (`ollama`).
    #[serde(default)]
    pub refs: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Locate {
    /// Tried in order; the first that yields a path wins.
    #[serde(default)]
    pub strategies: Vec<LocateStrategy>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "use", rename_all = "snake_case")]
pub enum LocateStrategy {
    /// The path the user entered in a field (for example a custom binary).
    Setting { field: String },
    /// A file next to the application's own executable (the packaged layout).
    NextToExe { file: String },
    /// A path below the source checkout the application was built in (`cargo run`).
    DevTree { path: String },
    /// A build QuickAI installed itself: one directory per build under `root`, the executable at a
    /// path inside it that depends on the system.
    Installed {
        /// A template (`${data}/ollama`).
        root: String,
        /// Which of several builds.
        #[serde(default)]
        pick: Pick,
        /// `linux`, `macos`, `windows` to the executable's path inside a build.
        binary: std::collections::BTreeMap<String, String>,
    },
    /// A program of this name in a directory of the `PATH`.
    PathSearch { name: String },
    /// Places such programs are usually installed, by system. `~` and `%NAME%` are filled in from the
    /// environment.
    KnownPaths {
        #[serde(default)]
        linux: Vec<String>,
        #[serde(default)]
        macos: Vec<String>,
        #[serde(default)]
        windows: Vec<String>,
    },
}

/// Which of several installed builds an `installed` strategy takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pick {
    /// The one installed last (by the build directory's modification time: a release tag is not a
    /// reliably sortable number across every past and future format).
    #[default]
    Newest,
}

/// Facts read from the program itself.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Runtime {
    pub version: Option<VersionProbe>,
    /// How `runtime.has_flag` is answered: from what the program says about itself. Read since
    /// Phase 3 and used by the adapters of Phase 4.
    pub flags: Option<FlagsProbe>,
}

/// Run the program and read the flags it supports from its help text.
#[derive(Clone, Debug, Deserialize)]
pub struct FlagsProbe {
    /// The command: the executable (`${binary}`) and its arguments (`--help`).
    pub exec: Vec<String>,
    /// How the output is read (`help_flags`: the words that start with `--`).
    pub parse: Option<String>,
    /// Lines that contain this are left out (the tombstones of flags that were removed).
    pub skip: Option<String>,
}

/// Run the program and read its version from what it prints.
#[derive(Clone, Debug, Deserialize)]
pub struct VersionProbe {
    /// The command: the executable (`${binary}`) and its arguments.
    pub exec: Vec<String>,
    /// A regular expression whose first group is the version, looked for in standard output and
    /// then in standard error.
    pub regex: String,
    /// The context value it becomes: `version` (`runtime.version`).
    #[serde(rename = "as")]
    pub name: String,
}

/// Finding, downloading and keeping builds of the engine (docs/architecture.md, sections 6.1, 9.2
/// and 11.5). The runtime manager in the settings window is configured by this table.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Acquire {
    /// Where installed builds live, a template (`${data}/llama-bin`): one directory per build.
    pub root: Option<String>,
    pub feed: Option<Feed>,
    pub assets: Option<Assets>,
    pub install: Option<Install>,
    /// What a build's file name says about the GPU it was made for, first match wins.
    #[serde(default)]
    pub backends: Vec<Backend>,
    /// A placeholder for the box that narrows the list of builds (`e.g. vulkan, rocm, b110...`).
    pub filter_hint: Option<String>,
    /// The release this document was tested with: offered first, and the one `pins` are for.
    pub tested: Option<Tested>,
    /// Archives that are unpacked over a build on request, by platform (ROCm's libraries).
    #[serde(default)]
    pub overlays: Vec<Overlay>,
    /// The sha256 of each file of the tested release, by file name (`sha256:<hex>`).
    #[serde(default)]
    pub pins: std::collections::BTreeMap<String, String>,
}

/// The release a document was tested with.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Tested {
    pub tag: String,
}

/// Extra files of a release that are added to a build when the person chooses them: never a
/// build of their own, and off until chosen.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Overlay {
    /// What the choice is called in the document (`rocm`).
    pub id: String,
    /// What the window says about it (`AMD ROCm (adds about 1 GB)`).
    pub label: Option<String>,
    /// The file for this machine, by exact name, first match wins: a platform no rule matches has
    /// no such overlay.
    pub rules: Vec<NameRule>,
}

/// Where the list of builds comes from.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "use", rename_all = "snake_case")]
pub enum Feed {
    /// The releases of a GitHub repository, with the files attached to each.
    GithubRelease {
        /// `owner/name`.
        repo: String,
        /// How many releases to fetch.
        #[serde(default = "default_per_page")]
        per_page: u32,
        /// How many of them, with a matching file, to show.
        #[serde(default = "default_show")]
        show: usize,
        /// Whether a release GitHub marks as a prerelease counts (llama.cpp tags every build as one).
        #[serde(default)]
        prereleases: bool,
        /// The root of the API. Default `https://api.github.com`; a document that names another
        /// is how a test points a feed at a fake, so only documents QuickAI trusts may.
        api: Option<String>,
        /// The feed lists a sha256 for each file (GitHub's `digest`), and a build carries it.
        #[serde(default)]
        digest: bool,
        /// Where the source archive of a release is fetched from (`assets` of `by = "source"`):
        /// `<archive>/<owner>/<name>/archive/refs/tags/<tag>.tar.gz`. Default `https://github.com`; as
        /// with `api`, a document that names another is how a test points the install at a fake.
        archive: Option<String>,
    },
    /// The releases of a package of the Python Package Index, with the files attached to each: the
    /// feed of an application that is installed from its wheel (`assets` of `by = "wheel"`). The index
    /// lists a sha256 for every file, so a build always carries one.
    Pypi {
        /// The package, as the index names it (`open-webui`).
        package: String,
        /// How many releases, with a wheel, to show.
        #[serde(default = "default_show")]
        show: usize,
        /// Whether a release the index marks as a pre-release (`1.0rc1`) counts.
        #[serde(default)]
        prereleases: bool,
        /// The root of the index's JSON API. Default `https://pypi.org`; as with `api` of the GitHub feed,
        /// a document that names another is how a test points the look-up at a fake.
        api: Option<String>,
    },
}

fn default_per_page() -> u32 {
    10
}

fn default_show() -> usize {
    5
}

/// Which of a release's files is a build for this machine.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum Assets {
    /// By words in the file name: a name qualifies when it ends in one of `ext`, has none of the
    /// `exclude` words, and has a word for this machine's OS and one for its CPU architecture.
    Keywords {
        ext: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
        /// `linux`, `windows`, `macos` to the words that mean it in a file name.
        os: std::collections::BTreeMap<String, Vec<String>>,
        /// `x86_64`, `aarch64` to the words that mean it.
        arch: std::collections::BTreeMap<String, Vec<String>>,
        /// The directory a build is installed under, a template over `tag`, `stem` and `asset`.
        /// Default `${tag}-${stem}`.
        label: Option<String>,
    },
    /// By exact file name, chosen by the first rule whose condition holds.
    Names {
        rules: Vec<NameRule>,
        label: Option<String>,
    },
    /// The release's own source archive (`<tag>.tar.gz`) is the build, for an application that is
    /// installed from its source (ComfyUI). Every release has one, on every platform.
    Source { label: Option<String> },
    /// The release's wheel is the build, for an application that is installed from the Python Package
    /// Index (Open WebUI): the one file of the release that is a wheel for every platform
    /// (`py3-none-any`), which is installed into an environment with what it needs.
    Wheel { label: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct NameRule {
    /// A condition over the context (`os == 'linux' and arch == 'x86_64'`).
    pub when: String,
    pub name: String,
}

/// How a downloaded file becomes an installed build.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "use", rename_all = "snake_case")]
pub enum Install {
    /// Unpack a `.zip`, `.tar.gz` or `.tar.zst` and find the executable inside.
    Archive {
        /// The executable. An engine that says where its builds are in `[locate]` need not say it
        /// twice.
        binary: Option<BinarySpec>,
        /// Mark the executable as executable (Unix).
        #[serde(default)]
        chmod: bool,
        /// Whether the download is compared with a sha256 before anything is unpacked.
        #[serde(default)]
        verify: Verify,
        /// A command that must succeed on the unpacked build before it counts as installed:
        /// the executable (`${binary}`) and its arguments.
        smoke: Option<Vec<String>>,
        /// Entries of the archive that are left out, by condition on this machine.
        #[serde(default)]
        skip: Vec<SkipRule>,
    },
    /// An application written in Python, installed from its source: the source archive is unpacked
    /// into the build, a virtual environment is made beside it, the packages it needs are installed
    /// into that, and a launcher that runs the application with that environment is written. The
    /// launcher is the build's executable.
    PythonApp {
        /// The launcher the installer writes (`comfyui`; `comfyui.cmd` on Windows).
        binary: BinarySpec,
        /// The script of the unpacked source the launcher runs, relative to the source (`main.py`). An
        /// application that is installed from a wheel has no source to run a script of: it names a
        /// `console_script` instead, and one of the two is needed.
        entry: Option<String>,
        /// The program the wheel puts in the environment (`open-webui`). The installer asks the environment
        /// which function it calls (the package's own records say), and the launcher has the environment's
        /// Python call it: the script itself is never run, because it holds the path of the folder it was
        /// made in, and a build is moved to its name when it is whole.
        console_script: Option<String>,
        python: PythonSpec,
        /// Whether the source archive is compared with a sha256 before anything is unpacked.
        #[serde(default)]
        verify: Verify,
        /// A command that must succeed on the finished build before it counts as installed: the
        /// launcher (`${binary}`) and its arguments.
        smoke: Option<Vec<String>>,
        /// How long the smoke test may take (default `120s`: a program that imports PyTorch for the
        /// first time is slow to start).
        smoke_timeout: Option<String>,
        /// How much free room an install needs (`30GiB`): the download and the environment together.
        space: Option<String>,
        /// The packages of the environment, in the order they are installed.
        #[serde(default)]
        pip: Vec<PipStep>,
        /// Packages of extra nodes unpacked into the source (a custom node of ComfyUI).
        #[serde(default)]
        nodes: Vec<NodePack>,
    },
}

/// Which Python makes the virtual environment of a `python_app`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PythonSpec {
    /// The versions to look for, best first (`3.13`, `3.12`): the first one the machine has is used.
    /// A person who has another puts its path in the engine's Python field.
    pub versions: Vec<String>,
}

/// One `pip install` of the environment of a `python_app`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PipStep {
    /// Its name; a document of the person's patches or replaces a step by `id`.
    pub id: String,
    /// What the log says it is doing (default: the id).
    pub label: Option<String>,
    /// Only on a machine where this holds (`has(gpu.vendors, 'amd')`); always when left out.
    pub when: Option<String>,
    /// Packages, as requirement specifiers (`torch`, `torchvision==0.29.1`).
    #[serde(default)]
    pub packages: Vec<String>,
    /// A requirements file of the unpacked source, relative to it (`requirements.txt`).
    pub requirements: Option<String>,
    /// Installs the wheel the build is (`assets` of `by = "wheel"`), the file that was downloaded and
    /// checked, and what it needs from the index.
    #[serde(default)]
    pub wheel: bool,
    /// The index the packages come from instead of PyPI (`https://download.pytorch.org/whl/rocm7.2`).
    pub index: Option<String>,
    /// How much of the progress bar this step is, next to the others (default 1): PyTorch is most of
    /// the time an install takes, and a bar that gave every step an equal share would stand still
    /// for it.
    pub weight: Option<u32>,
}

/// A package of extra nodes that is unpacked into the source of a `python_app`, pinned by a sha256.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct NodePack {
    pub id: String,
    pub label: Option<String>,
    /// Only on a machine where this holds; always when left out.
    pub when: Option<String>,
    /// A `.tar.gz` of one commit.
    pub url: String,
    /// What the archive must hash to (`sha256:` and 64 hexadecimal digits).
    pub sha256: String,
    /// Where it goes, relative to the unpacked source (`custom_nodes/ComfyUI-GGUF`).
    pub into: String,
    /// Its requirements file, relative to the pack (`requirements.txt`); installed into the environment.
    pub requirements: Option<String>,
    /// Its licence, for the About window.
    pub license: Option<String>,
}

/// Whether an install checks what it downloaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verify {
    /// Install what arrived (llama.cpp's builds have always been installed so).
    #[default]
    None,
    /// Compare it with the digest the host lists for the file and the one the document pins; with
    /// neither, the build is installed and said to be unchecked.
    Digest,
}

/// Part of an archive to leave out of an install.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct SkipRule {
    /// Only on a machine where this holds (`gpu.known and not has(gpu.vendors, 'nvidia')`).
    pub when: Option<String>,
    /// A path inside the build, `*` standing for any run of characters inside one name
    /// (`lib/ollama/cuda_*`); what is below a directory that matches is left out with it.
    pub glob: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct BinarySpec {
    /// The executable's file name, without a platform suffix (`llama-server`).
    pub name: String,
    /// How many directory levels below the install directory to look.
    #[serde(default = "default_search_depth")]
    pub search_depth: u32,
}

fn default_search_depth() -> u32 {
    4
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Backend {
    /// A word to look for in the file name, in lower case.
    #[serde(rename = "match")]
    pub needle: String,
    /// What the interface calls this backend.
    pub label: String,
    /// The GPU vendor a build for this backend needs (`nvidia`, `amd`); a mismatch is flagged.
    pub needs_gpu: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Launch {
    /// Ordered ways to bring the engine up: `attach_if_running` and `spawn`.
    #[serde(default)]
    pub strategies: Vec<String>,
    /// A `choice` field (`auto`, `attach`, `spawn`) that picks among the strategies: `auto` tries
    /// them in order, the others only the one they name.
    pub strategy_field: Option<String>,
    /// An engine that may already be running: how to recognise it and whether it is still there.
    pub attach: Option<Attach>,
    pub spawn: Option<Spawn>,
    pub ready: Option<Ready>,
    /// Calls made once the engine answers, to load the model (only when there is one).
    #[serde(default)]
    pub warm: Vec<Step>,
    /// Calls made before the engine is stopped, to free what the model holds.
    #[serde(default)]
    pub unload: Vec<Step>,
}

/// Using an engine that was already running.
#[derive(Clone, Debug, Deserialize)]
pub struct Attach {
    /// A template (`127.0.0.1:${public_port}`): where it would be.
    pub endpoint: String,
    /// How to tell that what answers there is this engine and not something else.
    pub identity: Identity,
    /// How to notice that it has gone, since there is no process of ours to watch.
    pub liveness: Option<Liveness>,
}

/// A request that only the engine answers in a known way.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Identity {
    pub path: String,
    /// The JSON pointer of a string in the answer (`/version`): what the engine says it is.
    pub pointer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Liveness {
    pub path: String,
    /// `5s`: how often to ask.
    pub every: Option<String>,
    /// How many questions in a row may go unanswered before the engine is taken to be gone.
    pub failures: Option<u32>,
}

/// One call of a phase, made only when `when` holds.
#[derive(Clone, Debug, Deserialize)]
pub struct Step {
    /// A condition over the context (`model`).
    pub when: Option<String>,
    pub call: CallSpec,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CallSpec {
    /// Default `POST`.
    pub method: Option<String>,
    pub path: String,
    /// The JSON body: tables, lists and values, every text of it a template.
    pub body: Option<toml::Value>,
    /// `600s`: how long the call may take.
    pub timeout: Option<String>,
    /// A failure of this call is logged and not acted on (an unload that does not answer must not
    /// hold up a stop).
    #[serde(default)]
    pub best_effort: bool,
    /// How the reply is read: `json` (the default: one body), `ndjson` or `sse` (a stream of events).
    /// The calls of `[models]` may stream; a warm-up or an unload is read whole.
    pub stream: Option<String>,
}

/// How an engine that keeps its own models lists, pulls and removes them, and what the model box
/// makes of what was typed (docs/phase3-test-plan.md, group M).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Models {
    /// The calls need the engine running: stopped, only what it listed last time can be shown, and
    /// a pull or a removal starts it with no model loaded.
    #[serde(default)]
    pub needs_server: bool,
    pub list: Option<ModelList>,
    pub pull: Option<ModelPull>,
    pub remove: Option<ModelRemove>,
    /// Asks about one model (what it can take as input).
    pub capabilities: Option<ModelCapabilities>,
    /// What the model box makes of what was typed: the first rule whose `when` holds.
    #[serde(default)]
    pub input_rules: Vec<InputRule>,
    /// For an engine whose models are files: where they are kept (Phase 4 reads these).
    #[serde(default)]
    pub sources: Vec<ModelSource>,
    /// For an engine whose models are files: how one gets into the folder (Phase 4).
    pub acquire: Option<ModelAcquire>,
    /// Looking models up online.
    #[serde(default)]
    pub lookup: Vec<ModelLookup>,
    /// What is fetched together with a model's weights (a vision model's projector).
    #[serde(default)]
    pub bundle: Vec<ModelBundle>,
    /// The ways the engine can be started for a kind of model (`embedding`), which a catalog names
    /// and a record beside the weights remembers.
    #[serde(default)]
    pub mode: Vec<ModelMode>,
}

/// A way the engine is started for a model (`[[models.mode]]`): the arguments that make it serve
/// that kind of model. A catalog, which is data, names a mode by its id and never carries the
/// arguments itself.
#[derive(Clone, Debug, Deserialize)]
pub struct ModelMode {
    pub id: String,
    /// What the model picker says of a model that runs so (default: the id).
    pub label: Option<String>,
    pub help: Option<String>,
    /// The words added to the command line: plain text, no templates.
    #[serde(default)]
    pub arg: Vec<String>,
}

/// A way to fetch a model with the files that go with it (`[[models.bundle]]`).
#[derive(Clone, Debug, Deserialize)]
pub struct ModelBundle {
    pub id: String,
    /// The words of its row of buttons: `with projector` (default: the id).
    pub label: Option<String>,
    pub help: Option<String>,
    /// The files beside the weights, one kind each; the bundle is offered for a repository that has
    /// a file for every part.
    #[serde(default)]
    pub parts: Vec<BundlePart>,
}

/// One part of a bundle: a kind of file and how to find it among the files of a repository.
#[derive(Clone, Debug, Deserialize)]
pub struct BundlePart {
    /// The kind of component, which `[components.<kind>]` says how to attach.
    pub kind: String,
    /// What the window calls it: `projector` (default: the kind).
    pub label: Option<String>,
    /// Patterns (`*` any run, `?` one character, case ignored) a file's name or path matches.
    #[serde(default, rename = "match")]
    pub matches: Vec<String>,
    /// The quantizations to take first, best first; else the smallest file.
    #[serde(default)]
    pub prefer: Vec<String>,
}

/// A place models are found: a folder shipped with the application, or one the person can write to.
#[derive(Clone, Debug, Deserialize)]
pub struct ModelSource {
    #[serde(rename = "use")]
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub ext: Vec<String>,
    #[serde(default)]
    pub writable: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModelAcquire {
    #[serde(rename = "use")]
    pub kind: String,
    pub dir: Option<String>,
    /// The folder of `dir` the weights of a model go in (`diffusion_models`), for an engine that
    /// finds its files by the folder they are in; a plain name. The recommended list may say another
    /// for a model.
    pub place: Option<String>,
    /// The other folders of `dir` a file can be put in by hand, for the same kind of engine: what the
    /// window offers a person who gives a link or a file of their own and says where it goes. Plain
    /// names; `place` and the folder of each `[components.<kind>]` are offered too.
    #[serde(default)]
    pub places: Vec<String>,
}

/// One way of looking models up online: the primitive that does it and what it is told.
#[derive(Clone, Debug, Deserialize)]
pub struct ModelLookup {
    pub id: String,
    #[serde(rename = "use")]
    pub kind: String,
    /// What the primitive named by `use` is told: its own keys, checked by it (Phase 4).
    #[serde(flatten)]
    pub settings: toml::Table,
}

/// The call that lists the models, where each one is in the reply, and where the last reply is kept.
#[derive(Clone, Debug, Deserialize)]
pub struct ModelList {
    pub call: CallSpec,
    /// The array of models in the reply, as a JSON pointer (`/models`).
    pub items: String,
    pub map: ModelMap,
    /// Where the last reply is kept (a template), so something can be shown while the engine is not
    /// running.
    pub cache: Option<String>,
    /// Names the engine lists that are its own bookkeeping, not models to pick: patterns over the
    /// whole name (`llamacpp:.*`). The picker, the recommendation and the cache never see them.
    #[serde(default)]
    pub hide: Vec<String>,
}

/// Where the parts of one listed model are, as JSON pointers into it.
#[derive(Clone, Debug, Deserialize)]
pub struct ModelMap {
    /// The name the engine knows it by, which is what a request names. Required.
    pub id: String,
    pub display: Option<String>,
    pub size_bytes: Option<String>,
    pub digest: Option<String>,
    pub capabilities: Option<String>,
}

/// The call that pulls a model by name, with `${ref}` the name, and where its progress is.
#[derive(Clone, Debug, Deserialize)]
pub struct ModelPull {
    pub call: CallSpec,
    pub progress: Option<PullProgressSpec>,
}

/// Where the progress of a pull is in the events of its stream (JSON pointers). Pulls add layers up
/// as the engine announces them, so a layer is told apart by `key`.
#[derive(Clone, Debug, Deserialize)]
pub struct PullProgressSpec {
    pub key: String,
    pub completed: String,
    pub total: String,
    pub status: String,
    /// Where an event says the pull failed, whatever the HTTP status was (default `/error`).
    pub error: Option<String>,
    /// The `status` text that ends the pull (default `success`).
    pub done: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModelRemove {
    pub call: CallSpec,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModelCapabilities {
    pub call: CallSpec,
    /// Where the list of what the model can do is in the reply.
    pub pointer: String,
}

/// The APIs whose traffic the Live tab reads.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Traffic {
    /// Ids from a vocabulary; each must have a `decode` there to be read.
    #[serde(default)]
    pub apis: Vec<String>,
}

/// What the API panel says: the address to give an app, what to put in the key box, and which
/// model name to use.
#[derive(Clone, Debug, Deserialize)]
pub struct ApiInfo {
    /// A template: the base URL of the OpenAI-compatible API (`http://127.0.0.1:${public_port}/v1`).
    pub base_url: String,
    /// More addresses worth showing, such as the engine's own API.
    #[serde(default)]
    pub extra_urls: Vec<ExtraUrl>,
    pub api_key: Option<ApiNote>,
    pub model: Option<ApiNote>,
    /// The id of the API whose curl example is shown (`openai.chat`).
    pub example: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExtraUrl {
    pub label: String,
    /// A template.
    pub url: String,
}

/// A value to show and a note under it. The value is a template.
#[derive(Clone, Debug, Deserialize)]
pub struct ApiNote {
    pub value: String,
    pub note: Option<String>,
}

/// One piece of advice: shown when `when` holds for a run that went wrong.
#[derive(Clone, Debug, Deserialize)]
pub struct DiagnosticRule {
    pub when: DiagnosticWhen,
    /// A template over the engine's context (`${field.port}`).
    pub hint: String,
    /// A button under the hint: what a click does (`focus_field:port`, `open:settings.runtime`).
    pub action: Option<String>,
}

/// When advice applies: the log says something, or the engine ended in a certain phase.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DiagnosticWhen {
    /// A regular expression looked for in the log lines.
    pub log_matches: Option<String>,
    /// The run ended while it was in this state (`starting`).
    pub exited_during: Option<String>,
}

/// One thing the computer has to have for a configuration of the engine to work: a program, a header
/// file of Python, a driver of a graphics card, or a small test that the environment of the engine passes.
/// It says how to tell whether the computer has it and, for what a package manager can bring, how to
/// put it right. Whether a configuration needs it is for the configuration to say; the packages are for
/// the document to name, because a package that is installed as the administrator must not come from a
/// file that was given away.
#[derive(Clone, Debug, Deserialize)]
pub struct Prerequisite {
    /// What a configuration calls it (`[[needs]] system = "c_compiler"`): letters, digits and `_`.
    pub id: String,
    /// What a sentence calls it (`a C compiler`).
    pub label: String,
    /// Only on a computer where this holds (`os == 'linux'`); always when left out.
    pub when: Option<String>,
    /// How to tell.
    pub check: PrerequisiteCheck,
    /// The packages that bring it, by package manager.
    #[serde(default)]
    pub fix: Fix,
    /// What to tell the person when nothing here can put it right, or when it did not help: a sentence.
    pub hint: Option<String>,
}

/// How a prerequisite is checked: a closed list, because each is a thing the program does to the computer.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "use", rename_all = "snake_case")]
pub enum PrerequisiteCheck {
    /// One of the programs is on the computer (on the `PATH`), or the one a variable names is.
    Program {
        /// Program names, any of which will do (`["gcc", "clang"]`).
        any_of: Vec<String>,
        /// A variable that names the program instead (`CC`), as the tool that needs it reads it.
        env: Option<String>,
    },
    /// A header file of Python is in the folder of Python's headers, for the Python the engine's
    /// environment is made with. It is a check of a `python_app` install, and is made before the install,
    /// with the Python that install will use.
    PythonHeader {
        /// The file (`Python.h`).
        file: String,
    },
    /// A module of Python's own library can be imported by the Python the engine's environment is made with
    /// (`ensurepip`, which Debian and Ubuntu leave out of the Python and put in the package `python3.12-venv`:
    /// without it no environment can be made). A check of a `python_app` install, made before the install with the
    /// Python the install will use; an environment that exists was made, so it passes.
    PythonModule {
        /// The module (`ensurepip`).
        module: String,
    },
    /// The driver of an NVIDIA card is of this version or a newer.
    NvidiaDriver {
        /// The version (`580`, or `580.65`): compared number by number.
        at_least: String,
    },
    /// A script of the application's own files, run by the Python of the engine's environment, in the
    /// environment the engine is started with. It passes when it ends with status 0. It is a test that the
    /// installed environment works, so it is run after the install and before the engine is started.
    PythonScript {
        /// The script (`comfyui/selftest/triton_kernel.py`).
        asset: String,
        /// How long it may take (`180s`; default `120s`).
        timeout: Option<String>,
    },
}

/// The packages that bring a prerequisite, by package manager. In a name, `${python.version}` is the
/// version of the Python of the environment (`3.12`) and `${python.digits}` the same without the dot
/// (`312`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Fix {
    /// Debian and Ubuntu (`apt-get`).
    #[serde(default)]
    pub apt: Vec<String>,
    /// Fedora and Red Hat (`dnf`).
    #[serde(default)]
    pub dnf: Vec<String>,
    /// openSUSE (`zypper`).
    #[serde(default)]
    pub zypper: Vec<String>,
    /// Arch (`pacman`).
    #[serde(default)]
    pub pacman: Vec<String>,
    /// Alpine (`apk`).
    #[serde(default)]
    pub apk: Vec<String>,
}

/// The package managers a document names packages for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PackageManager {
    Apt,
    Dnf,
    Zypper,
    Pacman,
    Apk,
}

impl PackageManager {
    /// In the order they are looked for.
    pub const ALL: [PackageManager; 5] = [
        PackageManager::Apt,
        PackageManager::Dnf,
        PackageManager::Zypper,
        PackageManager::Pacman,
        PackageManager::Apk,
    ];

    /// The program, as it is typed.
    pub fn program(self) -> &'static str {
        match self {
            PackageManager::Apt => "apt-get",
            PackageManager::Dnf => "dnf",
            PackageManager::Zypper => "zypper",
            PackageManager::Pacman => "pacman",
            PackageManager::Apk => "apk",
        }
    }

    /// How long, in seconds, a program that installs waits for a lock that another process holds (the upgrades
    /// a computer that is rented and left runs by itself, in its first hours above all) before it gives up with
    /// the manager's own words.
    pub const LOCK_WAIT_SECONDS: u64 = 120;

    /// The commands that install `packages`, each a program and its words, in the order they are run: where
    /// the manager keeps a list of packages that goes stale on a computer that is rented and left (apt's), it
    /// is refreshed first. Nothing asks a question. These are the words a person types.
    pub fn install_commands(self, packages: &[String]) -> Vec<Vec<String>> {
        self.commands(packages, false)
    }

    /// The same commands as a program runs them. Where the manager can be told to wait for a lock another
    /// process holds, instead of ending at once, the install is told to: apt's `-o DPkg::Lock::Timeout=120`
    /// (measured with apt 2.8 and a process holding `/var/lib/dpkg/lock-frontend`: without the option
    /// `apt-get install` ends at once with code 100 and `Could not get lock`; with it, it prints `Waiting for
    /// cache lock` once a second and goes on when the lock is let go, or ends with `Unable to acquire the dpkg
    /// frontend lock` when the time is up). It is on the install only: the refresh takes the lock of the
    /// lists, which the option does not cover (with that lock held it ends at once, with the option or
    /// without, and the install goes on with the list there is). A person who types the command sees the
    /// manager's refusal and tries again, so `install_commands` does not carry the option. The other managers
    /// are run as they are printed: what they do at a held lock was not looked at.
    pub fn install_commands_to_run(self, packages: &[String]) -> Vec<Vec<String>> {
        self.commands(packages, true)
    }

    fn commands(self, packages: &[String], waiting: bool) -> Vec<Vec<String>> {
        let words = |head: &[&str]| -> Vec<String> {
            head.iter()
                .map(|w| w.to_string())
                .chain(packages.iter().cloned())
                .collect()
        };
        match self {
            PackageManager::Apt => {
                let wait = format!("DPkg::Lock::Timeout={}", Self::LOCK_WAIT_SECONDS);
                let install = if waiting {
                    words(&["apt-get", "install", "-y", "-o", &wait])
                } else {
                    words(&["apt-get", "install", "-y"])
                };
                vec![vec!["apt-get".to_string(), "update".to_string()], install]
            }
            PackageManager::Dnf => vec![words(&["dnf", "install", "-y"])],
            PackageManager::Zypper => vec![words(&["zypper", "--non-interactive", "install"])],
            PackageManager::Pacman => {
                vec![words(&["pacman", "-S", "--needed", "--noconfirm"])]
            }
            PackageManager::Apk => vec![words(&["apk", "add"])],
        }
    }
}

impl Fix {
    /// The packages for one manager, as the document names them.
    pub fn packages(&self, manager: PackageManager) -> &[String] {
        match manager {
            PackageManager::Apt => &self.apt,
            PackageManager::Dnf => &self.dnf,
            PackageManager::Zypper => &self.zypper,
            PackageManager::Pacman => &self.pacman,
            PackageManager::Apk => &self.apk,
        }
    }

    /// Whether the document names no package at all.
    pub fn is_empty(&self) -> bool {
        PackageManager::ALL
            .iter()
            .all(|m| self.packages(*m).is_empty())
    }
}

/// One rule of the model box: when `when` holds (always, when it is left out) the input is pulled as
/// `ref` or refused with `message`.
#[derive(Clone, Debug, Deserialize)]
pub struct InputRule {
    /// A condition over `input` (the text typed, without the spaces around it), with `matches`,
    /// `starts_with`, `is_url` and `is_file`.
    pub when: Option<String>,
    /// `pull` or `reject`.
    #[serde(rename = "do")]
    pub action: String,
    /// For `pull`: the name to pull, a template over `input` (`${input}`).
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// For `reject`: what to tell the person.
    pub message: Option<String>,
    /// For `search` (Phase 4): the lookup that answers it.
    pub provider: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Spawn {
    /// A template: the program to start.
    pub program: String,
    /// Templates, one argument each. Fields append their own `bind.arg` after these.
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<EnvEntry>,
    /// Start in its own process group (Phase 3).
    #[serde(default)]
    pub group: bool,
    #[serde(default)]
    pub stop: StopSpec,
    /// The folder the program starts in, a template. Without it the program starts wherever QuickAI
    /// was started from, which is the person's, and a program that writes a file to its working
    /// directory (Open WebUI keeps its secret key there) would write it there. It is made with the
    /// folders of `mkdir` if it is not there.
    pub cwd: Option<String>,
    /// Folders made, with the ones above them, before the program starts: templates. For a program
    /// that refuses a folder it is told about that does not exist yet (ComfyUI's `--models-directory`).
    #[serde(default)]
    pub mkdir: Vec<String>,
    /// Files of the application's own that are put where the program is told to read them, before it
    /// starts: for a program that has a folder of extensions (ComfyUI's custom nodes).
    #[serde(default)]
    pub write: Vec<WriteSpec>,
}

/// One file or folder of the application's own, and where it is put.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct WriteSpec {
    /// A name in the application's own files, `comfyui/quickai_bridge`: a file, or a folder with everything in it.
    pub asset: String,
    /// A template: where it is put. What is already there and the same is left alone.
    pub to: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct EnvEntry {
    pub name: String,
    /// Set the variable to this template.
    pub value: Option<String>,
    /// Put this template in front of the inherited value, joined with `sep`.
    pub prepend: Option<String>,
    pub sep: Option<String>,
    /// Only when this expression holds.
    pub when: Option<String>,
    #[serde(default)]
    pub locked: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct StopSpec {
    #[serde(default)]
    pub policy: StopPolicy,
    /// `10s`: how long a graceful stop waits for the program to exit before it is killed.
    pub grace: Option<String>,
    /// `2s`: the same wait when the application itself is closing, so closing is not held up.
    pub exit_grace: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopPolicy {
    /// Kill it at once (one process, nothing to clean up).
    #[default]
    Kill,
    /// Ask it to stop (SIGTERM to its process group), wait the grace period, then kill what is left.
    Graceful,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Ready {
    pub probe: Probe,
    /// `120s`; how long to wait before giving up.
    pub timeout: Option<String>,
    /// `500ms`; how often to ask.
    pub interval: Option<String>,
    /// What a timeout means.
    #[serde(default)]
    pub on_timeout: OnTimeout,
}

/// What happens when the engine has not answered within the ready timeout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnTimeout {
    /// Give up: say so, stop what was started and end in `Crashed`.
    #[default]
    Fail,
    /// Say so and go on waiting for the process (it may still be loading a big model); only its
    /// exit, or a Stop, ends the wait.
    Wait,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "use", rename_all = "snake_case")]
pub enum Probe {
    Http {
        path: String,
        /// The status that means "ready" (default 200).
        expect: Option<u16>,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct Field {
    pub id: String,
    pub label: Option<String>,
    pub help: Option<String>,
    #[serde(rename = "type")]
    pub ty: FieldType,
    pub range: Option<(f64, f64)>,
    pub step: Option<f64>,
    pub default: Option<FieldDefault>,
    #[serde(default)]
    pub optional: bool,
    /// What re-derives the default (`model`: picking another model).
    #[serde(default)]
    pub recompute_on: Vec<String>,
    /// The `settings.json` key this field keeps for compatibility.
    pub legacy_key: Option<String>,
    pub visible_when: Option<String>,
    /// The field can be edited only while this holds (`not session.running`).
    pub enabled_when: Option<String>,
    /// The field must have a value while this holds.
    pub required_when: Option<String>,
    /// The values a `choice` field offers.
    #[serde(default)]
    pub options: Vec<String>,
    /// A regular expression the whole text value must match.
    pub pattern: Option<String>,
    /// Checks that may look at other fields; the first that fails is the field's error.
    #[serde(default)]
    pub validate: Vec<Rule>,
    /// Where the value is remembered.
    #[serde(default)]
    pub persist: Persist,
    /// Whether the value travels in a configuration (a saved, shareable runtime): `yes`, `review`
    /// (it does, and a person who is given a file with it is asked) or `never`. Without it the type
    /// decides (`Field::share`), so that a field added later is private until a document says
    /// otherwise.
    #[serde(default)]
    pub share: Option<Share>,
    /// The sentence a person is told when this field is asked about or left out of a configuration.
    pub share_note: Option<String>,
    /// When a change takes effect.
    #[serde(default)]
    pub applies: Applies,
    #[serde(default)]
    pub bind: Bind,
    /// Named facts for expressions: `gpu_offload = "value > 0"`.
    #[serde(default)]
    pub derive: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub ui: FieldUi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Int,
    Float,
    Bool,
    Text,
    Choice,
    Port,
    Dir,
    File,
    Url,
    RuntimePath,
    KvLines,
    ModelRef,
    Hidden,
}

/// A literal, or a named function over context values.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum FieldDefault {
    Computed {
        #[serde(rename = "fn")]
        func: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Literal(toml::Value),
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Bind {
    /// This field is the port the engine listens on. `public` marks the one clients use.
    pub port: Option<PortRole>,
    /// Argument templates added to the command line; `${value}` is the field's value.
    pub arg: Option<Vec<String>>,
    /// The value is a shell-like quoted string of arguments (nothing is added when it is empty).
    #[serde(default)]
    pub arg_split: bool,
    /// For a `choice` field: the arguments each option adds (plain words, not templates); an option
    /// that is not listed adds none.
    #[serde(default)]
    pub arg_by_value: std::collections::BTreeMap<String, Vec<String>>,
    /// Where among the field bindings this goes; defaults to the field's `ui.order`.
    pub order: Option<i32>,
    /// Environment variables of the engine this field decides: each is set when its `when` holds
    /// (`value` is the field's value), and none is set while the field has no value.
    #[serde(default)]
    pub env: Vec<EnvEntry>,
    /// For a `kv_lines` field: its lines are environment variables of the engine, except the
    /// protected ones.
    pub env_lines: Option<EnvLines>,
}

/// What a field of lines may and may not set.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct EnvLines {
    /// Names the person's lines may not set (`OLLAMA_HOST`): their value is the document's.
    #[serde(default)]
    pub protect: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortRole {
    Public,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FieldUi {
    pub section: Option<String>,
    pub order: Option<i32>,
    pub widget: Option<String>,
    /// Greyed text shown in an empty text box.
    pub hint: Option<String>,
    /// What a click on a `button` makes the application do (`open:settings`).
    pub action: Option<String>,
}

/// A cross-field check: `check` must hold, or `message` is the field's error.
#[derive(Clone, Debug, Deserialize)]
pub struct Rule {
    pub check: String,
    pub message: String,
}

/// Where a field's value is remembered (docs/architecture.md, section 6.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Persist {
    /// One value for the application.
    #[default]
    Global,
    /// One value per model.
    PerModel,
    /// Part of a saved profile.
    Profile,
    /// Forgotten when the application closes.
    Session,
}

/// Whether the value of a field travels in a configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Share {
    /// It is saved and given away as it is.
    Yes,
    /// It is saved and given away, and a person who is given a file that has it is shown it and asked:
    /// it becomes a command-line word, an environment line or an address, or it widens what a program
    /// may do.
    Review,
    /// It belongs to a computer or to a person (a folder, a program) and is never in a configuration.
    Never,
}

impl Share {
    /// The word a document uses.
    pub fn name(self) -> &'static str {
        match self {
            Share::Yes => "yes",
            Share::Review => "review",
            Share::Never => "never",
        }
    }
}

/// When a change to a field takes effect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Applies {
    /// At the next start of the engine.
    #[default]
    OnStart,
    /// At the next start, and only when QuickAI starts the engine itself (an engine it merely
    /// attached to keeps the environment its owner gave it).
    SpawnOnly,
    /// At once, even while the engine runs.
    Live,
}

impl Field {
    /// The position of this field's bindings among the arguments: `bind.order`, else `ui.order`,
    /// else last.
    pub fn arg_order(&self) -> i32 {
        self.bind.order.or(self.ui.order).unwrap_or(i32::MAX)
    }

    /// Whether the value travels in a configuration: what the document says, else what the type
    /// says. Numbers, ports, choices and flags do; text, addresses and lines of environment do and
    /// are asked about, because they reach a command line, an environment or a host; folders, files
    /// and programs belong to a computer and never do. A hidden value or a model reference is not a
    /// setting (the model has a section of its own), and a button holds nothing: `None`.
    pub fn share(&self) -> Option<Share> {
        if self.ty == FieldType::Hidden || self.ty == FieldType::ModelRef {
            return None;
        }
        Some(self.share.unwrap_or(match self.ty {
            FieldType::Int
            | FieldType::Float
            | FieldType::Bool
            | FieldType::Choice
            | FieldType::Port => Share::Yes,
            FieldType::Text | FieldType::Url | FieldType::KvLines => Share::Review,
            FieldType::Dir | FieldType::File | FieldType::RuntimePath => Share::Never,
            FieldType::Hidden | FieldType::ModelRef => return None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> EngineDoc {
        toml::from_str(text).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_small_engine_document_parses() {
        let doc = parse(
            r#"
            [engine]
            name = "llama.cpp"
            [provides]
            apis = ["openai.chat"]
            web_ui = { path = "/", label = "Open Chat UI" }
            [locate]
            strategies = [
              { use = "setting", field = "bin_override" },
              { use = "next_to_exe", file = "llama-server" },
              { use = "dev_tree", path = "llama.cpp/build/bin/llama-server" },
            ]
            [launch]
            strategies = ["spawn"]
            [launch.spawn]
            program = "${runtime.binary}"
            args = ["--model", "${model.main.path}"]
            env = [ { name = "LD_LIBRARY_PATH", prepend = "${runtime.dir}", sep = ":" } ]
            [launch.ready]
            probe = { use = "http", path = "/health", expect = 200 }
            timeout = "120s"
            interval = "500ms"
            [[field]]
            id = "port"
            type = "port"
            default = 8080
            bind.port = "public"
            [[field]]
            id = "ctx_size"
            type = "int"
            range = [256, 131072]
            default = { fn = "recommend_ctx_size", args = ["model.size_bytes", "sys.ram_bytes"] }
            recompute_on = ["model"]
            bind.arg = ["--ctx-size", "${value}"]
            ui = { section = "primary", order = 20 }
            "#,
        );
        assert_eq!(doc.engine.name, "llama.cpp");
        assert_eq!(doc.locate.strategies.len(), 3);
        assert_eq!(
            doc.locate.strategies[0],
            LocateStrategy::Setting {
                field: "bin_override".into()
            }
        );
        let spawn = doc.launch.spawn.as_ref().unwrap();
        assert_eq!(spawn.args, vec!["--model", "${model.main.path}"]);
        assert_eq!(spawn.env[0].prepend.as_deref(), Some("${runtime.dir}"));
        assert_eq!(
            doc.launch.ready.as_ref().unwrap().probe,
            Probe::Http {
                path: "/health".into(),
                expect: Some(200)
            }
        );
        assert_eq!(doc.fields.len(), 2);
        assert_eq!(doc.fields[0].bind.port, Some(PortRole::Public));
        assert!(matches!(
            doc.fields[0].default,
            Some(FieldDefault::Literal(_))
        ));
        match &doc.fields[1].default {
            Some(FieldDefault::Computed { func, args }) => {
                assert_eq!(func, "recommend_ctx_size");
                assert_eq!(args.len(), 2);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(doc.fields[1].range, Some((256.0, 131072.0)));
        assert_eq!(doc.fields[1].arg_order(), 20);
    }

    #[test]
    fn arg_order_prefers_bind_then_ui_then_last() {
        let doc = parse(
            r#"
            [engine]
            name = "x"
            [[field]]
            id = "a"
            type = "int"
            bind.order = 5
            ui.order = 9
            [[field]]
            id = "b"
            type = "int"
            ui.order = 9
            [[field]]
            id = "c"
            type = "int"
            "#,
        );
        assert_eq!(doc.fields[0].arg_order(), 5);
        assert_eq!(doc.fields[1].arg_order(), 9);
        assert_eq!(doc.fields[2].arg_order(), i32::MAX);
    }

    #[test]
    fn unknown_strategies_and_types_are_refused() {
        let bad_strategy: Result<EngineDoc, _> =
            toml::from_str("[engine]\nname='x'\n[locate]\nstrategies=[{use='telepathy'}]\n");
        assert!(bad_strategy.is_err());
        let bad_type: Result<EngineDoc, _> =
            toml::from_str("[engine]\nname='x'\n[[field]]\nid='a'\ntype='quaternion'\n");
        assert!(bad_type.is_err());
    }
}
