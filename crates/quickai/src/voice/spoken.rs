//! What a model writes, made into what should be heard, and cut into sentences: the marks of Markdown are left out (a
//! voice would read "asterisk"), a block of code is said to be there rather than read, a link is its words and an
//! address its site, and a heading or an item of a list ends with a pause. `SentenceFeed` does it as an answer streams
//! in, so that its first sentence can be read while the rest is written.
//!
//! The twin of `the read-aloud MCP server's` (the MCP server that reads aloud): a change to one belongs in the other.

/// The most that is read of one answer, in characters.
pub const MAX_CHARS: usize = 20_000;
/// A sentence longer than this is cut at a comma or a space, so that the voice starts before the end of a long one.
const LONGEST: usize = 280;

/// `raw` as it should be heard, in one line.
pub fn for_speech(raw: &str) -> String {
    let mut state = LineState::default();
    let lines: Vec<String> = raw
        .lines()
        .filter_map(|line| clean_line(line, &mut state))
        .collect();
    cut(&collapse(&lines.join(" ")))
}

/// `raw` as it should be heard, in sentences.
pub fn sentences(raw: &str) -> Vec<String> {
    let cleaned = for_speech(raw);
    let (mut done, rest) = split_sentences(&cleaned);
    // the last one has no space after its mark: it is a sentence all the same, cut when it is long
    done.extend(cut_long(rest.trim()));
    done
}

/// What is carried from one line to the next: whether it is inside a block of code.
#[derive(Clone, Debug, Default)]
pub struct LineState {
    in_code: bool,
}

/// One whole line of Markdown as it should be heard, or nothing (a rule, a fence's inside, an empty line).
fn clean_line(line: &str, state: &mut LineState) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        state.in_code = !state.in_code;
        return state
            .in_code
            .then(|| "(There is some code here.)".to_string());
    }
    if state.in_code || trimmed.is_empty() || is_table_rule(trimmed) || is_rule(trimmed) {
        return None;
    }
    let (body, pause) = strip_block_marks(trimmed);
    let mut spoken = inline(body);
    let table = trimmed.starts_with('|');
    if table {
        spoken = spoken
            .split('|')
            .map(str::trim)
            .filter(|cell| !cell.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
    }
    let spoken = collapse(&spoken);
    if spoken.is_empty() {
        return None;
    }
    // a heading, an item of a list or a row of a table ends with a pause when it has no stop of its own
    Some(
        if (pause || table) && !spoken.ends_with(['.', '!', '?', ':', ';']) {
            format!("{spoken}.")
        } else {
            spoken
        },
    )
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `MAX_CHARS` characters, cut at a space, with words that say the rest was left out.
fn cut(text: &str) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    let end = text
        .char_indices()
        .nth(MAX_CHARS)
        .map_or(text.len(), |(i, _)| i);
    let head = &text[..end];
    let head = head.rfind(' ').map_or(head, |space| &head[..space]);
    format!("{head} … The rest is left out.")
}

/// The sentences that end in `text`, and what is left after the last of them. A sentence ends at `.`, `!`, `?` or `…`
/// (and their marks of other scripts) before a space, unless the word is a short form (`e.g.`, `Dr.`); one longer than
/// `LONGEST` is cut at its last comma or space.
pub fn split_sentences(text: &str) -> (Vec<String>, &str) {
    let mut done = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(at, c)) in chars.iter().enumerate() {
        let next = chars.get(k + 1).map(|&(_, n)| n);
        let ends = match c {
            '.' | '!' | '?' | '…' => next.is_some_and(char::is_whitespace),
            // a full stop of Chinese and Japanese, of Hindi and of Arabic needs no space after it
            '。' | '！' | '？' | '।' | '؟' => true,
            _ => false,
        };
        if !ends {
            continue;
        }
        let end = at + c.len_utf8();
        let sentence = text[start..end].trim();
        if c == '.' && is_short_form(sentence) {
            continue;
        }
        if !sentence.is_empty() {
            done.extend(cut_long(sentence));
        }
        start = end;
    }
    let mut rest = &text[start..];
    // what is left has no end yet: a long one is read in pieces all the same
    while rest.chars().count() > LONGEST * 2 {
        let piece = longest_piece(rest);
        done.push(rest[..piece].trim().to_string());
        rest = &rest[piece..];
    }
    (done, rest)
}

/// The last word of `sentence` (with its dot) is a short form, not the end of the sentence.
fn is_short_form(sentence: &str) -> bool {
    let word = sentence.rsplit(char::is_whitespace).next().unwrap_or("");
    let word = word.trim_start_matches(['(', '"', '\'']).to_lowercase();
    const SHORT: [&str; 14] = [
        "e.g.", "i.e.", "etc.", "vs.", "mr.", "mrs.", "ms.", "dr.", "st.", "no.", "approx.",
        "fig.", "cf.", "a.m.",
    ];
    SHORT.contains(&word.as_str())
        // a single letter with a dot: an initial (J. R. R. Tolkien)
        || (word.chars().count() == 2 && word.starts_with(char::is_alphabetic))
}

fn cut_long(sentence: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = sentence;
    while rest.chars().count() > LONGEST {
        let piece = longest_piece(rest);
        out.push(rest[..piece].trim().to_string());
        rest = rest[piece..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// Where to cut a long `text`: after its last comma (or else space) within `LONGEST` characters.
fn longest_piece(text: &str) -> usize {
    let limit = text
        .char_indices()
        .nth(LONGEST)
        .map_or(text.len(), |(i, _)| i);
    let head = &text[..limit];
    head.rfind(", ")
        .map(|i| i + 1)
        .or_else(|| head.rfind(' '))
        .filter(|&i| i > 0)
        .unwrap_or(limit)
}

/// An answer as it streams in, made into sentences as soon as each is whole: pieces go in with `push`, and `finish`
/// gives what is left at its end. A line is cleaned once it is whole; a sentence that ends within the line still
/// being written is given at once, so the first one is read while the model writes the rest.
#[derive(Debug, Default)]
pub struct SentenceFeed {
    /// What came that is not a whole line yet.
    line: String,
    /// How much of `line` was already given as sentences.
    given: usize,
    state: LineState,
    /// Cleaned words that are not a whole sentence yet.
    rest: String,
    /// Characters given so far, to keep to `MAX_CHARS`.
    total: usize,
}

impl SentenceFeed {
    pub fn push(&mut self, piece: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.line.push_str(piece);
        while let Some(newline) = self.line.find('\n') {
            let whole: String = self.line.drain(..=newline).collect();
            let unsaid = &whole[self.given.min(whole.len())..];
            let fresh = self.given == 0;
            self.given = 0;
            // the beginning of a line that was given already was cleaned with its marks; the rest is words
            let cleaned = if fresh {
                clean_line(whole.trim_end_matches('\n'), &mut self.state)
            } else {
                Some(collapse(&inline(unsaid.trim_end_matches('\n'))))
            };
            if let Some(words) = cleaned.filter(|w| !w.is_empty()) {
                self.add(&words, &mut out);
            }
            // a line ends a sentence: what is left of it is said
            let rest = std::mem::take(&mut self.rest);
            for piece in cut_long(rest.trim()) {
                self.give(piece, &mut out);
            }
        }
        // the line still being written: sentences that end in it, unless it is code, a table or a fence
        let line = self.line.trim_start();
        let plain = !self.state.in_code
            && !line.starts_with("```")
            && !line.starts_with("~~~")
            && !line.starts_with('|');
        if plain && self.line.len() > self.given {
            // from where its words begin: past the marks of the line (`1. `, `- `, `# `), which are no sentence
            let from = if self.given == 0 {
                let lead = &self.line[self.line.len() - self.line.trim_start().len()..];
                let (body, _) = strip_block_marks(lead);
                body.as_ptr() as usize - self.line.as_ptr() as usize
            } else {
                self.given
            };
            let unsaid = &self.line[from..];
            // up to the last end of a sentence in it, before a space
            if let Some(end) = last_sentence_end(unsaid) {
                let words = collapse(&inline(&unsaid[..end]));
                self.given = from + end;
                if !words.is_empty() {
                    self.add(&words, &mut out);
                }
            }
        }
        out
    }

    /// What is left when the answer has ended.
    pub fn finish(&mut self) -> Vec<String> {
        let mut out = self.push("\n");
        let rest = std::mem::take(&mut self.rest);
        for piece in cut_long(rest.trim()) {
            self.give(piece, &mut out);
        }
        out
    }

    fn add(&mut self, words: &str, out: &mut Vec<String>) {
        if !self.rest.is_empty() {
            self.rest.push(' ');
        }
        self.rest.push_str(words);
        let joined = std::mem::take(&mut self.rest);
        let (done, rest) = split_sentences(&joined);
        self.rest = rest.trim_start().to_string();
        for sentence in done {
            self.give(sentence, out);
        }
    }

    fn give(&mut self, sentence: String, out: &mut Vec<String>) {
        if self.total >= MAX_CHARS {
            return;
        }
        self.total += sentence.chars().count();
        out.push(sentence);
    }
}

/// Where the last sentence of `text` ends (just after its mark), when one does before a space.
fn last_sentence_end(text: &str) -> Option<usize> {
    let (done, rest) = split_sentences(text);
    if done.is_empty() {
        return None;
    }
    Some(text.len() - rest.len())
}

/// `---`, `***` or `___` alone on a line.
fn is_rule(line: &str) -> bool {
    let line: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    line.len() >= 3
        && (line.chars().all(|c| c == '-')
            || line.chars().all(|c| c == '*')
            || line.chars().all(|c| c == '_'))
}

/// `|---|:--:|` under the head of a table.
fn is_table_rule(line: &str) -> bool {
    line.starts_with('|') && line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// The line without the marks that begin it (`#`, `>`, `-`, `*`, `+`, `1.`), and whether it should end with a pause.
fn strip_block_marks(line: &str) -> (&str, bool) {
    let mut rest = line;
    while let Some(after) = rest.strip_prefix('>') {
        rest = after.trim_start();
    }
    if rest.starts_with('#') {
        let after = rest.trim_start_matches('#');
        if after.starts_with(' ') || after.is_empty() {
            return (after.trim(), true);
        }
    }
    for bullet in ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "+ "] {
        if let Some(after) = rest.strip_prefix(bullet) {
            return (after, true);
        }
    }
    // 1. or 1)
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 3 {
        let after = &rest[digits..];
        if let Some(item) = after
            .strip_prefix(". ")
            .or_else(|| after.strip_prefix(") "))
        {
            return (item, true);
        }
    }
    (rest, false)
}

/// The marks inside a line: emphasis, code, links, pictures, addresses and tags.
pub fn inline(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // ![words](address) and [words](address): the words
        if c == '[' || (c == '!' && chars.get(i + 1) == Some(&'[')) {
            let start = if c == '!' { i + 2 } else { i + 1 };
            if let Some((words, next)) = link_at(&chars, start) {
                out.push_str(&inline(&words));
                i = next;
                continue;
            }
        }
        // an address: its site
        if c == 'h' && (starts_with(&chars, i, "https://") || starts_with(&chars, i, "http://")) {
            let end = chars[i..]
                .iter()
                .position(|c| c.is_whitespace() || matches!(c, ')' | '>' | '"'))
                .map_or(chars.len(), |n| i + n);
            let address: String = chars[i..end].iter().collect();
            out.push_str(&site_of(&address));
            i = end;
            continue;
        }
        // <tags> and <addresses>
        if c == '<' {
            if let Some(n) = chars[i..].iter().position(|c| *c == '>') {
                let inner: String = chars[i + 1..i + n].iter().collect();
                if inner.starts_with("http") {
                    out.push_str(&site_of(&inner));
                } else if inner.eq_ignore_ascii_case("br") || inner.eq_ignore_ascii_case("br/") {
                    out.push(' ');
                }
                i += n + 1;
                continue;
            }
        }
        match c {
            '*' | '`' => {}
            '~' if chars.get(i + 1) == Some(&'~') => i += 1,
            '_' if chars.get(i + 1) == Some(&'_') => i += 1,
            // `_emphasis_` is left out, a `_` inside a name (snake_case) is kept
            '_' => {
                let inside = out.chars().last().is_some_and(char::is_alphanumeric)
                    && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
                if inside {
                    out.push('_');
                }
            }
            '\\' if chars.get(i + 1).is_some_and(|n| n.is_ascii_punctuation()) => {
                out.push(chars[i + 1]);
                i += 1;
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// At `start` (just after `[`): the words up to `]` and, when `(address)` follows, where the link ends.
fn link_at(chars: &[char], start: usize) -> Option<(String, usize)> {
    let close = chars[start..].iter().position(|c| *c == ']')? + start;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = chars[close + 2..].iter().position(|c| *c == ')')? + close + 2;
    Some((chars[start..close].iter().collect(), end + 1))
}

fn starts_with(chars: &[char], at: usize, prefix: &str) -> bool {
    prefix
        .chars()
        .enumerate()
        .all(|(k, p)| chars.get(at + k) == Some(&p))
}

/// `https://www.example.com/a/b` → `a link to example.com`.
fn site_of(address: &str) -> String {
    let host = address
        .split("://")
        .nth(1)
        .unwrap_or(address)
        .split(['/', '?', '#', ':'])
        .next()
        .unwrap_or("");
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() {
        "a link".to_string()
    } else {
        format!("a link to {host}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_not_read() {
        assert_eq!(
            for_speech("This is **very** _important_ and `cargo run`."),
            "This is very important and cargo run."
        );
        assert_eq!(for_speech("keep snake_case names"), "keep snake_case names");
        assert_eq!(
            for_speech("# Fried rice\n\nYou need:\n- rice\n- two eggs\n1. Fry the eggs"),
            "Fried rice. You need: rice. two eggs. Fry the eggs."
        );
        assert_eq!(
            for_speech("Run:\n```sh\nmake\n```\nThen wait."),
            "Run: (There is some code here.) Then wait."
        );
        assert_eq!(
            for_speech("See [the docs](https://x.org/d) or https://www.example.org/x"),
            "See the docs or a link to example.org"
        );
        assert_eq!(
            for_speech("| Name | Size |\n|---|---:|\n| Bonsai | 1.1 GB |"),
            "Name, Size. Bonsai, 1.1 GB."
        );
    }

    #[test]
    fn sentences_end_at_their_marks_and_not_at_short_forms() {
        assert_eq!(
            sentences("It is 3.5 GB. Dr. Smith said so, e.g. here! Is it? Yes"),
            [
                "It is 3.5 GB.",
                "Dr. Smith said so, e.g. here!",
                "Is it?",
                "Yes"
            ]
        );
        assert_eq!(sentences("नमस्ते। आप कैसे हैं?"), ["नमस्ते।", "आप कैसे हैं?"]);
        let long = format!("{}, and then the end.", "word ".repeat(80).trim());
        let parts = sentences(&long);
        assert!(parts.len() >= 2, "{parts:?}");
        assert!(
            parts.iter().all(|p| p.chars().count() <= LONGEST + 1),
            "{parts:?}"
        );
    }

    #[test]
    fn a_streamed_answer_gives_each_sentence_once_as_soon_as_it_is_whole() {
        let mut feed = SentenceFeed::default();
        let mut heard = Vec::new();
        for piece in [
            "A quick **spinach",
            "** fried rice. Scramble",
            " two eggs",
            ".\n\n- Fry",
            " the rice\n- add",
            " spinach",
            "\n```\ncode()\n```\nDone",
        ] {
            heard.extend(feed.push(piece));
            if heard.len() == 1 {
                assert_eq!(
                    heard[0], "A quick spinach fried rice.",
                    "the first is given before the line ends"
                );
            }
        }
        heard.extend(feed.finish());
        assert_eq!(
            heard,
            [
                "A quick spinach fried rice.",
                "Scramble two eggs.",
                "Fry the rice.",
                "add spinach.",
                "(There is some code here.)",
                "Done",
            ]
        );
    }

    #[test]
    fn what_the_feed_gives_is_what_reading_it_whole_gives() {
        let text =
            "# Title\nOne. Two? Three!\n\n1. First item\n2. Second item\n> a quote. With two.\nEnd";
        let mut feed = SentenceFeed::default();
        let mut heard = Vec::new();
        for c in text.chars() {
            heard.extend(feed.push(&c.to_string()));
        }
        heard.extend(feed.finish());
        assert_eq!(heard, sentences(text));
    }
}
