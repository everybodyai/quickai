//! What a model writes, made into what should be heard, and cut into sentences: the marks of Markdown are left out (a
//! voice would read "asterisk"), a block of code is said to be there rather than read, a link is its words and an
//! address its site, and a heading or an item of a list ends with a pause. `SentenceFeed` does it as an answer streams
//! in, so that its first sentence can be read while the rest is written.

/// The most that is read of one answer, in characters.
pub const MAX_CHARS: usize = 20_000;
/// A sentence longer than this is cut at a comma or a space, so that the voice starts before the end of a long one.
const LONGEST: usize = 280;

/// `raw` as it should be heard, in one line, without the symbols a voice would name.
pub fn for_speech(raw: &str) -> String {
    for_speech_with(raw, false)
}

/// `raw` as it should be heard, in one line; `symbols` keeps the symbols too (`#`, `/`, arrows, emoji…), which a voice
/// reads by their names.
pub fn for_speech_with(raw: &str, symbols: bool) -> String {
    voiced(&cleaned(raw).replace(PAUSE, ""), symbols)
}

/// A pause of a dash: the sentence is read in two pieces there (`dashes_as_pauses`), for the voice takes a breath
/// between pieces and hardly any at a comma. A mark of its own, never heard or shown.
const PAUSE: char = '\u{E000}';

/// `raw` cleaned, with the pauses of its dashes marked.
fn cleaned(raw: &str) -> String {
    let mut state = LineState::default();
    let lines: Vec<String> = raw
        .lines()
        .filter_map(|line| clean_line(line, &mut state))
        .collect();
    cut(&collapse(&lines.join(" ")))
}

/// `raw` in the sentences it is read in, as it is written (the chat lights them in the answer): `voiced` makes each
/// what the voice is given.
pub fn sentences(raw: &str) -> Vec<String> {
    let cleaned = cleaned(raw);
    let (mut done, rest) = split_sentences(&cleaned);
    // the last one has no space after its mark: it is a sentence all the same, cut when it is long
    done.extend(cut_long(rest.trim()));
    done.into_iter()
        .map(|piece| piece.replace(PAUSE, ""))
        .filter(|piece| !piece.trim().is_empty())
        .collect()
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
    let mut spoken = dashes_as_pauses(&inline(body));
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

/// A dash between words is a pause: a voice reads "active - no glitches" in one breath, and a comma adds hardly any
/// (Kokoro, 0.06 s). A spaced hyphen, en or em dash, or an em dash between words, is made a comma where the sentence is
/// read in two pieces (`PAUSE`), with a breath between them (0.4 s); a hyphen inside a word or a number
/// ("well-known", "1-2") is left as it is.
pub fn dashes_as_pauses(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let dash = matches!(c, '-' | '\u{2013}' | '\u{2014}');
        let spaced =
            dash && i > 0 && chars[i - 1] == ' ' && chars.get(i + 1).is_some_and(|n| *n == ' ');
        let em_between = c == '\u{2014}'
            && i > 0
            && chars[i - 1].is_alphanumeric()
            && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
        if spaced {
            // "active - no": the space before it goes, a comma and the pause take its place
            while out.ends_with(' ') {
                out.pop();
            }
            if !out.is_empty() {
                if !out.ends_with([',', '.', ';', ':', '!', '?']) {
                    out.push(',');
                }
                out.push(PAUSE);
            }
        } else if em_between {
            out.push(',');
            out.push(PAUSE);
            out.push(' ');
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// A sentence as the voice is given it: short forms said in words ("e.g." is "for example"), an acronym spelled
/// ("APIs" is "eigh P I's", for a voice reads "API" as a word and drops the plural of "GPUs"), and the symbols left out
/// unless they are read (`symbols`).
pub fn voiced(sentence: &str, symbols: bool) -> String {
    let words: Vec<&str> = sentence.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    for (k, word) in words.iter().enumerate() {
        let last = k + 1 == words.len();
        let next_is_digit = words
            .get(k + 1)
            .is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit()));
        // the number a unit follows: "1.1 GB" is "1.1 gigabytes", "1 GB" "1 gigabyte"
        let after = k.checked_sub(1).map(|p| words[p]).filter(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
        });
        if let Some(unit) = after.and_then(|number| unit_word(word, number == "1")) {
            out.push(unit);
            continue;
        }
        out.push(say_word(word, last, next_is_digit));
    }
    let said = out.join(" ");
    if symbols {
        said
    } else {
        without_symbols(&said)
    }
}

/// Units after a number, and the words they are said as (in the plural).
const UNITS: &[(&str, &str)] = &[
    ("KB", "kilobytes"),
    ("MB", "megabytes"),
    ("GB", "gigabytes"),
    ("TB", "terabytes"),
    ("KiB", "kibibytes"),
    ("MiB", "mebibytes"),
    ("GiB", "gibibytes"),
    ("TiB", "tebibytes"),
    ("Hz", "hertz"),
    ("kHz", "kilohertz"),
    ("MHz", "megahertz"),
    ("GHz", "gigahertz"),
    ("ms", "milliseconds"),
    ("fps", "frames per second"),
];

/// `word` as a unit after a number, in words (`one`: after 1, in the singular), with what follows it kept.
fn unit_word(word: &str, one: bool) -> Option<String> {
    let core = word.trim_end_matches([',', ';', ':', '!', '?', ')', '.']);
    let tail = &word[core.len()..];
    let (_, said) = UNITS.iter().find(|(unit, _)| *unit == core)?;
    let said = if one && !said.contains(' ') && said.ends_with('s') && *said != "hertz" {
        &said[..said.len() - 1]
    } else {
        said
    };
    Some(format!("{said}{tail}"))
}

/// Short forms and the words they are said as, matched without regard to case (with their dots, when they have some).
const SHORT_FORMS: &[(&str, &str)] = &[
    ("e.g.", "for example"),
    ("i.e.", "that is"),
    ("etc.", "et cetera"),
    ("etc", "et cetera"),
    ("vs.", "versus"),
    ("vs", "versus"),
    ("approx.", "approximately"),
    ("approx", "approximately"),
    ("incl.", "including"),
    ("excl.", "excluding"),
    ("a.k.a.", "also known as"),
    ("aka", "also known as"),
    ("w/o", "without"),
    ("w/", "with"),
    ("btw", "by the way"),
    ("fyi", "for your information"),
    ("imo", "in my opinion"),
    ("imho", "in my humble opinion"),
    ("tbd", "to be decided"),
    ("asap", "as soon as possible"),
    ("pls", "please"),
    ("thx", "thanks"),
    ("mr.", "mister"),
    ("mrs.", "missus"),
    ("dr.", "doctor"),
];

/// Words in capitals that are said as words, not spelled: acronyms read as words, and words written in capitals.
const SAID_AS_WORDS: &[&str] = &[
    "NASA", "NATO", "JSON", "YAML", "TOML", "RAM", "ROM", "GIF", "JPEG", "ASCII", "CUDA", "UNIX",
    "LINUX", "WASM", "SCUBA", "LASER", "RADAR", "LIDAR", "SONAR", "AIDS", "COVID", "FIFA",
    "UNICEF", "UNESCO", "OPEC", "CAPTCHA", "SIM", "PIN", "LAN", "WAN", "CRUD", "REST", "SOAP",
    "AJAX", "BIOS", "GNU", "GNOME", "MIDI", "NAND", "DOS", "IKEA", "NOT", "NO", "YES", "ALL",
    "AND", "THE", "FOR", "NEW", "NOW", "YOU", "ARE", "BUT", "CAN", "NEVER", "ALWAYS", "ONLY",
    "VERY", "MUST", "STOP", "NOTE", "DO", "DONT", "FREE", "HELP", "ONE", "TWO", "TOP", "BIG",
    "BEST", "WILL", "NEED", "NEXT", "LAST", "FIRST", "MORE", "LESS", "KEY", "HOT", "OFF", "ON",
    "OR", "IF", "IS", "IT", "IN", "TO", "OF", "AT", "BY", "UP", "SO", "GO", "BE", "MY", "ME", "WE",
    "HE", "SHE", "HIS", "HER", "OUR", "OUT", "DONE", "TRUE", "FALSE", "NULL", "NONE", "GOOD",
    "BAD", "FAST", "SLOW", "START", "END", "OPEN", "CLOSE", "READ", "WRITE", "SAVE", "EXIT",
    "QUIT", "HOME", "WAIT", "WARN", "INFO", "DEBUG", "ERROR", "TODO", "FIXME", "HELLO", "THANK",
    "THANKS", "PLEASE", "WOW", "NEWS", "SALE", "YET", "AGAIN", "WHAT", "WHY", "HOW", "WHO", "WHEN",
    "WHERE",
];

/// One word as it is said: a short form in words, an acronym spelled, else itself.
fn say_word(word: &str, last: bool, next_is_digit: bool) -> String {
    const OPENING: &[char] = &['(', '[', '{', '"', '\'', '\u{201C}', '\u{2018}'];
    const CLOSING: &[char] = &[
        ',', ';', ':', '!', '?', ')', ']', '}', '"', '\'', '\u{201D}', '\u{2019}', '.',
    ];
    let start = word.len() - word.trim_start_matches(OPENING).len();
    let (lead, body) = word.split_at(start);
    let lower = body.to_lowercase();
    // "No." before a number is "number"
    if next_is_digit && (lower == "no." || lower == "no") && body.starts_with('N') {
        return format!("{lead}number");
    }
    for (short, said) in SHORT_FORMS {
        let Some(rest) = lower.strip_prefix(short) else {
            continue;
        };
        if !rest.chars().all(|c| CLOSING.contains(&c)) {
            continue;
        }
        let rest = &body[body.len() - rest.len()..];
        let mut said = said.to_string();
        if body.starts_with(char::is_uppercase) {
            said = capitalised(&said);
        }
        // the dot of a short form that ends the sentence ends it still
        let stop = if last && short.ends_with('.') && rest.is_empty() {
            "."
        } else {
            ""
        };
        return format!("{lead}{said}{rest}{stop}");
    }
    let core = body.trim_end_matches(CLOSING);
    let tail = &body[core.len()..];
    let (letters, plural) = if let Some(l) = core
        .strip_suffix("'s")
        .or_else(|| core.strip_suffix("\u{2019}s"))
    {
        (l, true)
    } else if let Some(l) = core.strip_suffix('s') {
        (l, true)
    } else {
        (core, false)
    };
    let acronym = (2..=6).contains(&letters.len())
        && letters.chars().all(|c| c.is_ascii_uppercase())
        && !SAID_AS_WORDS.contains(&letters)
        && !(plural && SAID_AS_WORDS.contains(&core));
    if !acronym {
        return word.to_string();
    }
    // each letter by its name: "A" alone is read as the word "a", "eigh" as the letter
    let mut spelled: Vec<String> = letters
        .chars()
        .map(|c| {
            if c == 'A' {
                "eigh".to_string()
            } else {
                c.to_string()
            }
        })
        .collect();
    if plural {
        if let Some(last) = spelled.last_mut() {
            last.push_str("'s");
        }
    }
    format!("{lead}{}{tail}", spelled.join(" "))
}

fn capitalised(words: &str) -> String {
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `text` without the symbols a voice reads by their names ("hash", "slash", "rightwards arrow", "grinning face"), each
/// made a space. Kept: letters and digits of every script with their marks, the marks of a sentence, quotes, brackets,
/// hyphens and dashes, and the symbols that are read as words (`% $ € £ ¥ & °`, and a slash between digits, as in 1/2).
pub fn without_symbols(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        let between_digits = i > 0
            && chars[i - 1].is_ascii_digit()
            && chars.get(i + 1).is_some_and(char::is_ascii_digit);
        let symbol = match c {
            '/' => !between_digits,
            '#' | '*' | '_' | '`' | '~' | '^' | '|' | '\\' | '<' | '>' | '=' | '+' | '@' | '['
            | ']' | '{' | '}' => true,
            // bullets, daggers, marks of a section or a trade
            '\u{2022}' | '\u{2023}' | '\u{2043}' | '\u{00B7}' | '\u{2020}' | '\u{2021}'
            | '\u{00A7}' | '\u{00B6}' | '\u{00A9}' | '\u{00AE}' | '\u{2122}' | '\u{203B}' => true,
            // emoji's joiners and selectors, and the keycap
            '\u{200D}' | '\u{FE0E}' | '\u{FE0F}' | '\u{20E3}' => true,
            _ => matches!(c as u32,
                // arrows, maths, the technical, box drawing, blocks, shapes, symbols, dingbats, more arrows
                0x2190..=0x23FF | 0x2500..=0x27BF | 0x27F0..=0x27FF | 0x2900..=0x297F | 0x2B00..=0x2BFF
                // emoji and pictographs
                | 0x1F000..=0x1FAFF),
        };
        out.push(if symbol { ' ' } else { c });
    }
    collapse(&out)
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
            // a dash's pause: what comes before it is read as a piece of its own
            PAUSE => true,
            '.' | '!' | '?' | '…' => next.is_some_and(char::is_whitespace),
            // a full stop of Chinese and Japanese, of Hindi, and of Arabic, Persian and Urdu needs no space after it
            '。' | '！' | '？' | '।' | '॥' | '؟' | '۔' => true,
            _ => false,
        };
        if !ends {
            continue;
        }
        let end = at + c.len_utf8();
        let sentence = text[start..end].trim().trim_end_matches(PAUSE).trim();
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
    /// Words of a line that was begun already: its inline marks left out, its dashes made pauses.
    fn speakable(&self, text: &str) -> String {
        collapse(&dashes_as_pauses(&inline(text)))
    }

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
                Some(self.speakable(unsaid.trim_end_matches('\n')))
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
                let words = self.speakable(&unsaid[..end]);
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
        let sentence = sentence.replace(PAUSE, "");
        if self.total >= MAX_CHARS || sentence.trim().is_empty() {
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
    fn short_forms_are_said_in_words_and_acronyms_spelled() {
        assert_eq!(
            voiced("Use APIs, e.g. GPUs and LLMs etc.", false),
            "Use eigh P I's, for example G P U's and L L M's et cetera."
        );
        assert_eq!(
            voiced("The API's key, i.e. the ID.", false),
            "The eigh P I's key, that is the I D."
        );
        assert_eq!(
            voiced("E.g. this vs. that w/o help", false),
            "For example this versus that without help"
        );
        assert_eq!(
            voiced("Read the JSON from NASA, NOT the CSV.", false),
            "Read the JSON from NASA, NOT the C S V."
        );
        assert_eq!(
            voiced("See No. 5 and (URLs).", false),
            "See number 5 and (U R L's)."
        );
        assert_eq!(voiced("I am OK, an AI.", false), "I am O K, an eigh I.");
        // a word with digits or lower case letters is left to the voice
        assert_eq!(
            voiced("GPT-4 on an H100 with Rust", false),
            "GPT-4 on an H100 with Rust"
        );
        assert_eq!(
            voiced("It is 1.1 GB, 1 GB at 3.2 GHz in 40 ms.", false),
            "It is 1.1 gigabytes, 1 gigabyte at 3.2 gigahertz in 40 milliseconds."
        );
    }

    #[test]
    fn a_dash_between_words_is_a_pause() {
        assert_eq!(
            for_speech("I'm active - no glitches"),
            "I'm active, no glitches"
        );
        assert_eq!(
            sentences("I'm active - no glitches."),
            ["I'm active,", "no glitches."]
        );
        assert_eq!(for_speech("Fast \u{2014} and small"), "Fast, and small");
        assert_eq!(for_speech("fast\u{2014}small"), "fast, small");
        assert_eq!(
            for_speech("A well-known 1-2 step, x -1"),
            "A well-known 1-2 step, x -1"
        );
        assert_eq!(for_speech_with("on - off", true), "on, off");
        let mut feed = SentenceFeed::default();
        let mut heard = feed.push("Ready - set. Go");
        heard.extend(feed.finish());
        assert_eq!(heard, ["Ready,", "set.", "Go"]);
    }

    #[test]
    fn symbols_are_left_out_unless_they_are_read_as_words_and_kept_when_asked() {
        assert_eq!(
            for_speech("Use the A → B path #2 with C++ and/or x = y @home 😀 • done"),
            "Use the A B path 2 with C and or x y home done"
        );
        assert_eq!(
            for_speech("It costs $5, 20% off, 1/2 of it & 20°C."),
            "It costs $5, 20% off, 1/2 of it & 20°C."
        );
        assert_eq!(for_speech("the snake_case name"), "the snake case name");
        // the marks of other scripts are not symbols
        assert_eq!(for_speech("മലയാളം हिन्दी 中文。"), "മലയാളം हिन्दी 中文。");
        assert_eq!(for_speech_with("A → B #2", true), "A → B #2");
        let mut feed = SentenceFeed::default();
        let mut heard = feed.push("Arrows → go here. ");
        heard.extend(feed.finish());
        assert_eq!(heard, ["Arrows → go here."]);
        assert_eq!(voiced(&heard[0], false), "Arrows go here.");
    }

    #[test]
    fn marks_are_not_read() {
        assert_eq!(
            for_speech("This is **very** _important_ and `cargo run`."),
            "This is very important and cargo run."
        );
        assert_eq!(for_speech("keep snake_case names"), "keep snake case names");
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
            "Name, Size. Bonsai, 1.1 gigabytes."
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
