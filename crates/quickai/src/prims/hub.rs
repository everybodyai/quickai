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
    candidate, parse_repo, parse_search, repo_url, search_url, search_words, similar_terms, Hit,
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
    send(LookupEvent::Status(if words.is_empty() {
        format!("Searching {host} for GGUF models...")
    } else {
        format!("Searching {host} for \"{words}\"...")
    }));

    let first = if words.is_empty() {
        None
    } else {
        Some(words.as_str())
    };
    let hits = match fetch(&search_url(&spec.site, first), &request) {
        Ok(body) => match parse_search(&body) {
            Ok(hits) => hits,
            Err(_) => {
                return fail(format!(
                    "{host} answered, but its answer is not one this version of QuickAI can read (the \
                     site may have changed). {}",
                    capitalize(STILL)
                ));
            }
        },
        Err(e) => return fail(sentence(&e)),
    };

    let mut seen: HashSet<String> = HashSet::new();
    let mut tally = Tally::default();
    let inspected = if words.is_empty() {
        spec.popular
    } else {
        spec.keyword
    };
    inspect(
        spec, &request, hits, inspected, &mut seen, &mut tally, rules, &send,
    );

    if !words.is_empty() && !tally.limited {
        let terms: Vec<String> = similar_terms(&words)
            .into_iter()
            .filter(|t| !t.eq_ignore_ascii_case(&words))
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

fn capitalize(text: &str) -> String {
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
        }
    }

    /// What a look-up said, in the order it said it.
    #[derive(Debug, Default)]
    struct Said {
        statuses: Vec<String>,
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
}
