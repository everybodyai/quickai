//! Cleaning text before it can be sent (docs/reports-and-community.md, section 7.3). The only free text telemetry
//! carries is a crash's message, its location and its thread's name; this takes out of them what could name a person
//! or a computer, or open something of theirs. It cannot be perfect, which is why logs are never sent.

use std::sync::OnceLock;

use regex::{Captures, Regex};

/// What is particular to this computer and person, to take out of text: the home folder, the user's and the
/// computer's names, and the addresses of the remote computers in the settings.
#[derive(Clone, Debug, Default)]
pub struct Cleaner {
    homes: Vec<String>,
    user: Option<String>,
    host: Option<String>,
    remotes: Vec<String>,
}

/// The longest cleaned text kept; anything longer is cut, with `…`.
pub const MAX_TEXT: usize = 2000;

impl Cleaner {
    /// A cleaner for this home folder, user name, host name and remote addresses. Names shorter than three
    /// characters are not looked for: they would match inside ordinary words.
    pub fn new(
        home: Option<&str>,
        user: Option<&str>,
        host: Option<&str>,
        remotes: &[String],
    ) -> Self {
        let mut homes = Vec::new();
        if let Some(home) = home
            .map(|h| h.trim_end_matches(['/', '\\']))
            .filter(|h| h.len() > 1)
        {
            homes.push(home.to_string());
            // the same folder as a string literal writes it, and with the other slash
            if home.contains('\\') {
                homes.push(home.replace('\\', "\\\\"));
                homes.push(home.replace('\\', "/"));
            }
        }
        // the longest first, so that an escaped spelling is replaced whole
        homes.sort_by_key(|h| std::cmp::Reverse(h.len()));
        let long = |name: Option<&str>| {
            name.map(str::trim)
                .filter(|n| n.len() >= 3)
                .map(str::to_string)
        };
        Self {
            homes,
            user: long(user),
            host: long(host),
            remotes: remotes
                .iter()
                .map(|r| r.trim().to_string())
                .filter(|r| r.len() >= 3)
                .collect(),
        }
    }

    /// `text` with what could name the person or the computer replaced, at most `MAX_TEXT` characters.
    pub fn clean(&self, text: &str) -> String {
        let mut out = text.to_string();
        // a URL keeps its host and path; its query, where tokens are put, goes
        out = re(&QUERY).replace_all(&out, "$1").into_owned();
        out = re(&TOKEN).replace_all(&out, "<secret>").into_owned();
        out = re(&ASSIGNMENT)
            .replace_all(&out, |c: &Captures| {
                if crate::plan::is_secret_name(&c[1]) {
                    format!("{}{}<secret>", &c[1], &c[2])
                } else {
                    c[0].to_string()
                }
            })
            .into_owned();
        for remote in &self.remotes {
            out = replace_word(&out, remote, "<remote>");
        }
        for home in &self.homes {
            out = out.replace(home.as_str(), "~");
        }
        out = re(&EMAIL).replace_all(&out, "<email>").into_owned();
        out = re(&IPV4)
            .replace_all(&out, |c: &Captures| keep_loopback(&c[0]))
            .into_owned();
        // a `::` between hex letters is as often a Rust path (`Self::face`) as an address: an address has a digit
        out = re(&IPV6)
            .replace_all(&out, |c: &Captures| {
                if c[0].chars().any(|ch| ch.is_ascii_digit()) {
                    keep_loopback(&c[0])
                } else {
                    c[0].to_string()
                }
            })
            .into_owned();
        if let Some(user) = &self.user {
            out = replace_word(&out, user, "<user>");
        }
        if let Some(host) = &self.host {
            out = replace_word(&out, host, "<host>");
        }
        out = re(&MODEL_FILE)
            .replace_all(&out, "<model file>")
            .into_owned();
        out = out
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect();
        if out.chars().count() > MAX_TEXT {
            out = out.chars().take(MAX_TEXT - 1).collect::<String>() + "…";
        }
        out
    }
}

/// `needle` replaced where it stands as a word of its own (not inside a longer name), case-sensitively.
fn replace_word(text: &str, needle: &str, with: &str) -> String {
    let pattern = format!(r"(^|[^\w.-]){}($|[^\w-])", regex::escape(needle));
    match Regex::new(&pattern) {
        Ok(re) => {
            // a match takes the character on each side, so a name repeated with one character between is
            // replaced in a second pass
            let once = re
                .replace_all(text, format!("${{1}}{with}${{2}}"))
                .into_owned();
            re.replace_all(&once, format!("${{1}}{with}${{2}}"))
                .into_owned()
        }
        Err(_) => text.to_string(),
    }
}

fn keep_loopback(address: &str) -> String {
    match address {
        "127.0.0.1" | "0.0.0.0" | "::1" => address.to_string(),
        _ => "<ip>".to_string(),
    }
}

fn re(cell: &'static (OnceLock<Regex>, &'static str)) -> &'static Regex {
    cell.0
        .get_or_init(|| Regex::new(cell.1).expect("the cleaner's patterns are valid"))
}

static QUERY: (OnceLock<Regex>, &str) = (OnceLock::new(), r"(https?://[^\s?#]+)\?[^\s#)]*");
static TOKEN: (OnceLock<Regex>, &str) = (
    OnceLock::new(),
    r"\b(?:hf_[A-Za-z0-9]{16,}|gh[pousr]_[A-Za-z0-9]{16,}|github_pat_[A-Za-z0-9_]{16,}|sk-[A-Za-z0-9_-]{16,})",
);
static ASSIGNMENT: (OnceLock<Regex>, &str) = (
    OnceLock::new(),
    r"\b([A-Za-z_][A-Za-z0-9_]*)(\s*[=:]\s*)[^\s,;]+",
);
static EMAIL: (OnceLock<Regex>, &str) = (
    OnceLock::new(),
    r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+",
);
static IPV4: (OnceLock<Regex>, &str) = (OnceLock::new(), r"\b(?:\d{1,3}\.){3}\d{1,3}\b");
// four groups or more, or a `::` between groups: three groups would be a time of day
static IPV6: (OnceLock<Regex>, &str) = (
    OnceLock::new(),
    r"(?i)\b(?:[0-9a-f]{1,4}:){3,7}[0-9a-f]{1,4}\b|(?:\b[0-9a-f]{1,4})?(?::[0-9a-f]{1,4})*::(?:[0-9a-f]{1,4}\b)(?::[0-9a-f]{1,4}\b)*",
);
static MODEL_FILE: (OnceLock<Regex>, &str) = (
    OnceLock::new(),
    r"[\w.+-]+\.(?:gguf|safetensors|ckpt|pth?|onnx|bin)\b",
);

#[cfg(test)]
mod tests {
    use super::*;

    fn cleaner() -> Cleaner {
        Cleaner::new(
            Some("/home/grace"),
            Some("grace"),
            Some("lab-desktop"),
            &["gpu-box.local".to_string(), "10.0.0.7".to_string()],
        )
    }

    /// Each row: what a crash message may hold, and what is sent.
    const ROWS: &[(&str, &str)] = &[
        (
            "cannot open /home/grace/.quickai/models/x",
            "cannot open ~/.quickai/models/x",
        ),
        ("no such file: /home/grace", "no such file: ~"),
        ("user grace on lab-desktop", "user <user> on <host>"),
        ("graceful shutdown", "graceful shutdown"),
        ("ssh to gpu-box.local failed", "ssh to <remote> failed"),
        ("connect 10.0.0.7:22 refused", "connect <remote>:22 refused"),
        (
            "connect 192.168.1.20:8080 refused",
            "connect <ip>:8080 refused",
        ),
        ("listening on 127.0.0.1:8080", "listening on 127.0.0.1:8080"),
        ("peer fe80::1ff:fe23:4567:890a lost", "peer <ip> lost"),
        ("at 09:12:44 it stopped", "at 09:12:44 it stopped"),
        ("mail grace.h@example.org now", "mail <email> now"),
        (
            "GET https://huggingface.co/x/y?token=abc123 failed",
            "GET https://huggingface.co/x/y failed",
        ),
        (
            "token hf_abcdefghijklmnopqrstuv is bad",
            "token <secret> is bad",
        ),
        ("HF_TOKEN=abcdef123 set", "HF_TOKEN=<secret> set"),
        ("PORT=8080 set", "PORT=8080 set"),
        ("api_key: zzz999", "api_key: <secret>"),
        (
            "loading Qwen3-8B-Q4_K_M.gguf failed",
            "loading <model file> failed",
        ),
        (
            "called `Option::unwrap()` on a `None` value",
            "called `Option::unwrap()` on a `None` value",
        ),
        ("in Self::face and Vec::add", "in Self::face and Vec::add"),
    ];

    #[test]
    fn what_could_name_a_person_or_a_computer_is_taken_out() {
        let c = cleaner();
        for (raw, sent) in ROWS {
            assert_eq!(c.clean(raw), *sent, "cleaning {raw:?}");
        }
    }

    #[test]
    fn a_windows_home_is_taken_out_in_every_spelling() {
        let c = Cleaner::new(Some(r"C:\Users\Grace"), Some("Grace"), None, &[]);
        assert_eq!(
            c.clean(r"open C:\Users\Grace\.quickai\x"),
            r"open ~\.quickai\x"
        );
        assert_eq!(
            c.clean(r#"path "C:\\Users\\Grace\\.quickai""#),
            r#"path "~\\.quickai""#
        );
        assert_eq!(c.clean("open C:/Users/Grace/x"), "open ~/x");
    }

    #[test]
    fn text_is_cut_and_control_characters_go() {
        let c = Cleaner::default();
        let long = "x".repeat(MAX_TEXT + 50);
        assert_eq!(c.clean(&long).chars().count(), MAX_TEXT);
        assert_eq!(c.clean("a\u{1b}[31mb\tc\nd"), "a[31mbc\nd");
    }

    #[test]
    fn short_names_are_not_looked_for() {
        let c = Cleaner::new(None, Some("al"), Some("pc"), &[]);
        assert_eq!(c.clean("all pc parts"), "all pc parts");
    }
}
