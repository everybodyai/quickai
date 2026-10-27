//! Conversations kept on this computer (WP4.28), when the person asks for it: one JSON file each under
//! `<data>/chats/`, written whole (a temporary file, then a rename) after every reply. Nothing is kept unless "Keep
//! chats" is on, nothing leaves the computer, and a chat can be exported as Markdown to be read anywhere.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One message of a kept conversation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTurn {
    pub from_person: bool,
    pub text: String,
}

/// A kept conversation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedChat {
    pub id: String,
    pub title: String,
    /// The model it was with, and the engine and computer that ran it, as they were named.
    pub model: String,
    #[serde(default)]
    pub engine: String,
    #[serde(default)]
    pub runner: Option<String>,
    #[serde(default)]
    pub instructions: String,
    /// Seconds since 1970 when it was begun, and when it last changed.
    pub created: u64,
    pub updated: u64,
    pub turns: Vec<SavedTurn>,
}

/// What the list of kept chats shows of one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSummary {
    pub id: String,
    pub title: String,
    pub updated: u64,
}

/// Where the kept chats are.
pub fn folder(data_dir: &Path) -> PathBuf {
    data_dir.join("chats")
}

/// Seconds since 1970, now.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A new id: the time it was begun, and a count that makes it one of its own in that second.
pub fn new_id(data_dir: &Path) -> String {
    let base = format!("chat-{}", now());
    let dir = folder(data_dir);
    let mut id = base.clone();
    let mut n = 2;
    while dir.join(format!("{id}.json")).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// What a conversation is called: its first message, cut at a word near 48 characters.
pub fn title_of(turns: &[SavedTurn]) -> String {
    let first = turns
        .iter()
        .find(|t| t.from_person)
        .map(|t| t.text.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    if first.is_empty() {
        return "New chat".to_string();
    }
    if first.chars().count() <= 48 {
        return first;
    }
    let cut: String = first.chars().take(48).collect();
    let cut = match cut.rfind(' ') {
        Some(at) if at > 20 => cut[..at].to_string(),
        _ => cut,
    };
    format!("{}…", cut.trim_end())
}

/// An id is a file name of ours, and nothing else (no folder, nothing that climbs out).
fn path_of(data_dir: &Path, id: &str) -> Option<PathBuf> {
    let plain = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain.then(|| folder(data_dir).join(format!("{id}.json")))
}

/// Writes `chat`, whole or not at all.
pub fn save(data_dir: &Path, chat: &SavedChat) -> std::io::Result<()> {
    let path = path_of(data_dir, &chat.id).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "not an id of a chat")
    })?;
    std::fs::create_dir_all(folder(data_dir))?;
    let text = serde_json::to_string_pretty(chat).map_err(std::io::Error::other)?;
    let temporary = path.with_extension("json.part");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, &path)
}

/// The kept chat `id`, when it is there and reads as one.
pub fn load(data_dir: &Path, id: &str) -> Option<SavedChat> {
    let text = std::fs::read_to_string(path_of(data_dir, id)?).ok()?;
    serde_json::from_str(&text).ok()
}

/// The kept chats, the one changed last first. A file that does not read as a chat is left out.
pub fn list(data_dir: &Path) -> Vec<ChatSummary> {
    let mut all: Vec<ChatSummary> = std::fs::read_dir(folder(data_dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".json")?;
            let chat = load(data_dir, id)?;
            Some(ChatSummary {
                id: chat.id,
                title: chat.title,
                updated: chat.updated,
            })
        })
        .collect();
    all.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
    all
}

/// Deletes the kept chat `id`.
pub fn delete(data_dir: &Path, id: &str) -> std::io::Result<()> {
    match path_of(data_dir, id) {
        Some(path) => match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
        None => Ok(()),
    }
}

/// The conversation as Markdown: its title, what ran it, the instructions, and each message under who said it.
pub fn markdown(chat: &SavedChat) -> String {
    let mut out = format!("# {}\n\n", chat.title);
    let mut ran = format!("*{}", chat.model);
    if !chat.engine.is_empty() {
        ran.push_str(&format!(", {}", chat.engine));
    }
    if let Some(runner) = &chat.runner {
        ran.push_str(&format!(", on {runner}"));
    }
    ran.push_str("*\n\n");
    out.push_str(&ran);
    if !chat.instructions.trim().is_empty() {
        out.push_str(&format!(
            "> **Instructions:** {}\n\n",
            chat.instructions.trim()
        ));
    }
    for turn in &chat.turns {
        let who = if turn.from_person { "You" } else { "Model" };
        out.push_str(&format!("**{who}:**\n\n{}\n\n", turn.text.trim()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::scratch;

    fn turn(from_person: bool, text: &str) -> SavedTurn {
        SavedTurn {
            from_person,
            text: text.into(),
        }
    }

    fn chat(id: &str, updated: u64, first: &str) -> SavedChat {
        let turns = vec![turn(true, first), turn(false, "Sure.")];
        SavedChat {
            id: id.into(),
            title: title_of(&turns),
            model: "qwen3:4b".into(),
            engine: "Ollama".into(),
            runner: None,
            instructions: String::new(),
            created: updated,
            updated,
            turns,
        }
    }

    #[test]
    fn chats_are_kept_whole_listed_newest_first_and_deleted() {
        let dir = scratch("chats");
        save(&dir, &chat("chat-1", 100, "Write a haiku")).unwrap();
        save(&dir, &chat("chat-2", 300, "Explain Rust lifetimes")).unwrap();
        save(&dir, &chat("chat-3", 200, "Plan a trip")).unwrap();
        std::fs::write(folder(&dir).join("garbage.json"), "not json").unwrap();
        let ids: Vec<String> = list(&dir).into_iter().map(|c| c.id).collect();
        assert_eq!(
            ids,
            ["chat-2", "chat-3", "chat-1"],
            "newest first, and what is not a chat left out"
        );
        assert_eq!(load(&dir, "chat-2").unwrap().turns.len(), 2);
        assert!(
            !folder(&dir).join("chat-2.json.part").exists(),
            "written whole"
        );
        delete(&dir, "chat-2").unwrap();
        delete(&dir, "chat-2").unwrap();
        assert_eq!(list(&dir).len(), 2);
        // an id is a name of ours and never a way out of the folder
        assert!(load(&dir, "../settings").is_none());
        assert!(save(&dir, &chat("../x", 1, "x")).is_err());
        assert!(new_id(&dir).starts_with("chat-"));
    }

    #[test]
    fn a_chat_is_called_by_its_first_message_and_exported_as_markdown() {
        assert_eq!(title_of(&[]), "New chat");
        assert_eq!(title_of(&[turn(true, "  Hello   there  ")]), "Hello there");
        let long = "Write a long story about a dragon who learns to code in Rust and then builds a compiler";
        let title = title_of(&[turn(true, long)]);
        assert!(
            title.ends_with('…') && title.chars().count() <= 49,
            "{title}"
        );
        let mut c = chat("chat-9", 1, "Hi");
        c.runner = Some("Studio PC".into());
        c.instructions = "Answer briefly.".into();
        let md = markdown(&c);
        assert!(md.starts_with("# Hi\n"));
        assert!(md.contains("*qwen3:4b, Ollama, on Studio PC*"));
        assert!(md.contains("> **Instructions:** Answer briefly."));
        assert!(md.contains("**You:**\n\nHi") && md.contains("**Model:**\n\nSure."));
    }
}
