//! What the provisioner tells its caller, and how it ends (docs/headless.md, section 5).
//!
//! The caller is a program with no window: it prints these, as sentences for a person or as one JSON object a
//! line for a script. Nothing here is a string a program has to parse out of another string: every event is
//! typed, and the ones that are sentences say so.

use serde::{Deserialize, Serialize};

/// One thing the provisioner has to say, in the order it happens.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// A sentence for the person, in the words the window's log would use.
    Say { text: String },
    /// What this computer is, as far as what is decided here depends on it.
    Computer {
        system: String,
        memory_bytes: u64,
        /// The vendors of the cards found (`NVIDIA`), none when none was found or none could be told.
        gpus: Vec<String>,
        /// How the cards were found: `detected`, or `named` when a person said (`--gpu`).
        gpus_from: String,
    },
    /// One note of the review of the configuration for this computer.
    Review { level: String, text: String },
    /// What was said about this computer besides the configuration, and where: an environment file, the
    /// command line, a variable. A secret is not shown.
    Environment { items: Vec<EnvironmentItem> },
    /// What the computer has to have for the configuration, and whether it has it (docs/headless.md,
    /// section 12). Said once before the plan, and again after packages were installed.
    Prerequisites { items: Vec<PrerequisiteInfo> },
    /// What will be done, in order, before anything is.
    Plan { steps: Vec<StepInfo> },
    /// A step begins.
    Step {
        index: usize,
        of: usize,
        text: String,
    },
    /// How far a step has got: bytes when it counts bytes, and what it is doing now when it says.
    Progress {
        index: usize,
        done: u64,
        total: Option<u64>,
        detail: Option<String>,
    },
    /// A step is over.
    StepDone { index: usize },
    /// A test of the installed environment, run before the engine is started: it is `running`, then
    /// `passed` or `failed`; what it printed comes with the end.
    SelfTest {
        id: String,
        label: String,
        state: String,
        output: Vec<String>,
    },
    /// A program that is run changed state (`Ollama`, `starting`).
    State { program: String, state: String },
    /// A line a program that is run wrote.
    Log { program: String, line: String },
    /// Everything that was asked for runs.
    Ready {
        endpoints: Vec<EndpointInfo>,
        model: Option<String>,
    },
    /// The programs are being stopped.
    Stopping,
    /// The ports the run will listen on, and whether another program has one now (said by `check`; docs/remote-runs.md
    /// section 3.2).
    Ports { items: Vec<PortUse> },
    /// The end.
    Finished {
        outcome: String,
        message: Option<String>,
        /// What it lacks that another way can be proposed for, when that is why it failed.
        #[serde(skip_serializing_if = "Option::is_none")]
        lack: Option<Lack>,
    },
}

/// A port the run will listen on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortUse {
    pub port: u16,
    /// Whose port it is (`Ollama`, `Open WebUI`).
    pub program: String,
    /// Another program listens on it now.
    pub taken: bool,
    /// What answers there says it is, when it answers as the engine does (`Ollama 0.12.3`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answers_as: Option<String>,
}

/// What a run lacks that another way can be proposed for (docs/remote-runs.md, section 6): the window that runs a
/// configuration on another computer reads it from the end of `check`, and offers the way to accept or reject.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "lack", rename_all = "snake_case")]
pub enum Lack {
    /// A model file that is not there, and that nobody says where to get.
    Model { file: String },
    /// The folder the models go in has not room for what the plan fetches.
    Room { bytes: u64, dir: String, free: u64 },
    /// Which build of the engine's program to install could not be told for this computer.
    Build { program: String },
    /// The file of the recommended list's model is more than this computer's memory holds, and a smaller file of the
    /// same model is not (`--model-file` chooses it).
    Memory {
        file: String,
        bytes: u64,
        memory: u64,
        smaller: String,
        smaller_bytes: u64,
    },
}

/// One thing that was said about this computer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EnvironmentItem {
    /// `values.<engine>.<field>`, `env.<program>.<NAME>`, `fetch.<what>`, `models.<file>`.
    pub key: String,
    /// What it is; `None` for a secret.
    pub value: Option<String>,
    /// Where it was said: `environment file x.toml`, `--set`, `HF_TOKEN`.
    pub from: String,
}

/// One thing the configuration needs from the computer, and what looking at it found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrerequisiteInfo {
    /// What the configuration calls it (`c_compiler`).
    pub id: String,
    /// What a sentence calls it (`a C compiler`).
    pub label: String,
    /// `met`, `missing`, `unchecked` (it could not be looked at), or `later` (a test of the installed
    /// environment, run after the install).
    pub state: String,
    /// What was found, or what was looked for.
    pub detail: String,
    /// The configuration's words about why it needs it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The command that brings it, when a package manager of this computer can.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// What the document says to do when nothing here can put it right.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// One step of the plan, as a person reads it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StepInfo {
    /// `install`, `fetch`, `link`, `start`, `pull`, `load` or `companion`.
    pub kind: String,
    pub text: String,
    /// What it will download, when that is known.
    pub bytes: Option<u64>,
    /// The files a step of `fetch` or `link` puts in the models folder, one by one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileInfo>,
}

/// A file of a step: its name, where it comes from, where it goes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FileInfo {
    pub name: String,
    /// `fetch` (from the address in `from`) or `link` (the file at the path in `from` is used where it is).
    pub how: String,
    /// An address, or a path of this computer.
    pub from: String,
    /// Where it is kept.
    pub to: String,
    pub bytes: Option<u64>,
    pub sha256: Option<String>,
}

/// An address a program that runs can be reached at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EndpointInfo {
    /// `api`, `page`.
    pub role: String,
    pub program: String,
    pub url: String,
}

/// Why it did not go as asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// The configuration could not be read, or its engine is not one this version has.
    Unreadable,
    /// A note of the review stops it on this computer.
    Stopped,
    /// The file asks for settings that widen what a program may do, and the person has not said yes.
    Asks,
    /// What it needs cannot be had: no build this computer can run, a model nobody says where to get.
    Needs,
    /// An install or a download failed.
    Fetch,
    /// A program would not start, or was not ready.
    Start,
    /// A program stopped by itself after it was started.
    Crashed,
    /// A word of the command line or a line of an environment file does not make sense for this
    /// configuration (a setting it has not, a file it does not fetch).
    Mistake,
    /// The configuration needs something from whoever runs it, and nobody has given it.
    Values,
    /// The computer lacks something the configuration needs (a program, a header file, a driver), or the
    /// environment that was installed failed the test the document has for it.
    Computer,
}

/// A failure, with the sentence that says what to do about it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
    /// What it lacks that another way can be proposed for.
    pub lack: Option<Lack>,
}

impl Failure {
    pub fn new(kind: FailureKind, message: impl Into<String>) -> Failure {
        Failure {
            kind,
            message: message.into(),
            lack: None,
        }
    }

    /// The failure, with what it lacks that another way can be proposed for.
    pub fn with_lack(mut self, lack: Lack) -> Failure {
        self.lack = Some(lack);
        self
    }
}

/// The installers are the window's too, and what they say about naming a Python is the window's: a box in its
/// Settings. A person at a command line is told the flag.
pub fn in_the_words_of_the_command_line(message: &str) -> String {
    message.replace(
        "choose the Python you have in the Python box of Settings",
        "name the Python you have with --python PATH",
    )
}

/// How a call to `up` ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// Only the plan was asked for, and it was made; nothing was changed.
    Planned,
    /// Everything that was needed is installed and fetched; nothing was started.
    Prepared,
    /// It ran, and was stopped when it was asked to.
    Stopped,
    /// It was asked to stop before it ran.
    Interrupted,
    Failed(Failure),
}

impl Outcome {
    /// The code a program exits with: 0 for what went as asked, 130 for an interruption (as a shell
    /// reports a Ctrl-C), and for a failure one that says which kind (2 is a mistake in what was said, 3 and
    /// 4 are the review's and a crash's, 5 is something the configuration needs and nobody has given, 6 is
    /// something the computer lacks).
    pub fn exit_code(&self) -> i32 {
        match self {
            Outcome::Planned | Outcome::Prepared | Outcome::Stopped => 0,
            Outcome::Interrupted => 130,
            Outcome::Failed(failure) => match failure.kind {
                FailureKind::Unreadable
                | FailureKind::Needs
                | FailureKind::Fetch
                | FailureKind::Start => 1,
                FailureKind::Mistake => 2,
                FailureKind::Stopped | FailureKind::Asks => 3,
                FailureKind::Crashed => 4,
                FailureKind::Values => 5,
                FailureKind::Computer => 6,
            },
        }
    }

    /// The word the end is reported with.
    pub fn word(&self) -> &'static str {
        match self {
            Outcome::Planned => "planned",
            Outcome::Prepared => "prepared",
            Outcome::Stopped => "stopped",
            Outcome::Interrupted => "interrupted",
            Outcome::Failed(_) => "failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O73: what a program exits with says which kind of failure it was, and what went as asked is 0.
    #[test]
    fn p3_o73_the_code_a_program_exits_with_says_which_kind_it_was() {
        let failed = |kind| Outcome::Failed(Failure::new(kind, "x")).exit_code();
        assert_eq!(
            [Outcome::Planned, Outcome::Prepared, Outcome::Stopped].map(|o| o.exit_code()),
            [0, 0, 0]
        );
        assert_eq!(Outcome::Interrupted.exit_code(), 130);
        assert_eq!(failed(FailureKind::Unreadable), 1);
        assert_eq!(failed(FailureKind::Needs), 1);
        assert_eq!(failed(FailureKind::Fetch), 1);
        assert_eq!(failed(FailureKind::Start), 1);
        assert_eq!(failed(FailureKind::Mistake), 2);
        assert_eq!(failed(FailureKind::Stopped), 3);
        assert_eq!(failed(FailureKind::Asks), 3);
        assert_eq!(failed(FailureKind::Crashed), 4);
        assert_eq!(failed(FailureKind::Values), 5);
        assert_eq!(failed(FailureKind::Computer), 6);
        assert_eq!(Outcome::Planned.word(), "planned");
    }
}
