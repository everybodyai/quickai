//! Problems found while loading or checking configuration (docs/architecture.md, section 5.3).
//!
//! A broken document never stops the application: it becomes a `Diagnostic` that says which file
//! and line it is in, which value, what is wrong and, when there is one, what to do about it. The
//! settings window, the log and the headless tools all show the same list.

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    /// The document, as its layer names it (`embedded:engines/llamacpp.toml`, or a path).
    pub file: Option<String>,
    /// 1-based line of the offending value in `file`, when it could be found.
    pub line: Option<u32>,
    /// Where in the document: `field[ctx_size].bind.arg`. Empty when the whole document is meant.
    pub path: String,
    pub message: String,
    pub hint: Option<String>,
}

impl Diagnostic {
    fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            file: None,
            line: None,
            path: String::new(),
            message: message.into(),
            hint: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::new(Severity::Error, message)
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, message)
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self::new(Severity::Info, message)
    }

    pub fn in_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn at_line(mut self, line: u32) -> Self {
        self.line = Some(line);
        self
    }

    pub fn at_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// `error: embedded:engines/llamacpp.toml:42: field[ctx_size].default: unknown name `runtim`` and,
/// on a second line, the hint. Parts that are not known are left out.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.severity.label())?;
        match (&self.file, self.line) {
            (Some(file), Some(line)) => write!(f, "{file}:{line}: ")?,
            (Some(file), None) => write!(f, "{file}: ")?,
            (None, _) => {}
        }
        if !self.path.is_empty() {
            write!(f, "{}: ", self.path)?;
        }
        write!(f, "{}", self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {hint}")?;
        }
        Ok(())
    }
}

/// Whether any diagnostic in `list` is an error.
pub fn has_errors(list: &[Diagnostic]) -> bool {
    list.iter().any(Diagnostic::is_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_shows_every_known_part() {
        let d = Diagnostic::error("unknown name `runtim`")
            .in_file("embedded:engines/llamacpp.toml")
            .at_line(42)
            .at_path("field[ctx_size].default")
            .with_hint("did you mean `runtime`?");
        assert_eq!(
            d.to_string(),
            "error: embedded:engines/llamacpp.toml:42: field[ctx_size].default: unknown name `runtim`\n  hint: did you mean `runtime`?"
        );
    }

    #[test]
    fn display_leaves_out_what_is_not_known() {
        assert_eq!(
            Diagnostic::warning("unknown key").to_string(),
            "warning: unknown key"
        );
        assert_eq!(
            Diagnostic::info("skipped").in_file("a.toml").to_string(),
            "info: a.toml: skipped"
        );
        assert_eq!(
            Diagnostic::error("bad")
                .in_file("a.toml")
                .at_line(3)
                .to_string(),
            "error: a.toml:3: bad"
        );
    }

    #[test]
    fn severities_order_and_errors_are_found() {
        assert!(Severity::Info < Severity::Warning && Severity::Warning < Severity::Error);
        assert!(!has_errors(&[
            Diagnostic::warning("w"),
            Diagnostic::info("i")
        ]));
        assert!(has_errors(&[
            Diagnostic::warning("w"),
            Diagnostic::error("e")
        ]));
    }
}
