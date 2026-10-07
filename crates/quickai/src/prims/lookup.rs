//! The look-ups of models online (docs/architecture.md, section 8, `[[models.lookup]]`), and what
//! they share: the events they report, how a request is made, and the sentences for what can go
//! wrong with one.
//!
//! `lookup.ollama_library@1` is here: the engine document gives the site; this module asks it for the
//! page of a search, hands the page to `quickai_core::lookup::ollama_library` to be read, and reports
//! each model it finds, made into an entry the window draws (`quickai_core::lookup::present`). The
//! other look-up, `lookup.huggingface_gguf@1`, is `prims/hub.rs`.
//!
//! A look-up is the application's only reach for its site, and it happens when a person presses the
//! button, never before: one request for Ollama's page, with an honest `User-Agent` and no more than
//! it needs. The site sets an anonymous cookie on every page; none is kept (the client has no cookie
//! jar), so the next request is as strange to it as the first. What comes back is read as what it
//! should be and nothing else: a reply of more than a few megabytes, an error status, a page that is
//! not the search page are each a sentence for the person, and none of them is a list that looks
//! empty.

use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use quickai_core::lookup::ollama_library::{parse_search, search_url, search_words, ReadError};
use quickai_core::lookup::present::{Entry, OwnedRules};

pub(super) const CALL_TIMEOUT: Duration = Duration::from_secs(15);
/// A page of the search is about a hundred kilobytes; a reply of more is not that page.
const MAX_PAGE_BYTES: u64 = 2 * 1024 * 1024;
pub(super) const USER_AGENT: &str = concat!("quickai/", env!("CARGO_PKG_VERSION"));

/// What a look-up reports, in order: a status line (more than one while it works), a `Found` for
/// each model, and `Done`.
pub enum LookupEvent {
    Status(String),
    /// A model, made into what the window shows. A model with nothing to fetch is not reported.
    Found(Entry),
    /// The look-up could not be made: the status line before it says why, and an interface must
    /// keep that line and not say that nothing was found. `Done` follows.
    Failed,
    Done,
}

/// Starts a look-up of `query` at `site` in a background thread and returns at once with the
/// receiving end of the channel. `wake` is called after each event, to have the window drawn.
pub fn spawn_library_lookup(
    site: String,
    query: String,
    rules: OwnedRules,
    wake: impl Fn() + Send + Sync + 'static,
) -> Receiver<LookupEvent> {
    spawn_with(site, query, rules, CALL_TIMEOUT, wake)
}

fn spawn_with(
    site: String,
    query: String,
    rules: OwnedRules,
    timeout: Duration,
    wake: impl Fn() + Send + Sync + 'static,
) -> Receiver<LookupEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::spawn(move || run(&site, &query, &rules, timeout, &tx, &wake));
    rx
}

/// `ollama.com`, or `127.0.0.1:8080`: what a sentence calls the site.
pub fn host_of(site: &str) -> String {
    let rest = site.split_once("://").map_or(site, |(_, rest)| rest);
    rest.split('/').next().unwrap_or(rest).to_string()
}

fn run(
    site: &str,
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
    let fail = |sentence: String| {
        send(LookupEvent::Status(sentence));
        send(LookupEvent::Failed);
        send(LookupEvent::Done);
    };
    let host = host_of(site);
    let words = search_words(query);
    send(LookupEvent::Status(if words.is_empty() {
        format!("Looking in {host}'s library...")
    } else {
        format!("Looking in {host}'s library for \"{words}\"...")
    }));

    let request = Request {
        accept: "text/html",
        max_bytes: MAX_PAGE_BYTES,
        timeout,
    };
    let page = match fetch(&search_url(site, query), &request) {
        Ok(page) => page,
        Err(e) => {
            return fail(e.sentence(
                &host,
                &request,
                "a page of its search",
                "a model's name typed into the box still pulls it.",
            ))
        }
    };
    let page = match parse_search(&page) {
        Ok(page) => page,
        Err(ReadError::Unreadable) => {
            return fail(format!(
                "{host} answered, but its page is not one this version of QuickAI can read (the \
                 site may have changed). A model's name typed into the box still pulls it."
            ));
        }
    };
    let entries: Vec<Entry> = page
        .models
        .iter()
        .map(|model| rules.present(&model.candidate()))
        .filter(|entry| !entry.offers.is_empty())
        .collect();
    if entries.is_empty() {
        send(LookupEvent::Status(if words.is_empty() {
            format!("{host}'s library lists no models.")
        } else {
            format!("No models found in {host}'s library for \"{words}\".")
        }));
        send(LookupEvent::Done);
        return;
    }
    let count = entries.len();
    for entry in entries {
        send(LookupEvent::Found(entry));
    }
    send(LookupEvent::Status(format!(
        "Found {count} model{} in {host}'s library.{}",
        if count == 1 { "" } else { "s" },
        if page.more {
            " There are more: type some words to narrow the search."
        } else {
            ""
        }
    )));
    send(LookupEvent::Done);
}

/// What a request asks for, and how much of the answer is believed.
pub(super) struct Request<'a> {
    pub accept: &'a str,
    pub max_bytes: u64,
    pub timeout: Duration,
}

/// Why the answer did not come.
#[derive(Debug)]
pub(super) enum FetchError {
    /// No answer: the name did not resolve, nothing listened, it took too long, the connection broke.
    Unreachable(String),
    /// The site is turning this network away (HTTP 429).
    Limited,
    /// The site answered with an error.
    Status(u16),
    /// More than the answer can be.
    TooBig,
}

impl FetchError {
    /// What the person is told. `thing` is what the answer should have been (`a page of its
    /// search`) and `still` what can be done without the look-up.
    pub(super) fn sentence(
        &self,
        host: &str,
        request: &Request<'_>,
        thing: &str,
        still: &str,
    ) -> String {
        match self {
            FetchError::Unreachable(why) => {
                format!("Could not reach {host}: {why}. Check the connection; {still}")
            }
            FetchError::Limited => format!(
                "{host} is limiting requests from this network (HTTP 429); try again in a while."
            ),
            FetchError::Status(code) => {
                format!("{host} answered with an error (HTTP {code}); try again later.")
            }
            FetchError::TooBig => format!(
                "{host} sent more than {} MB: that is not {thing}.",
                request.max_bytes / (1024 * 1024)
            ),
        }
    }
}

pub(super) fn fetch(url: &str, request: &Request<'_>) -> Result<String, FetchError> {
    // a status is an answer here: a 429 and a 503 are told apart
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(request.timeout))
        .build()
        .into();
    let response = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", request.accept)
        .call()
        .map_err(|e| FetchError::Unreachable(e.to_string()))?;
    let status = response.status().as_u16();
    if status == 429 {
        return Err(FetchError::Limited);
    }
    if !(200..300).contains(&status) {
        return Err(FetchError::Status(status));
    }
    // bytes, and the text made of them here: a byte that is not UTF-8 costs the character it is in and
    // not the page (the client's own lossy mode depends on what the reply says its charset is)
    let mut body = response.into_body();
    let bytes = body
        .with_config()
        .limit(request.max_bytes)
        .read_to_vec()
        .map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => FetchError::TooBig,
            other => FetchError::Unreachable(other.to_string()),
        })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::testkit::fake_library::{recorded_page as recorded, FakeLibrary};
    use crate::testkit::mini_http::{MiniServer, Request};
    use quickai_core::lookup::present::Fetch;

    /// What Ollama's look-up reads what it found with: the document the application ships.
    fn rules() -> OwnedRules {
        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        let engine = registry.engine("ollama").expect("the Ollama document");
        quickai_core::models::lookup_rules(
            &registry,
            engine,
            &quickai_core::models::lookups(engine)[0],
        )
    }

    /// The names a model is pulled by: one for each button.
    fn references(entry: &Entry) -> Vec<String> {
        entry
            .offers
            .iter()
            .map(|o| match &o.fetch {
                Fetch::Reference(r) => r.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    /// What a look-up said, in the order it said it.
    #[derive(Debug, Default)]
    struct Said {
        statuses: Vec<String>,
        models: Vec<Entry>,
        failed: bool,
        done: bool,
    }

    fn collect(rx: &Receiver<LookupEvent>) -> Said {
        let mut said = Said::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(LookupEvent::Status(s)) => said.statuses.push(s),
                Ok(LookupEvent::Found(m)) => said.models.push(m),
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

    fn url_of(server: &MiniServer) -> String {
        format!("http://127.0.0.1:{}", server.port())
    }

    #[test]
    fn p3_u12_a_look_up_asks_once_for_the_address_the_words_make_and_reports_what_the_page_holds() {
        let library = FakeLibrary::start();
        let said = collect(&spawn_library_lookup(
            library.base(),
            String::new(),
            rules(),
            || {},
        ));
        assert!(said.done && !said.failed, "{said:?}");
        assert_eq!(said.models.len(), 20);
        assert_eq!(said.models[0].name, "clef-flash");
        assert_eq!(said.models[6].name, "qwen3.8");
        assert_eq!(references(&said.models[6]), ["qwen3.8:27b"]);
        assert_eq!(
            said.statuses.first().map(String::as_str),
            Some(format!("Looking in {}'s library...", host_of(&library.base())).as_str())
        );
        let last = said.statuses.last().unwrap();
        assert!(
            last.starts_with("Found 20 models in 127.0.0.1:") && last.contains("type some words"),
            "{last}"
        );

        // one request, for the page of local models, saying who is asking and for what
        let asked = library.requests();
        assert_eq!(library.paths(), ["/search?c=local"]);
        assert!(asked[0]
            .header("user-agent")
            .is_some_and(|v| v.starts_with("quickai/")));
        assert_eq!(asked[0].header("accept"), Some("text/html"));
        assert_eq!(asked[0].header("cookie"), None);
        assert_eq!(asked[0].header("authorization"), None);

        // words, and a tag after them that is not a word
        let said = collect(&spawn_library_lookup(
            library.base(),
            "gpt:20b".into(),
            rules(),
            || {},
        ));
        assert!(said.done && !said.failed, "{said:?}");
        assert_eq!(
            said.models.len(),
            19,
            "the upload among the twenty is not offered"
        );
        assert!(
            said.statuses[0].ends_with("library for \"gpt\"..."),
            "{:?}",
            said.statuses
        );
        assert_eq!(
            library.paths(),
            ["/search?c=local", "/search?q=gpt&c=local"]
        );
    }

    #[test]
    fn p3_u13_a_redirect_is_followed_and_the_cookie_the_site_sets_is_not_kept() {
        let seen: Arc<Mutex<Vec<(String, bool)>>> = Arc::default();
        let log = seen.clone();
        let page = recorded("popular_local.html");
        let server = MiniServer::start(None, move |req: &Request, resp| {
            log.lock()
                .unwrap()
                .push((req.path.clone(), req.header("cookie").is_some()));
            resp.header("Set-Cookie", "aid=abc; Path=/; HttpOnly");
            if req.path == "/search?c=local" {
                resp.status(303).header("Location", "/moved/popular");
                resp.send(b"");
            } else {
                resp.header("Content-Type", "text/html");
                resp.send(&page);
            }
        })
        .unwrap();
        for _ in 0..2 {
            let said = collect(&spawn_library_lookup(
                url_of(&server),
                String::new(),
                rules(),
                || {},
            ));
            assert!(
                said.done && !said.failed && said.models.len() == 20,
                "{said:?}"
            );
        }
        let seen = seen.lock().unwrap();
        let paths: Vec<&str> = seen.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/search?c=local",
                "/moved/popular",
                "/search?c=local",
                "/moved/popular"
            ]
        );
        assert!(
            seen.iter().all(|(_, cookie)| !cookie),
            "a cookie was sent back: {seen:?}"
        );
    }

    #[test]
    fn p3_u14_a_look_up_that_cannot_be_made_says_why_and_is_never_a_list_that_looks_empty() {
        let failing = |said: &Said, words: &[&str]| {
            assert!(
                said.done && said.failed && said.models.is_empty(),
                "{said:?}"
            );
            let sentence = said.statuses.last().unwrap();
            for word in words {
                assert!(sentence.contains(word), "`{word}` is not in: {sentence}");
            }
        };
        let serving = |status: u16, headers: &'static [(&str, &str)], body: Vec<u8>| {
            MiniServer::start(None, move |_: &Request, resp| {
                resp.status(status);
                for (k, v) in headers {
                    resp.header(k, v);
                }
                resp.send(&body);
            })
            .unwrap()
        };

        // nothing listens: the port of a server that is gone
        let gone = {
            let server = MiniServer::start(None, |_: &Request, _| {}).unwrap();
            let url = url_of(&server);
            server.shutdown();
            std::thread::sleep(Duration::from_millis(100));
            url
        };
        failing(
            &collect(&spawn_library_lookup(
                gone.clone(),
                String::new(),
                rules(),
                || {},
            )),
            &[
                "Could not reach 127.0.0.1:",
                "Check the connection",
                "still pulls it",
            ],
        );
        // a name that does not resolve
        failing(
            &collect(&spawn_library_lookup(
                "http://no-such-host.invalid".into(),
                String::new(),
                rules(),
                || {},
            )),
            &["Could not reach no-such-host.invalid"],
        );
        // errors from the site, told apart
        for (status, words) in [
            (
                429,
                vec!["is limiting requests from this network", "HTTP 429"],
            ),
            (503, vec!["answered with an error (HTTP 503)"]),
            (500, vec!["HTTP 500"]),
            (404, vec!["HTTP 404"]),
            (403, vec!["HTTP 403"]),
        ] {
            let server = serving(status, &[], b"oops".to_vec());
            failing(
                &collect(&spawn_library_lookup(
                    url_of(&server),
                    String::new(),
                    rules(),
                    || {},
                )),
                &words,
            );
        }
        // something other than the search page: the sentence says the page is not understood
        for body in [
            b"<html><body>Down for maintenance</body></html>".to_vec(),
            b"{\"models\":[]}".to_vec(),
            Vec::new(),
        ] {
            let server = serving(200, &[("Content-Type", "text/html")], body);
            failing(
                &collect(&spawn_library_lookup(
                    url_of(&server),
                    String::new(),
                    rules(),
                    || {},
                )),
                &[
                    "answered, but its page is not one this version of QuickAI can read",
                    "site may have changed",
                ],
            );
        }
        // more than a page can be
        let server = serving(200, &[], vec![b'x'; 3 * 1024 * 1024]);
        failing(
            &collect(&spawn_library_lookup(
                url_of(&server),
                String::new(),
                rules(),
                || {},
            )),
            &["sent more than 2 MB", "not a page of its search"],
        );
        // no answer within the time allowed
        let slow = MiniServer::start(None, |_: &Request, resp| {
            std::thread::sleep(Duration::from_secs(3));
            resp.send(b"late");
        })
        .unwrap();
        let started = std::time::Instant::now();
        failing(
            &collect(&spawn_with(
                url_of(&slow),
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
    }

    #[test]
    fn p3_u15_nothing_found_is_said_in_words_and_is_not_a_failure() {
        let library = FakeLibrary::start();
        let said = collect(&spawn_library_lookup(
            library.base(),
            "zzzzqqqq".into(),
            rules(),
            || {},
        ));
        assert!(
            said.done && !said.failed && said.models.is_empty(),
            "{said:?}"
        );
        let last = said.statuses.last().unwrap();
        assert!(
            last.starts_with("No models found in 127.0.0.1:")
                && last.ends_with("for \"zzzzqqqq\"."),
            "{last}"
        );
    }

    #[test]
    fn p3_u16_a_page_that_is_not_valid_text_is_read_for_what_it_holds() {
        // the library's page with a byte that is not UTF-8 in the middle of a description
        let mut page = recorded("popular_local.html");
        let at = page
            .windows(14)
            .position(|w| w == b"Clef-Flash is ")
            .expect("the first card");
        page[at + 5] = 0xFF;
        let server = MiniServer::start(None, move |_: &Request, resp| resp.send(&page)).unwrap();
        let said = collect(&spawn_library_lookup(
            url_of(&server),
            String::new(),
            rules(),
            || {},
        ));
        assert!(
            said.done && !said.failed && said.models.len() == 20,
            "{said:?}"
        );
        assert!(
            said.models[0].description.contains('\u{FFFD}'),
            "{}",
            said.models[0].description
        );
        assert_eq!(said.models[1].name, "clef");
    }

    #[test]
    fn p3_u19_each_engine_document_declares_its_look_up() {
        use quickai_core::models::lookups;
        use quickai_core::registry::engine::LookupKind;

        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        assert!(
            registry.diagnostics.is_empty(),
            "{:#?}",
            registry.diagnostics
        );
        let ollama = registry.engine("ollama").expect("the Ollama document");
        let declared = lookups(ollama);
        assert_eq!(declared.len(), 1);
        assert_eq!(declared[0].id, "library");
        match &declared[0].kind {
            LookupKind::OllamaLibrary { site, read } => {
                assert_eq!(site, "https://ollama.com");
                assert_eq!(read.bytes_per_parameter, 0.6);
            }
            other => panic!("{other:?}"),
        }
        assert!(
            ollama
                .merged
                .envelope
                .requires_primitives
                .iter()
                .any(|p| p.to_string() == "lookup.ollama_library@1"),
            "the document does not say it needs the look-up"
        );
        assert!(crate::prims::PROVIDED.contains(&"lookup.ollama_library@1"));
        // llama.cpp's look-up is Hugging Face's, declared as Ollama's is, and its window has the buttons
        // it always had
        let llamacpp = registry.engine("llamacpp").expect("the llama.cpp document");
        let declared = lookups(llamacpp);
        assert_eq!(declared.len(), 1);
        assert_eq!(declared[0].id, "hub");
        match &declared[0].kind {
            LookupKind::HuggingFace {
                site,
                popular,
                keyword,
                similar,
                ..
            } => {
                assert_eq!(site, "https://huggingface.co");
                assert_eq!((*popular, *keyword, *similar), (30, 20, 20));
            }
            other => panic!("{other:?}"),
        }
        assert!(llamacpp
            .merged
            .envelope
            .requires_primitives
            .iter()
            .any(|p| p.to_string() == "lookup.huggingface_gguf@1"));
        assert!(crate::prims::PROVIDED.contains(&"lookup.huggingface_gguf@1"));
        assert!(llamacpp.models.as_ref().is_some_and(|m| m.list.is_none()));
    }

    #[test]
    fn p3_u20_nothing_the_look_up_offers_is_something_the_model_box_would_refuse() {
        use quickai_core::lookup::ollama_library::parse_search;
        use quickai_core::registry::InputAction;

        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        let ollama = registry.engine("ollama").expect("the Ollama document");
        let mut checked = 0;
        for file in ["popular_local.html", "gpt_local.html", "embed_local.html"] {
            let page = parse_search(&String::from_utf8(recorded(file)).unwrap()).unwrap();
            for model in &page.models {
                let references = std::iter::once(model.reference(None))
                    .chain(model.sizes.iter().map(|s| model.reference(Some(s))));
                for reference in references {
                    match ollama.classify_input(&reference, &|_| false) {
                        InputAction::Pull { reference: pulled } => {
                            assert_eq!(pulled, reference, "the box changes what the row means");
                        }
                        other => panic!("`{reference}` is not taken by the box: {other:?}"),
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 100, "{checked}");
    }

    #[test]
    fn p3_u17_the_site_is_named_in_a_sentence_by_its_host() {
        assert_eq!(host_of("https://ollama.com"), "ollama.com");
        assert_eq!(host_of("http://127.0.0.1:8080"), "127.0.0.1:8080");
        assert_eq!(host_of("https://mirror.example/ollama"), "mirror.example");
        assert_eq!(host_of("ollama.com"), "ollama.com");
    }

    /// Run by hand: the real site, to see that the page is still one the look-up reads. When this
    /// fails the site has changed: record its pages again with `tests/fixtures/ollama/library/
    /// record.sh`, read the diff, and teach the reading what moved.
    #[test]
    #[ignore = "manual: needs the internet (cargo test -p quickai p3_u18 -- --ignored)"]
    fn p3_u18_the_real_site_is_still_read() {
        let site = quickai_core::lookup::ollama_library::SITE.to_string();
        let popular = collect(&spawn_library_lookup(
            site.clone(),
            String::new(),
            rules(),
            || {},
        ));
        assert!(popular.done && !popular.failed, "{popular:?}");
        assert!(popular.models.len() >= 10, "{:?}", popular.statuses);
        for m in &popular.models {
            assert!(
                quickai_core::lookup::ollama_library::valid_name(&m.name),
                "{}",
                m.name
            );
            assert!(!m.description.is_empty(), "{} has no description", m.name);
        }
        assert!(
            popular
                .models
                .iter()
                .any(|m| m.offers.len() > 1 || m.offers[0].label != "latest"),
            "no model shows its sizes: the reading of them is lost"
        );
        assert!(
            popular.models.iter().any(|m| m.downloads.is_some()),
            "no model shows its count: the reading of it is lost"
        );
        let qwen = collect(&spawn_library_lookup(site, "qwen".into(), rules(), || {}));
        assert!(qwen.done && !qwen.failed, "{qwen:?}");
        assert!(
            qwen.models.iter().any(|m| m.name.starts_with("qwen")),
            "{:?}",
            qwen.models
        );
    }
}
