//! `lookup.huggingface_gguf@1`: the GGUF repositories of Hugging Face (docs/lookup-and-bundles.md,
//! section 5). The engine document gives the site and how many repositories to look into; this
//! module asks the site's API, hands each answer to `quickai_core::lookup::huggingface` to be read,
//! makes each repository into an entry the window draws, and reports them in the shape of the other
//! look-up (a status line, a `Found` for each, `Done`).
//!
//! Two kinds of request, made when a person presses a button and never before: a search of GGUF
//! repositories (the most downloaded, or for the words typed, and then for each word of a phrase
//! alone, to find what the phrase misses), and for each repository worth looking into the list of its
//! files with their sizes. The calls are one after the other, the first of them the one whose failure
//! is the look-up's: a repository that cannot be read is left out, a site that begins to refuse
//! (HTTP 429) ends the look-up with what it has, and neither is ever a list that looks empty.

use std::collections::HashSet;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use quickai_core::lookup::huggingface::{
    candidate, parse_repo, parse_search, quantized_of_url, repo_url, search_url, search_url_in,
    search_words, similar_terms, typed_repo, Hit, Repo,
};
use quickai_core::lookup::present::{Entry, OwnedRules};

use super::lookup::{fetch, host_of, FetchError, LookupEvent, Request, CALL_TIMEOUT};

/// The answer of a search is a list of fifty repositories; of a repository, its files. Neither is
/// anywhere near this, and an answer of more is not what was asked for.
const MAX_ANSWER_BYTES: u64 = 4 * 1024 * 1024;

/// What the document says to ask.
#[derive(Clone, Debug)]
pub struct HubSpec {
    pub site: String,
    /// How many repositories to look into for the plain list, for the words typed, and for each of
    /// their words alone.
    pub popular: usize,
    pub keyword: usize,
    pub similar: usize,
    /// The pipelines (`text-to-video`) the plain list is made of: one search each, `popular` of the
    /// repositories of each looked into. None: the most downloaded of any kind.
    pub pipelines: Vec<String>,
    /// Words the plain list is also made of (`encoder`): one search each, looked into the same way.
    pub words: Vec<String>,
}

/// Starts a look-up in a background thread and returns at once with the receiving end of the channel.
/// `query` is what was typed (nothing is the list of the most downloaded); `wake` is called after each
/// event, to have the window drawn.
pub fn spawn_hub_lookup(
    spec: HubSpec,
    query: String,
    rules: OwnedRules,
    wake: impl Fn() + Send + Sync + 'static,
) -> Receiver<LookupEvent> {
    spawn_with(spec, query, rules, CALL_TIMEOUT, wake)
}

fn spawn_with(
    spec: HubSpec,
    query: String,
    rules: OwnedRules,
    timeout: Duration,
    wake: impl Fn() + Send + Sync + 'static,
) -> Receiver<LookupEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::spawn(move || run(&spec, &query, &rules, timeout, &tx, &wake));
    rx
}

const STILL: &str = "a link typed into the box still downloads a model.";
const THING: &str = "an answer of its API";

/// What went on while the repositories were looked into.
#[derive(Default)]
struct Tally {
    /// How many were reported.
    found: usize,
    /// How many needed a login and were left out.
    gated: usize,
    /// How many could not be read.
    skipped: usize,
    /// The site refused: the look-up ended with what it had.
    limited: bool,
    /// What the first repository that could not be read said.
    first_error: Option<String>,
}

fn run(
    spec: &HubSpec,
    query: &str,
    rules: &OwnedRules,
    timeout: Duration,
    tx: &Sender<LookupEvent>,
    wake: &dyn Fn(),
) {
    let send = |event: LookupEvent| {
        let _ = tx.send(event);
        wake();
    };
    let host = host_of(&spec.site);
    let words = search_words(query);
    let request = Request {
        accept: "application/json",
        max_bytes: MAX_ANSWER_BYTES,
        timeout,
    };
    let sentence = |e: &FetchError| e.sentence(&host, &request, THING, STILL);
    let fail = |text: String| {
        send(LookupEvent::Status(text));
        send(LookupEvent::Failed);
        send(LookupEvent::Done);
    };
    // a repository typed (`owner/name`, or a link to its page) is looked up itself first, then the GGUF files others
    // made of it, and then its name, without the owner, is what is searched
    let typed = typed_repo(&words).map(str::to_string);
    let phrase = typed
        .as_deref()
        .map_or(words.as_str(), |id| id.rsplit('/').next().unwrap_or(id))
        .to_string();
    send(LookupEvent::Status(match &typed {
        Some(id) => format!("Looking up {id} on {host}..."),
        None if words.is_empty() => format!("Searching {host} for GGUF models..."),
        None => format!("Searching {host} for \"{words}\"..."),
    }));

    let mut seen: HashSet<String> = HashSet::new();
    let mut tally = Tally::default();
    if let Some(id) = &typed {
        let mut cannot: Option<Repo> = None;
        match fetch(&repo_url(&spec.site, id), &request) {
            Ok(body) => match parse_repo(&body) {
                Ok(repo) if repo.gated => {
                    seen.insert(id.clone());
                    tally.gated += 1;
                    send(LookupEvent::Note(format!(
                        "{id} asks for a login on {host} before it can be downloaded, and QuickAI does not sign \
                         in, so it is left out."
                    )));
                }
                Ok(repo) => {
                    seen.insert(id.clone());
                    let hit = Hit {
                        id: id.clone(),
                        downloads: 0,
                        likes: 0,
                        pipeline: repo.pipeline.clone(),
                        tags: repo.tags.clone(),
                    };
                    let entry: Entry = rules.present(&candidate(&spec.site, &hit, &repo));
                    if entry.offers.is_empty() {
                        send(LookupEvent::Note(cannot_run(id, &repo, None)));
                        cannot = Some(repo);
                    } else {
                        tally.found += 1;
                        send(LookupEvent::Found(entry));
                    }
                }
                // an answer that cannot be read: its name is searched as any other words
                Err(_) => {}
            },
            Err(FetchError::Status(401 | 404)) => send(LookupEvent::Note(format!(
                "{id} is not on {host}, or it is private. These are models with similar names."
            ))),
            Err(FetchError::Limited) => tally.limited = true,
            Err(e) => return fail(sentence(&e)),
        }
        if !tally.limited {
            send(LookupEvent::Status(format!(
                "Looking for GGUF versions of {id}..."
            )));
            let before = tally.found;
            // a search that cannot be made loses the versions, not the look-up
            let hits = fetch(&quantized_of_url(&spec.site, id), &request)
                .ok()
                .and_then(|body| parse_search(&body).ok())
                .unwrap_or_default();
            inspect(
                spec,
                &request,
                hits,
                spec.keyword,
                &mut seen,
                &mut tally,
                rules,
                &send,
            );
            if let Some(repo) = &cannot {
                send(LookupEvent::Note(cannot_run(
                    id,
                    repo,
                    Some(tally.found > before),
                )));
            }
            send(LookupEvent::Status(format!(
                "Searching {host} for \"{phrase}\"..."
            )));
        }
    }

    // the plain list of an engine that runs one kind of model is a search for each of its pipelines and for
    // each of its words; words typed are searched as they are, and once more as words apart when they are joined
    // by dashes or underscores (`Llama-3.2-3B` also finds `3B-Llama-3.2`), which neither narrows
    let spaced = phrase
        .split(|c: char| !(c.is_alphanumeric() || c == '.'))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let searches: Vec<(Option<&str>, Option<&str>)> = if words.is_empty() {
        if spec.pipelines.is_empty() && spec.words.is_empty() {
            vec![(None, None)]
        } else {
            spec.pipelines
                .iter()
                .map(|p| (Some(p.as_str()), None))
                .chain(spec.words.iter().map(|w| (None, Some(w.as_str()))))
                .collect()
        }
    } else if spaced.contains(' ') && !spaced.eq_ignore_ascii_case(&phrase) {
        vec![(None, Some(phrase.as_str())), (None, Some(spaced.as_str()))]
    } else {
        vec![(None, Some(phrase.as_str()))]
    };
    let inspected = if words.is_empty() {
        spec.popular
    } else {
        spec.keyword
    };
    for (n, (pipeline, searched)) in searches.iter().enumerate() {
        if tally.limited {
            break;
        }
        if let Some(pipeline) = pipeline {
            send(LookupEvent::Status(format!(
                "Searching {host} for {pipeline} models..."
            )));
        } else if let (Some(word), true) = (searched, words.is_empty()) {
            send(LookupEvent::Status(format!(
                "Searching {host} for \"{word}\"..."
            )));
        }
        // the first request is the look-up's: its failure is said; a search after it that cannot be made loses its
        // models and not the look-up
        let later = n > 0 || typed.is_some();
        let url = search_url_in(&spec.site, *searched, *pipeline);
        let hits = match fetch(&url, &request) {
            Ok(body) => match parse_search(&body) {
                Ok(hits) => hits,
                Err(_) if later => continue,
                Err(_) => {
                    return fail(format!(
                        "{host} answered, but its answer is not one this version of QuickAI can read (the \
                         site may have changed). {}",
                        capitalize(STILL)
                    ));
                }
            },
            Err(_) if later => continue,
            Err(e) => return fail(sentence(&e)),
        };
        inspect(
            spec, &request, hits, inspected, &mut seen, &mut tally, rules, &send,
        );
    }

    if !words.is_empty() && !tally.limited {
        let terms: Vec<String> = similar_terms(&phrase)
            .into_iter()
            .filter(|t| !t.eq_ignore_ascii_case(&phrase) && !t.eq_ignore_ascii_case(&spaced))
            .collect();
        if !terms.is_empty() {
            send(LookupEvent::Status(
                "Looking for similar models...".to_string(),
            ));
        }
        for term in terms {
            if tally.limited {
                break;
            }
            // a pass that cannot be made loses the models of that word, not the look-up
            let Ok(body) = fetch(&search_url(&spec.site, Some(&term)), &request) else {
                continue;
            };
            if let Ok(hits) = parse_search(&body) {
                inspect(
                    spec,
                    &request,
                    hits,
                    spec.similar,
                    &mut seen,
                    &mut tally,
                    rules,
                    &send,
                );
            }
        }
    }

    if tally.found == 0 && (tally.skipped > 0 || tally.limited) {
        let why = if tally.limited {
            FetchError::Limited.sentence(&host, &request, THING, STILL)
        } else {
            tally
                .first_error
                .clone()
                .unwrap_or_else(|| format!("{host} could not be read."))
        };
        return fail(why);
    }
    send(LookupEvent::Status(final_sentence(&host, &words, &tally)));
    send(LookupEvent::Done);
}

/// Looks into the first `limit` repositories of `hits` that were not seen, reporting each that has
/// weights to fetch.
#[allow(clippy::too_many_arguments)]
fn inspect(
    spec: &HubSpec,
    request: &Request<'_>,
    hits: Vec<Hit>,
    limit: usize,
    seen: &mut HashSet<String>,
    tally: &mut Tally,
    rules: &OwnedRules,
    send: &dyn Fn(LookupEvent),
) {
    let host = host_of(&spec.site);
    for hit in hits
        .into_iter()
        .filter(|h| seen.insert(h.id.clone()))
        .take(limit)
    {
        if tally.limited {
            return;
        }
        let body = match fetch(&repo_url(&spec.site, &hit.id), request) {
            Ok(body) => body,
            Err(FetchError::Limited) => {
                tally.limited = true;
                return;
            }
            Err(e) => {
                tally.skipped += 1;
                tally
                    .first_error
                    .get_or_insert_with(|| e.sentence(&host, request, THING, STILL));
                continue;
            }
        };
        let Ok(repo) = parse_repo(&body) else {
            tally.skipped += 1;
            continue;
        };
        if repo.gated {
            tally.gated += 1;
            continue;
        }
        let entry: Entry = rules.present(&candidate(&spec.site, &hit, &repo));
        if entry.offers.is_empty() {
            continue;
        }
        tally.found += 1;
        send(LookupEvent::Found(entry));
    }
}

fn final_sentence(host: &str, words: &str, tally: &Tally) -> String {
    let mut sentence = if tally.found == 0 {
        if words.is_empty() {
            format!("{host} lists no GGUF models.")
        } else {
            format!("No models found on {host} for \"{words}\".")
        }
    } else {
        format!(
            "Found {} model{} on {host}.",
            tally.found,
            if tally.found == 1 { "" } else { "s" }
        )
    };
    if tally.gated > 0 {
        sentence.push_str(&format!(
            " {} that need{} a login {} left out.",
            tally.gated,
            if tally.gated == 1 { "s" } else { "" },
            if tally.gated == 1 { "is" } else { "are" }
        ));
    }
    if tally.skipped > 0 {
        sentence.push_str(&format!(" {} could not be read.", tally.skipped));
    }
    if tally.limited {
        sentence.push_str(&format!(
            " {host} began limiting requests from this network (HTTP 429), so there may be more."
        ));
    }
    sentence
}

/// What is said of a repository that was typed and has nothing QuickAI can fetch: that it has no GGUF file and what
/// its weights are in, what kind of model the hub says it is (with where QuickAI's own speech models are, for one of
/// speech), and, once they were looked for, whether GGUF versions of it made by others are in the results.
fn cannot_run(id: &str, repo: &Repo, versions: Option<bool>) -> String {
    let has_gguf = repo
        .files
        .iter()
        .any(|f| f.path.to_ascii_lowercase().ends_with(".gguf"));
    let mut said = if has_gguf {
        format!("{id} has no model file QuickAI can run.")
    } else {
        let largest = repo
            .files
            .iter()
            .filter(|f| f.size.is_some())
            .max_by_key(|f| f.size)
            .map(|f| f.path.rsplit('/').next().unwrap_or(&f.path).to_string());
        match largest {
            Some(name) => format!(
                "{id} has no GGUF file, which is what QuickAI downloads from Hugging Face: its largest file is {name}."
            ),
            None => format!("{id} has no GGUF file, which is what QuickAI downloads from Hugging Face."),
        }
    };
    match repo.pipeline.as_deref() {
        Some("automatic-speech-recognition") => said.push_str(
            " It is a speech-recognition model: the speech models QuickAI uses are in Settings › Voice.",
        ),
        Some("text-to-speech") => said.push_str(
            " It is a text-to-speech model: the voices QuickAI reads aloud with are in Settings › Voice.",
        ),
        Some(pipeline) => said.push_str(&format!(" Hugging Face lists it as {pipeline}.")),
        None => {}
    }
    match versions {
        Some(true) => said.push_str(" GGUF versions of it made by others are listed first."),
        Some(false) => {
            said.push_str(" No GGUF version of it was found; these are models with similar names.")
        }
        None => {}
    }
    said
}

/// `text` with its first letter a capital.
pub(crate) fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::fake_hub::{Behaviour, FakeHub};
    use crate::testkit::mini_http::{MiniServer, Request};
    use quickai_core::lookup::present::Fetch;

    /// What llama.cpp's look-up reads what it found with: the documents the application ships.
    fn rules() -> OwnedRules {
        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        let engine = registry.engine("llamacpp").expect("the llama.cpp document");
        quickai_core::models::lookup_rules(
            &registry,
            engine,
            &quickai_core::models::lookups(engine)[0],
        )
    }

    fn spec(site: &str) -> HubSpec {
        HubSpec {
            site: site.to_string(),
            popular: 30,
            keyword: 20,
            similar: 20,
            pipelines: Vec::new(),
            words: Vec::new(),
        }
    }

    /// What a look-up said, in the order it said it.
    #[derive(Debug, Default)]
    struct Said {
        statuses: Vec<String>,
        notes: Vec<String>,
        entries: Vec<Entry>,
        failed: bool,
        done: bool,
    }

    fn collect(rx: &Receiver<LookupEvent>) -> Said {
        let mut said = Said::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(LookupEvent::Status(s)) => said.statuses.push(s),
                Ok(LookupEvent::Found(e)) => said.entries.push(e),
                Ok(LookupEvent::Failed) => said.failed = true,
                Ok(LookupEvent::Note(n)) => said.notes.push(n),
                Ok(LookupEvent::Done) => {
                    said.done = true;
                    break;
                }
                Err(_) => {}
            }
        }
        said
    }

    fn look(site: &str, query: &str) -> Said {
        collect(&spawn_hub_lookup(spec(site), query.into(), rules(), || {}))
    }

    /// The six repositories that were recorded, in the order of the recorded list.
    const RECORDED: [&str; 6] = [
        "mudler/locate-anything.cpp-gguf",
        "unsloth/Qwen3.8-27B-GGUF",
        "unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF",
        "mixedbread-ai/mxbai-embed-large-v1",
        "mudler/Laguna-XS-2.1-APEX-GGUF",
        "ggml-org/gemma-3-4b-it-GGUF",
    ];

    fn recorded_hub() -> FakeHub {
        let hub = FakeHub::start();
        hub.only(&RECORDED);
        hub
    }

    fn failing(said: &Said, words: &[&str]) {
        assert!(
            said.done && said.failed && said.entries.is_empty(),
            "{said:?}"
        );
        let sentence = said.statuses.last().unwrap();
        for word in words {
            assert!(sentence.contains(word), "`{word}` is not in: {sentence}");
        }
    }

    #[test]
    fn p3_v10_a_look_up_asks_for_the_list_and_then_for_each_repository_and_reports_what_each_holds()
    {
        let hub = recorded_hub();
        let said = look(&hub.base(), "");
        assert!(said.done && !said.failed, "{said:?}");
        assert_eq!(
            said.statuses.first().map(String::as_str),
            Some(format!("Searching {} for GGUF models...", host_of(&hub.base())).as_str())
        );
        assert_eq!(
            said.statuses.last().map(String::as_str),
            Some(format!("Found 6 models on {}.", host_of(&hub.base())).as_str())
        );
        // the order of the list, the most downloaded first; each of its type
        let types: Vec<(&str, &str)> = said
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.model_type.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(
            types,
            [
                ("mudler/locate-anything.cpp-gguf", "other"),
                ("unsloth/Qwen3.8-27B-GGUF", "vision"),
                ("unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF", "code"),
                ("mixedbread-ai/mxbai-embed-large-v1", "embedding"),
                ("mudler/Laguna-XS-2.1-APEX-GGUF", "code"),
                ("ggml-org/gemma-3-4b-it-GGUF", "vision"),
            ]
        );
        // the vision model has its projector offered with it
        let gemma = said.entries.last().unwrap();
        assert_eq!(gemma.offers.len(), 6);
        assert_eq!(
            gemma
                .offers
                .iter()
                .filter(|o| o.bundle.as_deref() == Some("vision"))
                .count(),
            3
        );
        assert!(matches!(gemma.offers[0].fetch, Fetch::Files(_)));
        assert_eq!(
            gemma.description,
            "Quantized from google/gemma-3-4b-it · gemma"
        );
        assert!(gemma.downloads.is_some());

        // one search, then one request for each repository, in order, and nothing else
        let paths = hub.paths();
        assert_eq!(
            paths[0],
            "/api/models?filter=gguf&sort=downloads&direction=-1&limit=50"
        );
        assert_eq!(
            &paths[1..],
            RECORDED
                .iter()
                .map(|id| format!("/api/models/{id}?blobs=true"))
                .collect::<Vec<_>>()
        );
        // saying who asks, and what for; no cookie, no credentials
        for asked in hub.requests() {
            assert!(asked
                .header("user-agent")
                .is_some_and(|v| v.starts_with("quickai/")));
            assert_eq!(asked.header("accept"), Some("application/json"));
            assert_eq!(asked.header("cookie"), None);
            assert_eq!(asked.header("authorization"), None);
        }
    }

    #[test]
    fn p3_v11_words_are_searched_as_a_phrase_and_then_one_by_one_and_a_repository_is_looked_into_once(
    ) {
        let hub = FakeHub::start();
        let said = collect(&spawn_hub_lookup(
            HubSpec {
                site: hub.base(),
                popular: 30,
                keyword: 2,
                similar: 3,
                pipelines: Vec::new(),
                words: Vec::new(),
            },
            "  qwen   coder:7b ".into(),
            rules(),
            || {},
        ));
        assert!(said.done && !said.failed, "{said:?}");
        let searches: Vec<String> = hub
            .paths()
            .into_iter()
            .filter(|p| p.starts_with("/api/models?"))
            .collect();
        // the phrase, then each word of it (a tag is no word of a name): sorted, the phrase's own not repeated
        assert_eq!(
            searches,
            [
                "/api/models?filter=gguf&sort=downloads&direction=-1&limit=50&search=qwen%20coder",
                "/api/models?filter=gguf&sort=downloads&direction=-1&limit=50&search=coder",
                "/api/models?filter=gguf&sort=downloads&direction=-1&limit=50&search=qwen",
            ]
        );
        assert_eq!(
            said.statuses[0],
            format!("Searching {} for \"qwen coder\"...", host_of(&hub.base()))
        );
        assert!(said
            .statuses
            .contains(&"Looking for similar models...".to_string()));
        // the phrase looked into at most `keyword` repositories, each of the others at most `similar`, none twice
        let looked: Vec<String> = hub
            .paths()
            .into_iter()
            .filter(|p| p.starts_with("/api/models/"))
            .collect();
        let distinct: std::collections::BTreeSet<&String> = looked.iter().collect();
        assert_eq!(
            distinct.len(),
            looked.len(),
            "a repository was looked into twice: {looked:?}"
        );
        assert!(looked.len() <= 2 + 3 + 3, "{looked:?}");
        assert_eq!(
            said.entries.len(),
            looked.len(),
            "each has weights, so each is reported: looked into {looked:?}, reported {:?}",
            said.entries.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
        let names: Vec<&str> = said.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.iter().any(|n| n.to_lowercase().contains("coder")),
            "{names:?}"
        );
    }

    /// A hub with an original whose weights are not GGUF, a version of it named otherwise (found only by what the hub
    /// says it was made from), one named after it, and a speech model nobody made a GGUF version of.
    fn hub_with_an_original() -> FakeHub {
        let hub = FakeHub::start();
        hub.only(&[]);
        hub.add_repo(
            "someone/Talk-3B",
            Some("text-generation"),
            &[],
            &[("config.json", 900), ("model.safetensors", 6_000_000)],
        );
        hub.add_repo(
            "packer/talky-q4-GGUF",
            Some("text-generation"),
            &["gguf", "base_model:quantized:someone/Talk-3B"],
            &[("talky-Q4_K_M.gguf", 2_000_000)],
        );
        hub.add_repo(
            "other/Talk-3B-GGUF",
            Some("text-generation"),
            &["gguf"],
            &[("Talk-3B-Q4_K_M.gguf", 2_000_000)],
        );
        hub.add_repo(
            "lab/ears-ml",
            Some("automatic-speech-recognition"),
            &[],
            &[("README.md", 500), ("ears-ml.cact", 90_000_000)],
        );
        hub
    }

    #[test]
    fn a_repository_typed_is_looked_up_itself_then_the_gguf_versions_of_it_then_its_name() {
        let hub = hub_with_an_original();
        let said = look(&hub.base(), "someone/Talk-3B");
        assert!(said.done && !said.failed, "{said:?}");
        let paths = hub.paths();
        assert_eq!(paths[0], "/api/models/someone/Talk-3B?blobs=true");
        assert_eq!(
            paths[1],
            "/api/models?filter=gguf&filter=base_model%3Aquantized%3Asomeone%2FTalk-3B&sort=downloads&direction=-1&limit=50"
        );
        // then its name without the owner, and its words apart
        assert!(paths.contains(
            &"/api/models?filter=gguf&sort=downloads&direction=-1&limit=50&search=Talk-3B"
                .to_string()
        ));
        assert!(paths.contains(
            &"/api/models?filter=gguf&sort=downloads&direction=-1&limit=50&search=Talk%203B"
                .to_string()
        ));
        // the version named otherwise first, then the one named after it; the original is not a result
        let names: Vec<&str> = said.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["packer/talky-q4-GGUF", "other/Talk-3B-GGUF"]);
        // said at once, and again once the versions were looked for
        assert_eq!(said.notes.len(), 2, "{:?}", said.notes);
        assert!(said.notes[0].starts_with(
            "someone/Talk-3B has no GGUF file, which is what QuickAI downloads from Hugging Face: its largest file is model.safetensors."
        ), "{}", said.notes[0]);
        assert!(said.notes[1].ends_with("GGUF versions of it made by others are listed first."));
        assert!(said.notes[1].contains("Hugging Face lists it as text-generation."));
    }

    #[test]
    fn a_speech_model_typed_points_to_the_voice_settings_and_says_no_version_was_found() {
        let hub = hub_with_an_original();
        let said = look(&hub.base(), "lab/ears-ml");
        assert!(said.done && !said.failed, "{said:?}");
        assert!(said.entries.is_empty(), "{:?}", said.entries);
        let note = said.notes.last().unwrap();
        assert_eq!(
            note,
            "lab/ears-ml has no GGUF file, which is what QuickAI downloads from Hugging Face: its largest file is \
             ears-ml.cact. It is a speech-recognition model: the speech models QuickAI uses are in Settings › Voice. \
             No GGUF version of it was found; these are models with similar names."
        );
    }

    #[test]
    fn a_repository_typed_that_is_not_there_is_said_and_one_that_is_gguf_comes_first() {
        let hub = hub_with_an_original();
        let said = look(&hub.base(), "nobody/Talk-3B");
        assert!(said.done && !said.failed, "{said:?}");
        assert_eq!(
            said.notes,
            [format!(
                "nobody/Talk-3B is not on {}, or it is private. These are models with similar names.",
                host_of(&hub.base())
            )]
        );
        assert!(said.entries.iter().any(|e| e.name == "other/Talk-3B-GGUF"));

        let hub = hub_with_an_original();
        let said = look(&hub.base(), "other/Talk-3B-GGUF");
        assert!(said.notes.is_empty(), "{:?}", said.notes);
        assert_eq!(said.entries[0].name, "other/Talk-3B-GGUF");
        // looked into once, though its name is searched too
        let looked = hub
            .paths()
            .into_iter()
            .filter(|p| p == "/api/models/other/Talk-3B-GGUF?blobs=true")
            .count();
        assert_eq!(looked, 1);
    }

    #[test]
    fn p3_v12_a_look_up_that_cannot_be_made_says_why_and_is_never_a_list_that_looks_empty() {
        let serving = |status: u16, body: Vec<u8>| {
            MiniServer::start(None, move |_: &Request, resp| {
                resp.status(status);
                resp.send(&body);
            })
            .unwrap()
        };
        let url_of = |s: &MiniServer| format!("http://127.0.0.1:{}", s.port());
        // nothing listens
        let gone = {
            let server = MiniServer::start(None, |_: &Request, _| {}).unwrap();
            let url = url_of(&server);
            server.shutdown();
            std::thread::sleep(Duration::from_millis(100));
            url
        };
        failing(
            &look(&gone, ""),
            &[
                "Could not reach 127.0.0.1:",
                "Check the connection",
                "a link typed into the box still downloads a model",
            ],
        );
        failing(
            &look("http://no-such-host.invalid", ""),
            &["Could not reach no-such-host.invalid"],
        );
        for (status, words) in [
            (
                429,
                vec!["is limiting requests from this network", "HTTP 429"],
            ),
            (503, vec!["answered with an error (HTTP 503)"]),
            (404, vec!["HTTP 404"]),
        ] {
            failing(
                &look(&url_of(&serving(status, b"oops".to_vec())), ""),
                &words,
            );
        }
        // an answer that is not a list of repositories
        for body in [
            b"<html>maintenance</html>".to_vec(),
            b"{\"error\":\"x\"}".to_vec(),
            Vec::new(),
        ] {
            failing(
                &look(&url_of(&serving(200, body)), ""),
                &[
                    "answered, but its answer is not one this version of QuickAI can read",
                    "site may have changed",
                ],
            );
        }
        // more than an answer can be
        failing(
            &look(&url_of(&serving(200, vec![b'x'; 5 * 1024 * 1024])), ""),
            &["sent more than 4 MB", "not an answer of its API"],
        );
        // no answer within the time allowed
        let slow = MiniServer::start(None, |_: &Request, resp| {
            std::thread::sleep(Duration::from_secs(3));
            resp.send(b"[]");
        })
        .unwrap();
        let started = std::time::Instant::now();
        failing(
            &collect(&spawn_with(
                spec(&url_of(&slow)),
                String::new(),
                rules(),
                Duration::from_millis(500),
                || {},
            )),
            &["Could not reach", "timeout"],
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        // the repositories all fail: the look-up fails with what the first said, and is not "nothing found"
        let hub = recorded_hub();
        hub.behave(Behaviour::RepositoriesFail);
        failing(
            &look(&hub.base(), ""),
            &["answered with an error (HTTP 500)"],
        );
        // a site that begins to refuse part-way: what was found is kept, and the sentence says so
        let hub = recorded_hub();
        hub.behave(Behaviour::LimitAfter(3));
        let said = look(&hub.base(), "");
        assert!(said.done && !said.failed, "{said:?}");
        assert_eq!(
            said.entries.len(),
            2,
            "the search and two repositories were answered"
        );
        let last = said.statuses.last().unwrap();
        assert!(
            last.starts_with("Found 2 models on")
                && last.contains("began limiting requests")
                && last.contains("HTTP 429"),
            "{last}"
        );
        // refused at the very start of the repositories: failure, not an empty list
        let hub = recorded_hub();
        hub.behave(Behaviour::LimitAfter(1));
        failing(
            &look(&hub.base(), ""),
            &["is limiting requests from this network"],
        );
    }

    #[test]
    fn p3_v13_what_cannot_be_fetched_is_left_out_and_counted_and_nothing_found_is_not_a_failure() {
        let hub = recorded_hub();
        hub.add_repo(
            "me/private-GGUF",
            Some("text-generation"),
            &["gguf"],
            &[("p-Q4_K_M.gguf", 10)],
        );
        hub.gate("me/private-GGUF");
        hub.add_repo(
            "me/weights-only-in-shards",
            Some("text-generation"),
            &["gguf"],
            &[("m-00001-of-00002.gguf", 10), ("m-00002-of-00002.gguf", 10)],
        );
        hub.add_repo(
            "me/no-gguf-at-all",
            None,
            &["gguf"],
            &[("model.safetensors", 10)],
        );
        hub.add_repo(
            "me/projector-only",
            None,
            &["gguf"],
            &[("mmproj-f16.gguf", 10)],
        );
        let said = look(&hub.base(), "");
        assert!(said.done && !said.failed, "{said:?}");
        assert_eq!(
            said.entries.len(),
            6,
            "only the six that have weights to fetch are reported"
        );
        assert_eq!(
            said.entries.last().unwrap().name,
            "ggml-org/gemma-3-4b-it-GGUF"
        );
        let last = said.statuses.last().unwrap();
        assert!(
            last.starts_with("Found 6 models on")
                && last.ends_with("1 that needs a login is left out."),
            "{last}"
        );

        // a word nothing has: said in words, and not a failure
        let none = look(&hub.base(), "zzzzqqqq");
        assert!(
            none.done && !none.failed && none.entries.is_empty(),
            "{none:?}"
        );
        let last = none.statuses.last().unwrap();
        assert!(
            last.starts_with("No models found on 127.0.0.1:")
                && last.ends_with("for \"zzzzqqqq\"."),
            "{last}"
        );
        // a list that is empty
        let empty = FakeHub::start();
        empty.only(&[]);
        let nothing = look(&empty.base(), "");
        assert!(
            nothing.done
                && !nothing.failed
                && nothing
                    .statuses
                    .last()
                    .unwrap()
                    .ends_with("lists no GGUF models.")
        );
        // a repository that cannot be read among those that can is left out and counted
        let hub = recorded_hub();
        hub.only(&["ggml-org/gemma-3-4b-it-GGUF", "no/such-repository"]);
        hub.lose("no/such-repository");
        let said = look(&hub.base(), "");
        assert_eq!(said.entries.len(), 1);
        assert!(
            said.statuses
                .last()
                .unwrap()
                .ends_with("1 could not be read."),
            "{:?}",
            said.statuses
        );
    }

    #[test]
    fn p3_v14_what_the_look_up_reports_is_what_the_box_and_the_downloader_accept() {
        // every file a button fetches has a name that stays in the models folder and an address on the site
        let hub = recorded_hub();
        let said = look(&hub.base(), "");
        let mut files = 0;
        for entry in &said.entries {
            for offer in &entry.offers {
                match &offer.fetch {
                    Fetch::Files(download) => {
                        for piece in &download.files {
                            assert_eq!(
                                quickai_core::lookup::present::safe_file_name(&piece.store_as),
                                piece.store_as,
                                "{}",
                                piece.store_as
                            );
                            assert!(piece.url.starts_with(&hub.base()), "{}", piece.url);
                            assert!(piece.url.contains("/resolve/main/"), "{}", piece.url);
                            files += 1;
                        }
                    }
                    other => panic!("{other:?}"),
                }
            }
        }
        assert!(files > 20, "{files}");
    }

    /// Run by hand: the real Hugging Face, to see that its answers are still the ones the look-up reads.
    /// When this fails the site has changed: record its answers again with
    /// `tests/fixtures/huggingface/record.sh`, read the diff, and teach the reading what moved.
    #[test]
    #[ignore = "manual: needs the internet (cargo test -p quickai p3_v15 -- --ignored)"]
    fn p3_v15_the_real_hub_is_still_read() {
        let site = quickai_core::lookup::huggingface::SITE;
        let popular = look(site, "");
        assert!(popular.done && !popular.failed, "{popular:?}");
        assert!(popular.entries.len() >= 10, "{:?}", popular.statuses);
        assert!(popular.entries.iter().all(|e| !e.offers.is_empty()));
        assert!(
            popular
                .entries
                .iter()
                .any(|e| e.offers.iter().any(|o| o.bundle.is_some())),
            "no model of the day has a projector offered: the reading of the files is lost"
        );
        assert!(
            popular
                .entries
                .iter()
                .any(|e| e.model_type.as_deref() != Some("chat")),
            "every model is a chat model: the reading of the tags is lost"
        );
        let qwen = look(site, "qwen");
        assert!(
            qwen.done && !qwen.failed && !qwen.entries.is_empty(),
            "{qwen:?}"
        );
        assert!(
            qwen.entries
                .iter()
                .all(|e| e.name.to_lowercase().contains("qwen")),
            "{:?}",
            qwen.entries.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
    }

    // ---- the plain list of an engine that runs pictures and video (work package 4.13, group Q) ----------

    /// What ComfyUI's look-up reads what it found with: the documents the application ships.
    fn comfy_rules() -> OwnedRules {
        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        let engine = registry.engine("comfyui").expect("the ComfyUI document");
        quickai_core::models::lookup_rules(
            &registry,
            engine,
            &quickai_core::models::lookups(engine)[0],
        )
    }

    fn comfy_hub() -> FakeHub {
        let hub = FakeHub::start();
        hub.add_checked_repo(
            "me/clip-GGUF",
            Some("text-to-video"),
            &["gguf"],
            &[("clip-Q4_K_M.gguf", 300), ("clip-Q8_0.gguf", 500)],
        );
        hub.add_checked_repo(
            "me/anim-GGUF",
            Some("image-to-video"),
            &["gguf"],
            &[("anim-Q8_0.gguf", 400)],
        );
        hub.add_checked_repo(
            "me/t5-encoder-GGUF",
            None,
            &["gguf"],
            &[("t5-encoder-Q8_0.gguf", 600)],
        );
        hub.only(&["me/clip-GGUF", "me/anim-GGUF", "me/t5-encoder-GGUF"]);
        hub
    }

    fn comfy_spec(site: &str) -> HubSpec {
        HubSpec {
            site: site.to_string(),
            popular: 6,
            keyword: 20,
            similar: 20,
            pipelines: vec!["text-to-video".into(), "image-to-video".into()],
            words: vec!["encoder".into()],
        }
    }

    fn searches(hub: &FakeHub) -> Vec<String> {
        hub.paths()
            .into_iter()
            .filter(|p| p.starts_with("/api/models?"))
            .map(|p| p.split("limit=50").nth(1).unwrap().to_string())
            .collect()
    }

    #[test]
    fn p3_q12_the_plain_list_is_a_search_for_each_pipeline_and_each_word_and_words_typed_are_one_search(
    ) {
        let hub = comfy_hub();
        let said = collect(&spawn_hub_lookup(
            comfy_spec(&hub.base()),
            String::new(),
            comfy_rules(),
            || {},
        ));
        assert!(said.done && !said.failed, "{said:?}");
        assert_eq!(
            searches(&hub),
            [
                "&pipeline_tag=text-to-video",
                "&pipeline_tag=image-to-video",
                "&search=encoder"
            ]
        );
        let found: Vec<(&str, Option<&str>)> = said
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.model_type.as_deref()))
            .collect();
        assert_eq!(
            found,
            [
                ("me/clip-GGUF", Some("video")),
                ("me/anim-GGUF", Some("video")),
                ("me/t5-encoder-GGUF", Some("text_encoder"))
            ]
        );
        // it says what it is looking at as it goes
        assert!(
            said.statuses
                .iter()
                .any(|s| s.ends_with("for text-to-video models...")),
            "{:?}",
            said.statuses
        );
        assert!(
            said.statuses
                .iter()
                .any(|s| s.ends_with("for \"encoder\"...")),
            "{:?}",
            said.statuses
        );
        // each repository is looked into once: a repository two searches meet is not read twice
        let reads = hub
            .paths()
            .into_iter()
            .filter(|p| p.starts_with("/api/models/"))
            .count();
        assert_eq!(reads, 3);

        // words typed are one search that neither the pipelines nor the plain words narrow
        let hub = comfy_hub();
        let said = collect(&spawn_hub_lookup(
            comfy_spec(&hub.base()),
            "clip".into(),
            comfy_rules(),
            || {},
        ));
        assert!(said.done && !said.failed, "{said:?}");
        assert_eq!(searches(&hub), ["&search=clip"]);
        assert_eq!(said.entries.len(), 1);

        // an engine that names none of either has the one plain search it always had
        let hub = comfy_hub();
        let mut spec = comfy_spec(&hub.base());
        spec.pipelines.clear();
        spec.words.clear();
        let said = collect(&spawn_hub_lookup(spec, String::new(), comfy_rules(), || {}));
        assert!(said.done && !said.failed, "{said:?}");
        assert_eq!(searches(&hub), [""]);
    }

    #[test]
    fn p3_q12b_a_pipeline_after_the_first_that_cannot_be_searched_loses_its_models_and_not_the_look_up(
    ) {
        // the second pipeline fails: the models of the others are there and nothing says the list is short
        let hub = comfy_hub();
        hub.break_pipeline("image-to-video");
        let said = collect(&spawn_hub_lookup(
            comfy_spec(&hub.base()),
            String::new(),
            comfy_rules(),
            || {},
        ));
        assert!(said.done && !said.failed, "{said:?}");
        let names: Vec<&str> = said.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["me/clip-GGUF", "me/t5-encoder-GGUF"]);

        // the first fails: that is the look-up's failure, said once, and no list that looks empty
        let hub = comfy_hub();
        hub.break_pipeline("text-to-video");
        let said = collect(&spawn_hub_lookup(
            comfy_spec(&hub.base()),
            String::new(),
            comfy_rules(),
            || {},
        ));
        assert!(said.failed && said.entries.is_empty(), "{said:?}");
        assert!(
            said.statuses
                .last()
                .unwrap()
                .contains("answered with an error (HTTP 500)"),
            "{:?}",
            said.statuses
        );
        assert_eq!(
            searches(&hub),
            ["&pipeline_tag=text-to-video"],
            "it did not go on"
        );

        // an answer that is not what the site sends is the same: after the first it costs that pipeline's
        // models, and the first one it is said as what it is (the site may have changed), not as a short list
        let hub = comfy_hub();
        hub.garble_pipeline("image-to-video");
        let said = collect(&spawn_hub_lookup(
            comfy_spec(&hub.base()),
            String::new(),
            comfy_rules(),
            || {},
        ));
        assert!(said.done && !said.failed, "{said:?}");
        let names: Vec<&str> = said.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["me/clip-GGUF", "me/t5-encoder-GGUF"]);
        let hub = comfy_hub();
        hub.garble_pipeline("text-to-video");
        let said = collect(&spawn_hub_lookup(
            comfy_spec(&hub.base()),
            String::new(),
            comfy_rules(),
            || {},
        ));
        assert!(said.failed && said.entries.is_empty(), "{said:?}");
        assert!(
            said.statuses
                .last()
                .unwrap()
                .contains("its answer is not one this version of QuickAI can read"),
            "{:?}",
            said.statuses
        );
        assert_eq!(searches(&hub), ["&pipeline_tag=text-to-video"]);
    }

    /// Run by hand: the real Hugging Face, asked the way ComfyUI's window asks, still offers the three files
    /// someone asked for (a video model, its text encoder and its VAE, from three repositories) in the rows
    /// and with the sizes and checksums the recorded answers say (`tests/fixtures/huggingface/comfyui/`).
    /// When this fails the site or a repository has changed: record it again with `record.sh` there.
    #[test]
    #[ignore = "manual: needs the internet (cargo test -p quickai p3_q14 -- --ignored)"]
    fn p3_q14_the_real_hub_offers_the_video_model_its_text_encoder_and_its_vae_by_their_names() {
        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        let engine = registry.engine("comfyui").expect("the ComfyUI document");
        let declared = quickai_core::models::lookups(engine)[0].kind.clone();
        let quickai_core::registry::engine::LookupKind::HuggingFace {
            site,
            popular,
            keyword,
            similar,
            pipelines,
            words,
            ..
        } = declared
        else {
            panic!("the hub")
        };
        let look = |query: &str| {
            collect(&spawn_hub_lookup(
                HubSpec {
                    site: site.clone(),
                    popular,
                    keyword,
                    similar,
                    pipelines: pipelines.clone(),
                    words: words.clone(),
                },
                query.into(),
                comfy_rules(),
                || {},
            ))
        };
        let fetched =
            |entry: &Entry, label: &str| -> (String, Option<String>, Option<u64>, Option<String>) {
                let offer = entry
                    .offers
                    .iter()
                    .find(|o| o.label.starts_with(label))
                    .unwrap_or_else(|| {
                        panic!(
                            "{label} in {:?}",
                            entry.offers.iter().map(|o| &o.label).collect::<Vec<_>>()
                        )
                    });
                let Fetch::Files(download) = &offer.fetch else {
                    panic!()
                };
                let piece = &download.files[0];
                (
                    piece.store_as.clone(),
                    piece.place.clone(),
                    piece.size,
                    piece.sha256.clone(),
                )
            };

        let turbo = look("wan2.2 ti2v 5b turbo");
        assert!(turbo.done && !turbo.failed, "{turbo:?}");
        let entry = turbo
            .entries
            .iter()
            .find(|e| e.name == "hum-ma/Wan2.2-TI2V-5B-Turbo-GGUF")
            .expect("the Turbo repository");
        assert_eq!(
            fetched(entry, "Q8_0"),
            (
                "Wan2_2-TI2V-5B-Turbo-Q8_0.gguf".to_string(),
                Some("diffusion_models".to_string()),
                Some(5_404_990_176),
                Some(
                    "b4d7d1888d5ec2c29046cc443239faad99614cd3d356c61d8192fcac5f4c98fc".to_string()
                )
            )
        );

        let encoder = look("umt5");
        let entry = encoder
            .entries
            .iter()
            .find(|e| e.name == "city96/umt5-xxl-encoder-gguf")
            .expect("the text encoder repository");
        assert_eq!(entry.model_type.as_deref(), Some("text_encoder"));
        assert_eq!(
            fetched(entry, "Q8_0"),
            (
                "umt5-xxl-encoder-Q8_0.gguf".to_string(),
                Some("text_encoders".to_string()),
                Some(6_043_068_256),
                Some(
                    "2521d4de0bf9e1cc6549866463ceae85e4ec3239bc6063f7488810be39033bbc".to_string()
                )
            )
        );

        let vae = look("wan2.2 ti2v 5b");
        let entry = vae
            .entries
            .iter()
            .find(|e| e.name == "QuantStack/Wan2.2-TI2V-5B-GGUF")
            .expect("the repository that holds the VAE");
        assert_eq!(
            fetched(entry, "Wan2.2_VAE"),
            (
                "Wan2.2_VAE.safetensors".to_string(),
                Some("vae".to_string()),
                Some(1_409_400_960),
                Some(
                    "e40321bd36b9709991dae2530eb4ac303dd168276980d3e9bc4b6e2b75fed156".to_string()
                )
            )
        );

        // and the plain list is video and pictures and encoders, not language models
        let plain = look("");
        assert!(plain.done && !plain.failed, "{plain:?}");
        let kinds: std::collections::BTreeSet<&str> = plain
            .entries
            .iter()
            .filter_map(|e| e.model_type.as_deref())
            .collect();
        assert!(
            kinds.contains("video") && kinds.contains("image"),
            "{kinds:?}"
        );
        assert!(!kinds.contains("chat"), "{kinds:?}");
    }
}
