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
    /// Where builds of the engine come from and where they are kept (the runtime manager).
    pub acquire: Option<Acquire>,
    #[serde(default, rename = "field")]
    pub fields: Vec<Field>,
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
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Provides {
    /// API ids the running engine serves (defined in the vocabulary).
    #[serde(default)]
    pub apis: Vec<String>,
    pub web_ui: Option<WebUi>,
    pub accepts: Option<Accepts>,
    #[serde(default)]
    pub model_name_matters: bool,
    pub default_port: Option<u16>,
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
    /// Unpack a `.zip` or `.tar.gz` and find the executable inside.
    Archive {
        binary: BinarySpec,
        /// Mark the executable as executable (Unix).
        #[serde(default)]
        chmod: bool,
    },
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
    /// Ordered ways to bring the engine up; this build knows `spawn`.
    #[serde(default)]
    pub strategies: Vec<String>,
    pub spawn: Option<Spawn>,
    pub ready: Option<Ready>,
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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopPolicy {
    #[default]
    Kill,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Ready {
    pub probe: Probe,
    /// `120s`; how long to wait before giving up.
    pub timeout: Option<String>,
    /// `500ms`; how often to ask.
    pub interval: Option<String>,
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
    /// Where among the field bindings this goes; defaults to the field's `ui.order`.
    pub order: Option<i32>,
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
