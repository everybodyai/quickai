//! Problem reports (C202): when something breaks, QuickAI shows a report for the person to read, copy and post where
//! they choose (QuickAI's site, `quickai.everybodyai.dev/report`, or a GitHub issue). QuickAI sends nothing by itself.
//!
//! A report is plain text that names no one: what happened, the build and the kind of computer in ranges, and what
//! QuickAI was doing, with any free text cleaned first (`clean`). It has no id, no time and nothing that tells one
//! install from another. Its first lines are always in the same shape, so that the site can sort what is posted:
//!
//! ```text
//! QuickAI problem report
//! What happened: QuickAI crashed
//! QuickAI: 0.2.6, release build f35224e, x86_64-unknown-linux-gnu, installed as AppImage
//! System: Linux 24.04, x86_64, 32 GB of memory
//! Graphics: NVIDIA, Intel
//! ```
//!
//! Pure, as the rest of this crate: the runtime finds out the build and the computer, and writes a crash's report to
//! disk; the window shows the report.

pub mod clean;
pub mod measure;

/// The first line of every report: how the site knows one.
pub const FIRST_LINE: &str = "QuickAI problem report";

/// The most lines of an engine's log a report keeps (its last ones).
pub const LOG_LINES: usize = 30;

/// The longest line of an engine's log a report keeps; a longer one is cut.
pub const LOG_LINE_LENGTH: usize = 300;

/// What broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// QuickAI stopped because of an error (a panic); shown at the next start.
    Crash,
    /// An engine (llama.cpp, Ollama, ComfyUI) exited by itself while it started or served.
    EngineStopped,
    /// The window did not answer for five seconds or more.
    WindowStalled,
    /// The person opened Help → Report a problem.
    FromHelp,
}

impl Kind {
    /// What the report says happened, and the title of its window.
    pub fn headline(self) -> &'static str {
        match self {
            Kind::Crash => "QuickAI crashed",
            Kind::EngineStopped => "An engine stopped by itself",
            Kind::WindowStalled => "The window stopped answering",
            Kind::FromHelp => "Reported from Help",
        }
    }
}

/// The build and the computer, as a report says them: nothing more particular than a version, a system and memory in
/// steps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Environment {
    /// QuickAI's version (`0.2.6`).
    pub version: String,
    /// EverybodyAI released this build (`QUICKAI_CHANNEL=official` when it was built), or it was built from source.
    pub released: bool,
    /// The commit it was built from, when the build says (`f35224e`).
    pub commit: Option<String>,
    /// What it was built for (`x86_64-unknown-linux-gnu`), when the build says.
    pub target: Option<String>,
    /// How it was installed, in words (`AppImage`, `Microsoft Store`, `tar.gz`).
    pub package: String,
    /// The system (`Linux`, `Windows`, `macOS`) and its version (`24.04`), and the processor's architecture.
    pub os: String,
    pub os_version: String,
    pub arch: String,
    /// The memory in steps: 8, 16, 32, 64 or 128 (and more) GB (`measure::memory_bucket_gb`).
    pub memory_gb: u32,
    /// The makers of the graphics adapters (`NVIDIA`, `AMD`, `Intel`, `Apple`), never the card.
    pub graphics: Vec<String>,
}

impl Environment {
    /// The lines of the head of a report after "What happened".
    fn lines(&self) -> Vec<String> {
        let mut build = vec![
            self.version.clone(),
            if self.released {
                "release build".to_string()
            } else {
                "built from source".to_string()
            },
        ];
        if let Some(commit) = &self.commit {
            build[1] = format!("{} {commit}", build[1]);
        }
        if let Some(target) = &self.target {
            build.push(target.clone());
        }
        build.push(format!("installed as {}", self.package));
        let memory = if self.memory_gb >= 128 {
            "128 GB or more of memory".to_string()
        } else {
            format!("{} GB of memory", self.memory_gb)
        };
        let system = if self.os_version.is_empty() || self.os_version == "unknown" {
            self.os.clone()
        } else {
            format!("{} {}", self.os, self.os_version)
        };
        let graphics = if self.graphics.is_empty() {
            "none found".to_string()
        } else {
            self.graphics.join(", ")
        };
        vec![
            format!("QuickAI: {}", build.join(", ")),
            format!("System: {system}, {}, {memory}", self.arch),
            format!("Graphics: {graphics}"),
        ]
    }
}

/// A report: what happened, the build and the computer, and what is known of the moment, each already cleaned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub kind: Kind,
    pub environment: Environment,
    /// `Name: value` lines, in order (`Message`, `Where`, `Engine`, `Exit`...).
    pub details: Vec<(String, String)>,
    /// The function names that led to a crash, outermost last.
    pub backtrace: Vec<String>,
    /// The last lines of an engine's log, cleaned.
    pub log: Vec<String>,
}

impl Report {
    pub fn new(kind: Kind, environment: Environment) -> Self {
        Self {
            kind,
            environment,
            details: Vec::new(),
            backtrace: Vec::new(),
            log: Vec::new(),
        }
    }

    /// A `Name: value` line; an empty value is left out.
    pub fn detail(mut self, name: &str, value: impl Into<String>) -> Self {
        let value = value.into();
        let value = value.trim();
        if !value.is_empty() {
            self.details.push((name.to_string(), one_line(value)));
        }
        self
    }

    /// The report as the person reads it, copies it and posts it.
    pub fn text(&self) -> String {
        let mut out = vec![
            FIRST_LINE.to_string(),
            format!("What happened: {}", self.kind.headline()),
        ];
        out.extend(self.environment.lines());
        if !self.details.is_empty() {
            out.push(String::new());
            out.extend(
                self.details
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}")),
            );
        }
        if !self.backtrace.is_empty() {
            out.push(String::new());
            out.push("Backtrace:".to_string());
            out.extend(self.backtrace.iter().map(|f| format!("  {f}")));
        }
        if !self.log.is_empty() {
            out.push(String::new());
            out.push(format!(
                "The engine's log, its last {} lines:",
                self.log.len()
            ));
            out.extend(self.log.iter().map(|l| format!("  {l}")));
        }
        out.join("\n") + "\n"
    }
}

/// The last `LOG_LINES` lines of a log, each cleaned and cut to `LOG_LINE_LENGTH` characters, blank ones left out.
pub fn log_tail(lines: &[String], cleaner: &clean::Cleaner) -> Vec<String> {
    let kept: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    let from = kept.len().saturating_sub(LOG_LINES);
    kept[from..]
        .iter()
        .map(|l| {
            let line = cleaner.clean(l.trim_end());
            if line.chars().count() > LOG_LINE_LENGTH {
                line.chars().take(LOG_LINE_LENGTH).collect::<String>() + "…"
            } else {
                line
            }
        })
        .collect()
}

/// `text` on one line: a value never breaks the shape of the report.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment() -> Environment {
        Environment {
            version: "0.2.6".into(),
            released: true,
            commit: Some("f35224e".into()),
            target: Some("x86_64-unknown-linux-gnu".into()),
            package: "AppImage".into(),
            os: "Linux".into(),
            os_version: "24.04".into(),
            arch: "x86_64".into(),
            memory_gb: 32,
            graphics: vec!["NVIDIA".into(), "Intel".into()],
        }
    }

    #[test]
    fn a_report_starts_with_the_lines_the_site_reads() {
        let text = Report::new(Kind::Crash, environment())
            .detail("Message", "index out of bounds")
            .detail("Where", "crates/quickai/src/app/mod.rs:12:5")
            .text();
        assert_eq!(
            text,
            "QuickAI problem report\n\
             What happened: QuickAI crashed\n\
             QuickAI: 0.2.6, release build f35224e, x86_64-unknown-linux-gnu, installed as AppImage\n\
             System: Linux 24.04, x86_64, 32 GB of memory\n\
             Graphics: NVIDIA, Intel\n\
             \n\
             Message: index out of bounds\n\
             Where: crates/quickai/src/app/mod.rs:12:5\n"
        );
    }

    #[test]
    fn a_build_from_source_with_nothing_known_says_so_plainly() {
        let environment = Environment {
            version: "0.2.6".into(),
            package: "tar.gz".into(),
            os: "Windows".into(),
            os_version: "unknown".into(),
            arch: "x86_64".into(),
            memory_gb: 128,
            ..Default::default()
        };
        let text = Report::new(Kind::FromHelp, environment).text();
        assert!(
            text.contains("What happened: Reported from Help\n"),
            "{text}"
        );
        assert!(
            text.contains("QuickAI: 0.2.6, built from source, installed as tar.gz\n"),
            "{text}"
        );
        assert!(
            text.contains("System: Windows, x86_64, 128 GB or more of memory\n"),
            "{text}"
        );
        assert!(text.contains("Graphics: none found\n"), "{text}");
    }

    #[test]
    fn a_value_stays_on_its_line_and_an_empty_one_is_left_out() {
        let report = Report::new(Kind::EngineStopped, environment())
            .detail("Message", "first line\nWhat happened: forged")
            .detail("Model", "  ");
        assert_eq!(
            report.details,
            vec![(
                "Message".to_string(),
                "first line What happened: forged".to_string()
            )]
        );
        assert_eq!(report.text().matches("\nWhat happened:").count(), 1);
    }

    #[test]
    fn the_log_keeps_its_last_lines_cleaned_and_cut() {
        let cleaner = clean::Cleaner::new(Some("/home/grace"), Some("grace"), None, &[]);
        let mut lines: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
        lines.push(String::new());
        lines.push("loading /home/grace/.quickai/models/secret.gguf".into());
        lines.push("x".repeat(400));
        let tail = log_tail(&lines, &cleaner);
        assert_eq!(tail.len(), LOG_LINES);
        assert_eq!(tail[0], "line 12");
        assert_eq!(
            tail[LOG_LINES - 2],
            "loading ~/.quickai/models/<model file>"
        );
        assert_eq!(tail[LOG_LINES - 1].chars().count(), LOG_LINE_LENGTH + 1);
        let report = Report {
            log: tail,
            ..Report::new(Kind::EngineStopped, environment())
        };
        assert!(report
            .text()
            .contains("\nThe engine's log, its last 30 lines:\n  line 12\n"));
    }
}
