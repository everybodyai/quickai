//! The pure half of `lookup.ollama_library@1`: the address of a search of Ollama's library and the
//! reading of the page that comes back.
//!
//! ollama.com publishes no listing of its library that a program can read (docs/engine-facts.md,
//! O23): `/api/tags` lists the models of its cloud service, the registry cannot list names, and the
//! only list there is, is the page behind the site's own search box. So this reads that page the
//! way a person does, and is written to lose as little as possible when the page changes, which it
//! will:
//!
//! - a card is the link to `/library/<name>` and what lies between it and its closing tag. The name
//!   is the one part a row cannot do without; every other part is taken from where it was seen (a
//!   `title`, the words next to an icon), never from how it is styled, so no class name is read;
//! - what cannot be found is left out (a model with no sizes is pulled by its name), and a page that
//!   has results this reading cannot find is told from a page that has none, so that the person is
//!   told the site looks different and not that nothing matched;
//! - nothing read from the page is trusted: a name that is not a model name, a size that is not a
//!   tag and a number that is not a number are dropped, and a page of any shape or length is read
//!   without a panic.

use std::collections::BTreeSet;

/// Where the library is, unless the document says another place.
pub const SITE: &str = "https://ollama.com";

/// The most of what was typed that is asked about: longer is not a search a person typed.
const MAX_QUERY_CHARS: usize = 100;
/// A page holds twenty; this is the bound against a page that holds a great many more.
const MAX_MODELS: usize = 50;
const MAX_DESCRIPTION_CHARS: usize = 400;
/// What one card may be: a card the closing tag of which is lost does not swallow the page.
const MAX_CARD_BYTES: usize = 64 * 1024;
const MAX_NAME_CHARS: usize = 80;
const MAX_SIZE_CHARS: usize = 12;
const MAX_CAPABILITIES: usize = 8;
const MAX_CAPABILITY_CHARS: usize = 20;

/// One model of the library, as the search page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryModel {
    /// `qwen3`: what `ollama pull` takes, and the one part a row cannot do without.
    pub name: String,
    /// The sentence the library gives it; empty when the card has none.
    pub description: String,
    /// The sizes it is offered in, as the tags the card shows (`0.6b`, `8b`, `e2b`), in the card's
    /// order. Empty when the card shows none: then only the default tag is known.
    pub sizes: Vec<String>,
    /// What the card says it can do (`tools`, `thinking`, `vision`, `embedding`, ...), lower case,
    /// in the card's order. `cloud` is left out: a cloud variant is nothing this application runs.
    pub capabilities: Vec<String>,
    /// How many times it was pulled, when the card says.
    pub downloads: Option<u64>,
}

impl LibraryModel {
    /// What `ollama pull` is given for this model: `qwen3:4b` for a size, `qwen3` for its default.
    pub fn reference(&self, size: Option<&str>) -> String {
        match size {
            Some(size) => format!("{}:{size}", self.name),
            None => self.name.clone(),
        }
    }

    /// What this look-up learned of the model, in the words every look-up uses, for `present`.
    pub fn candidate(&self) -> super::candidate::Candidate {
        super::candidate::Candidate {
            key: format!("ollama:{}", self.name),
            name: self.name.clone(),
            description: self.description.clone(),
            downloads: self.downloads,
            likes: None,
            capabilities: self.capabilities.clone(),
            pipeline: None,
            tags: Vec::new(),
            origin: super::candidate::Origin::Library {
                name: self.name.clone(),
                sizes: self.sizes.clone(),
            },
        }
    }
}

/// What a page of the search says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchPage {
    pub models: Vec<LibraryModel>,
    /// The site's own count of the results on the page, when it gives one.
    pub reported: Option<usize>,
    /// The site says there are results beyond this page.
    pub more: bool,
}

/// Why a page gave nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The page says it has results and none could be found in it, or it is not a search page.
    Unreadable,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Unreadable => f.write_str(
                "the page is not one this version of QuickAI can read (the site may have changed)",
            ),
        }
    }
}

/// The address of the search for `query` at `site`, for models that run on this machine. Only the
/// words matter: a `:tag` after them is not asked about (`qwen3:4b` looks for `qwen3`), and nothing
/// typed is the site's own list of what is popular.
pub fn search_url(site: &str, query: &str) -> String {
    let site = site.trim().trim_end_matches('/');
    let words = search_words(query);
    if words.is_empty() {
        format!("{site}/search?c=local")
    } else {
        format!("{site}/search?q={}&c=local", percent_encode(&words))
    }
}

/// The words a search is made of: what was typed, up to a `:`, with one space between the words
/// and no more than a person would type. Empty when nothing is asked about.
pub fn search_words(query: &str) -> String {
    let words = query.split(':').next().unwrap_or("");
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    let words: String = words.chars().take(MAX_QUERY_CHARS).collect();
    words.trim().to_string()
}

/// A query-string value per RFC 3986: unreserved characters pass, every other byte is `%XX`.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Reads one page of the search.
pub fn parse_search(html: &str) -> Result<SearchPage, ReadError> {
    let region = match html
        .find("id=\"searchresults\"")
        .or_else(|| html.find("id='searchresults'"))
    {
        Some(at) => &html[at..],
        None => html,
    };
    let reported = number_after(region, "data-result-count=\"").map(|n| n as usize);
    let more = region.contains("data-has-more=\"true\"");

    let mut models = Vec::new();
    let mut seen = BTreeSet::new();
    for card in cards(region) {
        if models.len() >= MAX_MODELS {
            break;
        }
        if let Some(model) = read_card(&card) {
            if seen.insert(model.name.clone()) {
                models.push(model);
            }
        }
    }
    if models.is_empty() {
        // a page that says it has no results, in its own count or its own words, is an answer
        let says_none =
            reported == Some(0) || (reported.is_none() && region.contains("No models found"));
        if !says_none {
            return Err(ReadError::Unreadable);
        }
    }
    Ok(SearchPage {
        models,
        reported,
        more,
    })
}

/// The digits that follow `marker`, when there are some.
fn number_after(text: &str, marker: &str) -> Option<u64> {
    let rest = &text[text.find(marker)? + marker.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

// ---- cards -------------------------------------------------------------------------------------------

struct Card<'a> {
    /// What the link says the model is called.
    name: String,
    /// Everything between the link's opening tag and its closing tag.
    body: &'a str,
}

/// The links to `/library/<name>` and what is inside them, in page order.
fn cards(region: &str) -> Vec<Card<'_>> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(offset) = region[at..].find("<a") {
        let start = at + offset;
        at = start + 2;
        // `<a` begins a link only when a space (or a line end) follows: not `<abbr`, not `<article`
        if !region[at..].starts_with(|c: char| c.is_ascii_whitespace()) {
            continue;
        }
        let Some(end) = tag_end(region, start) else {
            break;
        };
        let attributes = &region[start + 2..end];
        let Some(href) = attribute(attributes, "href") else {
            continue;
        };
        let Some(rest) = href.strip_prefix("/library/") else {
            continue;
        };
        let name_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let name = &rest[..name_end];
        if !valid_name(name) {
            continue;
        }
        let after = end + 1;
        let limit = cut(&region[after..], MAX_CARD_BYTES).len();
        let close = region[after..after + limit].find("</a>").unwrap_or(limit);
        found.push(Card {
            name: name.to_string(),
            body: &region[after..after + close],
        });
        at = after + close;
    }
    found
}

fn read_card(card: &Card<'_>) -> Option<LibraryModel> {
    let tokens = tokens(card.body);

    let description = tokens
        .iter()
        .enumerate()
        .find_map(|(i, token)| match token {
            Token::Open { name, attributes } if name.eq_ignore_ascii_case("p") => {
                let titled = attribute(attributes, "title").filter(|t| !t.trim().is_empty());
                // no title: the text of the paragraph, markup inside it left out
                Some(titled.or_else(|| {
                    let words: Vec<String> = tokens[i + 1..]
                        .iter()
                        .take_while(
                            |t| !matches!(t, Token::Close(n) if n.eq_ignore_ascii_case("p")),
                        )
                        .filter_map(|t| match t {
                            Token::Text(text) => Some(unescape(text)),
                            _ => None,
                        })
                        .collect();
                    (!words.is_empty()).then(|| words.join(" "))
                }))
            }
            _ => None,
        })
        .flatten()
        .map(|d| shorten(&collapse(&d), MAX_DESCRIPTION_CHARS))
        .unwrap_or_default();

    let downloads = tokens.iter().find_map(|token| match token {
        Token::Open { attributes, .. } => attribute(attributes, "title").and_then(|t| pulls(&t)),
        _ => None,
    });

    Some(LibraryModel {
        name: card.name.clone(),
        description,
        sizes: sizes(&tokens),
        capabilities: capabilities(&tokens),
        downloads,
    })
}

/// `3,301,495 downloads` is 3301495.
fn pulls(title: &str) -> Option<u64> {
    let lower = title.trim().to_ascii_lowercase();
    let count = lower
        .strip_suffix(" downloads")
        .or_else(|| lower.strip_suffix(" pulls"))?;
    if count.is_empty()
        || !count
            .chars()
            .all(|c| c.is_ascii_digit() || ",. ".contains(c))
    {
        return None;
    }
    let digits: String = count.chars().filter(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The sizes that follow the words that say the model runs on this computer: `22m · 33m · 110m`.
/// They are what is shown after that tooltip and look like a size, up to the first thing that does
/// not (another tooltip, an icon's word).
fn sizes(tokens: &[Token<'_>]) -> Vec<String> {
    // the description is searched for nothing: it may say anything, "your computer" among it
    let after_description = tokens
        .iter()
        .position(|token| matches!(token, Token::Close(name) if name.eq_ignore_ascii_case("p")))
        .map_or(0, |i| i + 1);
    let Some(from) = tokens[after_description..]
        .iter()
        .position(|token| match token {
            Token::Text(text) => text.to_ascii_lowercase().contains("computer"),
            _ => false,
        })
    else {
        return Vec::new();
    };
    let mut found: Vec<String> = Vec::new();
    for token in &tokens[after_description + from + 1..] {
        let Token::Text(text) = token else { continue };
        let text = unescape(text);
        let text = text.trim();
        if text.is_empty() || (text.chars().count() == 1 && "·•,|/".contains(text)) {
            continue;
        }
        if !valid_size(text) {
            break;
        }
        if !found.iter().any(|s| s == text) {
            found.push(text.to_string());
        }
    }
    found
}

/// The words that sit right after an icon: `<svg>…</svg>Tools`. A size or a count has an element
/// of its own between the icon and the text, so neither is taken for one.
fn capabilities(tokens: &[Token<'_>]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for pair in tokens.windows(2) {
        let (Token::Close(closed), Token::Text(text)) = (&pair[0], &pair[1]) else {
            continue;
        };
        if !closed.eq_ignore_ascii_case("svg") {
            continue;
        }
        let word = unescape(text).trim().to_ascii_lowercase();
        let plausible = !word.is_empty()
            && word.chars().count() <= MAX_CAPABILITY_CHARS
            && word.chars().all(|c| c.is_ascii_lowercase());
        if plausible && word != "cloud" && !found.contains(&word) && found.len() < MAX_CAPABILITIES
        {
            found.push(word);
        }
    }
    found
}

// ---- what is acceptable ---------------------------------------------------------------------------------

/// A model's name in the library: lower case, digits, dots, dashes and underscores, beginning with a
/// letter or a digit and with no `..` in it. A name with anything else is not offered, because it
/// would not be one the model box takes either.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && !name.contains("..")
        && name.chars().count() <= MAX_NAME_CHARS
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
}

/// A size the library shows: `270m`, `0.6b`, `70b`, `8x7b`, `e2b`, `1t`.
pub fn valid_size(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_SIZE_CHARS {
        return false;
    }
    let mut i = 0;
    if bytes[i].is_ascii_lowercase() && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
        i += 1; // the `e` of `e2b`
    }
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let whole = digits(i);
    if whole == 0 {
        return false;
    }
    i += whole;
    if bytes.get(i) == Some(&b'.') {
        let fraction = digits(i + 1);
        if fraction == 0 {
            return false;
        }
        i += 1 + fraction;
    }
    if bytes.get(i) == Some(&b'x') {
        let times = digits(i + 1);
        if times == 0 {
            return false;
        }
        i += 1 + times;
    }
    i + 1 == bytes.len() && matches!(bytes[i], b'k' | b'm' | b'b' | b't')
}

// ---- reading markup ---------------------------------------------------------------------------------------

#[derive(Debug)]
enum Token<'a> {
    Open { name: &'a str, attributes: &'a str },
    Close(&'a str),
    Text(&'a str),
}

/// The tags and the text of a piece of markup, in order. Comments and white space between tags are
/// dropped; a `<` that does not begin a tag is text.
fn tokens(source: &str) -> Vec<Token<'_>> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < source.len() {
        let rest = &source[at..];
        if rest.starts_with("<!--") {
            at += rest.find("-->").map_or(rest.len(), |i| i + 3);
        } else if rest.starts_with("</") {
            let end = rest.find('>').unwrap_or(rest.len());
            found.push(Token::Close(rest[2..end].trim()));
            at += end + 1;
        } else if rest.starts_with('<') && rest[1..].starts_with(|c: char| c.is_ascii_alphabetic())
        {
            let end = tag_end(source, at).unwrap_or(source.len());
            let inner = &source[at + 1..end];
            let name_len = inner
                .find(|c: char| c.is_ascii_whitespace() || c == '/')
                .unwrap_or(inner.len());
            found.push(Token::Open {
                name: &inner[..name_len],
                attributes: &inner[name_len..],
            });
            at = end + 1;
        } else {
            let first = rest.chars().next().map_or(1, char::len_utf8);
            let next = rest[first..].find('<').map_or(rest.len(), |i| first + i);
            let text = rest[..next].trim();
            if !text.is_empty() {
                found.push(Token::Text(text));
            }
            at += next;
        }
    }
    found
}

/// The position of the `>` that ends the tag that begins at `start`, with quoted values skipped
/// (an attribute may hold a `>`); `None` when the tag is never closed.
fn tag_end(source: &str, start: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (i, b) in source.bytes().enumerate().skip(start + 1) {
        match (quote, b) {
            (None, b'"' | b'\'') => quote = Some(b),
            (Some(q), _) if b == q => quote = None,
            (None, b'>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// The value of attribute `key` in the text of a tag after its name, entities decoded.
fn attribute(attributes: &str, key: &str) -> Option<String> {
    let bytes = attributes.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        let name = &attributes[name_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = "";
        if bytes.get(i) == Some(&b'=') {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            match bytes.get(i) {
                Some(&q @ (b'"' | b'\'')) => {
                    let begin = i + 1;
                    let close = attributes[begin..]
                        .find(q as char)
                        .map_or(attributes.len(), |c| begin + c);
                    value = &attributes[begin..close];
                    i = (close + 1).min(attributes.len());
                }
                Some(_) => {
                    let begin = i;
                    while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    value = &attributes[begin..i];
                }
                None => {}
            }
        }
        if !name.is_empty() && name.eq_ignore_ascii_case(key) {
            return Some(unescape(value));
        }
    }
    None
}

/// `&amp;`, `&#39;`, `&#x27;` and the few more a page uses; what is not understood stays as it is.
fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').filter(|end| *end <= 10).and_then(|end| {
            let entity = &rest[1..end];
            let ch = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => entity
                    .strip_prefix('#')
                    .and_then(|n| match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse().ok(),
                    })
                    .and_then(char::from_u32),
            };
            // a control character in a name or a sentence would only be trouble
            ch.filter(|c| !c.is_control() || *c == '\t' || *c == '\n')
                .map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters, with an ellipsis where it was cut.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// `text` cut to at most `max` bytes, at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
