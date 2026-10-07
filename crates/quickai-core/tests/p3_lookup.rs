//! Group U of docs/phase3-test-plan.md: looking models up online, the pure half (work package 3.15).
//! The pages are recordings of the real ollama.com search (`record.sh` beside them), trimmed to the
//! results; the tests that follow say what those pages hold and what must be made of them.

use std::path::Path;

use quickai_core::lookup::ollama_library::{
    parse_search, search_url, valid_name, valid_size, LibraryModel, ReadError, SearchPage,
};

fn page(file: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../quickai/tests/fixtures/ollama/library")
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn read(file: &str) -> SearchPage {
    parse_search(&page(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

fn model<'a>(page: &'a SearchPage, name: &str) -> &'a LibraryModel {
    page.models
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("`{name}` is not on the page: {:?}", names(page)))
}

fn names(page: &SearchPage) -> Vec<&str> {
    page.models.iter().map(|m| m.name.as_str()).collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn p3_u01_the_recorded_popular_page_is_read_card_by_card() {
    let popular = read("popular_local.html");
    // the page says twenty, and more behind them: that is what it was recorded with
    assert_eq!((popular.reported, popular.more), (Some(20), true));
    assert_eq!(
        names(&popular),
        [
            "clef-flash",
            "clef",
            "embeddinggemma-2",
            "laya",
            "tev1",
            "nimble",
            "qwen3.8",
            "qwen3.6",
            "ornith-1.5",
            "ornith",
            "qwen3.8-flash-next",
            "muse-glimmer",
            "nemotron-3.5-lightning",
            "nemotron3",
            "granite4.1",
            "granite4.2",
            "mistral-medium-3.5",
            "medgemma",
            "laguna-s-2.1",
            "lfm2.5",
        ],
        "every card of the page, in the page's order"
    );

    let first = &popular.models[0];
    assert_eq!(first.name, "clef-flash");
    assert!(
        first
            .description
            .starts_with("Clef-Flash is a 9B multimodal model"),
        "{}",
        first.description
    );
    assert_eq!(first.sizes, strings(&["9b"]));
    assert_eq!(first.capabilities, strings(&["vision", "decision"]));
    assert_eq!(first.downloads, Some(10_455));

    // sizes in the page's order, and the words an icon carries
    let gemma = model(&popular, "embeddinggemma-2");
    assert_eq!(gemma.sizes, strings(&["270m", "440m", "570m", "740m"]));
    assert_eq!(
        gemma.capabilities,
        strings(&["vision", "audio", "embedding"])
    );
    assert_eq!(model(&popular, "tev1").sizes, strings(&["0.8b", "4b"]));
    assert_eq!(
        model(&popular, "ornith-1.5").sizes,
        strings(&["9b", "35b", "397b"])
    );
    assert_eq!(model(&popular, "qwen3.8").downloads, Some(3_301_841));
    assert_eq!(
        model(&popular, "qwen3.6").capabilities,
        strings(&["tools", "thinking", "vision"])
    );
    // a card with sizes and none of the words
    assert!(model(&popular, "granite4.2").capabilities.is_empty());

    for m in &popular.models {
        assert!(valid_name(&m.name), "{}", m.name);
        assert!(!m.description.is_empty(), "{} has no description", m.name);
        assert!(m.downloads.is_some(), "{} has no count", m.name);
        assert!(m.sizes.iter().all(|s| valid_size(s)), "{:?}", m.sizes);
    }
}

#[test]
fn p3_u02_what_a_card_may_lack_or_carry_is_read_as_it_is() {
    let gpt = read("gpt_local.html");
    let embed = read("embed_local.html");

    // a card that offers a cloud variant too: the local sizes, and the cloud word is not one of the
    // things the model can do
    let oss = model(&gpt, "gpt-oss");
    assert_eq!(oss.sizes, strings(&["20b", "120b"]));
    assert_eq!(oss.capabilities, strings(&["tools", "thinking"]));
    let gemma4 = model(&gpt, "gemma4");
    assert_eq!(gemma4.sizes, strings(&["e2b", "e4b", "12b", "26b", "31b"]));
    assert_eq!(
        gemma4.capabilities,
        strings(&["tools", "thinking", "vision", "audio"])
    );
    assert!(gpt
        .models
        .iter()
        .all(|m| !m.capabilities.contains(&"cloud".to_string())));

    // a card with no size to pull it in: the default is all there is, so no sizes are made up
    for (page, name) in [
        (&gpt, "glm-4.7-flash"),
        (&embed, "nomic-embed-text"),
        (&embed, "nomic-embed-text-v2-moe"),
    ] {
        assert!(model(page, name).sizes.is_empty(), "{name}");
    }
    let nomic = model(&embed, "nomic-embed-text");
    assert_eq!(nomic.capabilities, strings(&["embedding"]));
    assert_eq!(nomic.downloads, Some(88_321_876));
    assert_eq!(model(&gpt, "gemma3n").sizes, strings(&["e2b", "e4b"]));
    assert_eq!(
        model(&embed, "exaone3.5").sizes,
        strings(&["2.4b", "7.8b", "32b"])
    );

    // people's uploads (`user/name`) are on the page and are not offered: twenty cards, eight of
    // them uploads
    assert_eq!(embed.models.len(), 12, "{:?}", names(&embed));
    assert!(embed.models.iter().all(|m| !m.name.contains('/')));
    assert_eq!(gpt.models.len(), 19, "one upload among the twenty");
}

#[test]
fn p3_u03_a_page_with_no_results_is_an_answer_and_a_page_that_cannot_be_read_is_not() {
    let none = read("none_local.html");
    assert!(none.models.is_empty());
    assert_eq!((none.reported, none.more), (Some(0), false));

    // the site says there are results and none can be found: it looks different, which is not
    // the same as nothing matching
    let cards_gone =
        page("popular_local.html").replace("href=\"/library/", "data-gone=\"/library/");
    assert_eq!(parse_search(&cards_gone), Err(ReadError::Unreadable));
    // not a search page at all
    for text in [
        "",
        "<html><body>Maintenance</body></html>",
        "{\"models\":[]}",
        "<p data-result-count=\"3\">3 models</p><ul></ul>",
    ] {
        assert_eq!(parse_search(text), Err(ReadError::Unreadable), "{text:?}");
    }
    // a page without the site's count that says so in words is an answer too
    let said = parse_search("<ul><li>No models found.</li></ul>").unwrap();
    assert!(said.models.is_empty());
    assert_eq!(said.reported, None);
    // and one without the count that has cards is read
    let uncounted = parse_search("<ul><li><a href=\"/library/foo\">foo</a></li></ul>").unwrap();
    assert_eq!(names(&uncounted), ["foo"]);
    assert_eq!((uncounted.reported, uncounted.more), (None, false));
}

#[test]
fn p3_u04_text_from_the_page_is_decoded_cleaned_and_bounded() {
    let card = |inner: &str| format!("<a href=\"/library/m\" class=\"x\">{inner}</a>");
    let described = |inner: &str| {
        let parsed = parse_search(&card(inner)).unwrap();
        parsed.models[0].description.clone()
    };

    // entities, in the title and in the text, and white space of the page's indentation
    assert_eq!(
        described("<p title=\"Q&amp;A, it&#39;s &quot;fast&quot; &#8217; &#x2019; &lt;b&gt; &nbsp;ok\">x</p>"),
        "Q&A, it's \"fast\" ’ ’ <b> ok"
    );
    assert_eq!(
        described("<p>\n      A   model\n   for   you\n   </p>"),
        "A model for you",
        "no title: the text of the paragraph, the white space collapsed"
    );
    // an entity that is not one stays as typed, and a character that is not text is not made
    assert_eq!(
        described("<p title=\"a &bogus; b &#0; c &\">x</p>"),
        "a &bogus; b &#0; c &"
    );
    // long is cut, at a character and not a byte, and says so
    let long = "é".repeat(1000);
    let cut = described(&format!("<p title=\"{long}\">x</p>"));
    assert_eq!(cut.chars().count(), 400, "{cut}");
    assert!(cut.ends_with('…'));
    // markup inside a description is not text of it
    assert_eq!(described("<p>a <b>bold</b> <i>word</i></p>"), "a bold word");
    assert_eq!(described("<p></p>"), "", "nothing is made up");

    // how often it was pulled, as the site writes a number
    let pulls = |title: &str| {
        let parsed = parse_search(&card(&format!("<span title=\"{title}\">3.1M</span>"))).unwrap();
        parsed.models[0].downloads
    };
    assert_eq!(pulls("3,141,362 downloads"), Some(3_141_362));
    assert_eq!(pulls("12 downloads"), Some(12));
    assert_eq!(pulls("1.234 pulls"), Some(1234));
    assert_eq!(pulls("Downloads"), None);
    assert_eq!(pulls("many downloads"), None);
    assert_eq!(
        pulls("99999999999999999999999 downloads"),
        None,
        "too large for a count"
    );
}

#[test]
fn p3_u05_the_address_asks_for_the_words_and_for_local_models_only() {
    let site = "https://ollama.com";
    // nothing typed: the site's own list of what is popular
    assert_eq!(search_url(site, ""), "https://ollama.com/search?c=local");
    assert_eq!(
        search_url(site, "   \t "),
        "https://ollama.com/search?c=local"
    );
    // words, one space between them, encoded
    assert_eq!(
        search_url(site, "qwen"),
        "https://ollama.com/search?q=qwen&c=local"
    );
    assert_eq!(
        search_url(site, "  qwen   coder "),
        "https://ollama.com/search?q=qwen%20coder&c=local"
    );
    // a tag after the name is not a word to look for
    assert_eq!(
        search_url(site, "qwen3:4b"),
        "https://ollama.com/search?q=qwen3&c=local"
    );
    assert_eq!(search_url(site, ":4b"), "https://ollama.com/search?c=local");
    // what would end the value or the address is encoded, text that is not ASCII is its UTF-8
    assert_eq!(
        search_url(site, "a&b=c#d?e/f+g"),
        "https://ollama.com/search?q=a%26b%3Dc%23d%3Fe%2Ff%2Bg&c=local"
    );
    assert_eq!(
        search_url(site, "é"),
        "https://ollama.com/search?q=%C3%A9&c=local"
    );
    // a very long line is not a search a person typed: it is cut
    let long = "x".repeat(500);
    let url = search_url(site, &long);
    assert_eq!(url.matches('x').count(), 100, "{url}");
    // the root a document gives may end in a slash, or have spaces round it
    assert_eq!(
        search_url(" http://127.0.0.1:8080/ ", "a"),
        "http://127.0.0.1:8080/search?q=a&c=local"
    );
}

#[test]
fn p3_u06_a_page_of_any_shape_is_read_without_a_panic_and_nothing_unfit_is_offered() {
    // the real page cut short anywhere (the connection dropped, the size cap), at every kind of place
    let whole = page("gpt_local.html");
    let mut cuts = 0;
    for (at, _) in whole.char_indices().step_by(311) {
        let _ = parse_search(&whole[..at]);
        cuts += 1;
    }
    assert!(cuts > 100);
    // the page's tags thrown about
    for broken in [
        whole.replace('>', ""),
        whole.replace('<', "<<"),
        whole.replace('"', ""),
        whole.replace("</a>", ""),
        whole.replace("<a ", "<a"),
        whole.replace("</", "<"),
        whole.replace("href=", "href"),
    ] {
        let _ = parse_search(&broken);
    }
    // bytes that are not text, and shapes that are only brackets
    let mut noise = String::new();
    let mut state = 12345u32;
    for _ in 0..20_000 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        noise.push(char::from_u32(32 + (state >> 24) % 200).unwrap_or('?'));
    }
    let _ = parse_search(&noise);
    for text in [
        "<a",
        "<a ",
        "<a href",
        "<a href=",
        "<a href=\"",
        "<a href=\"/library/x",
        "<a href=\"/library/x\"",
        "<a href=\"/library/x\">",
        "<a href=\"/library/x\"><p title=\"",
        "</",
        "<",
        "&",
        "&#",
        "<!--",
        "<a href='/library/x'><svg></svg>",
        "<a href=\"/library/x\"><!-- <p title=\"y\"> -->",
    ] {
        let _ = parse_search(text);
    }

    // names the library would never give are not offered, whatever the page says
    let long_name = "a".repeat(81);
    let hostile = [
        "../etc/passwd",
        "..",
        "Qwen3",
        "a b",
        "",
        "%2e%2e",
        "x&quot;y",
        "-lead",
        "a;b",
        "a\u{0}b",
        "a&#x2e;&#x2e;",
        "a..b",
        long_name.as_str(),
    ];
    for name in hostile {
        let text = format!("<a href=\"/library/{name}\">{name}</a>");
        let parsed = parse_search(&text);
        assert!(
            parsed.map_or(true, |p| p.models.is_empty()),
            "`{name}` was offered"
        );
    }
    // a link to a model's own pages is the model; a link to something that only begins alike is not
    let linked = parse_search(
        "<a href=\"/library/foo/tags\">a</a><a href=\"/library/bar?x=1\">b</a>\
         <a href=\"/library\">c</a><a href=\"/libraryx/baz\">d</a><abbr href=\"/library/q\">e</abbr>\
         <a href=\"/library/enc&#47;x\">f</a>",
    )
    .unwrap();
    assert_eq!(names(&linked), ["foo", "bar", "enc"]);

    // sizes that are not sizes are not offered as ways to pull
    for (shown, ok) in [
        ("22m", true),
        ("0.6b", true),
        ("70b", true),
        ("8x7b", true),
        ("e2b", true),
        ("1t", true),
        ("latest", false),
        ("1.5", false),
        ("7B", false),
        ("b", false),
        ("", false),
        (".5b", false),
        ("1.b", false),
        ("8x", false),
        ("8xb", false),
        ("1b2", false),
        ("e", false),
        ("ee2b", false),
        ("12345678901b", true),
        ("123456789012b", false),
        ("1b ", false),
        ("q4_k_m", false),
        ("2;rm", false),
    ] {
        assert_eq!(valid_size(shown), ok, "{shown:?}");
    }
    let tagged = parse_search(
        "<a href=\"/library/foo\"><p title=\"d\">d</p><span role=\"tooltip\">Runs on your computer</span>\
         <svg></svg><span>8b</span><span>·</span><span>latest</span><span>70b</span></a>",
    )
    .unwrap();
    assert_eq!(
        tagged.models[0].sizes,
        strings(&["8b"]),
        "the sizes end at the first thing that is not one"
    );

    // a page of ten thousand cards is bounded
    let many: String = (0..10_000)
        .map(|i| format!("<a href=\"/library/m{i}\">m{i}</a>"))
        .collect();
    assert_eq!(parse_search(&many).unwrap().models.len(), 50);
    // the same name twice is one row
    let twice = parse_search("<a href=\"/library/a\">1</a><a href=\"/library/a\">2</a>").unwrap();
    assert_eq!(names(&twice), ["a"]);
}

#[test]
fn p3_u07_a_page_restyled_still_gives_names_and_a_page_rearranged_gives_what_it_can() {
    // no class, no list, no wrapper: the link is the card
    let bare = parse_search("<a href=\"/library/foo\">foo</a> <a href='/library/bar'>bar</a> <a href=/library/baz>baz</a>")
        .unwrap();
    assert_eq!(names(&bare), ["foo", "bar", "baz"]);
    for m in &bare.models {
        assert_eq!(m.description, "");
        assert!(m.sizes.is_empty() && m.capabilities.is_empty());
        assert_eq!(m.downloads, None);
    }
    // attributes in another order, another quoting and an extra attribute before the address
    let other = parse_search(
        "<a data-x=\"y>z\" class='c d' href=\"/library/foo\" id=q><p title='T'>t</p></a>",
    )
    .unwrap();
    assert_eq!(names(&other), ["foo"]);
    assert_eq!(other.models[0].description, "T");

    // the words that say a model runs here changed: no sizes, and still a row to pull by name
    let reworded = page("popular_local.html").replace("Runs on your computer", "Local model");
    let parsed = parse_search(&reworded).unwrap();
    assert_eq!(parsed.models.len(), 20);
    assert!(parsed.models.iter().all(|m| m.sizes.is_empty()));
    assert_eq!(
        model(&parsed, "qwen3.8").capabilities,
        strings(&["tools", "thinking", "vision"])
    );
    // no `title`s: the description is the text, and no count is the loss of the count only
    let untitled = page("popular_local.html").replace(" title=\"", " data-t=\"");
    let parsed = parse_search(&untitled).unwrap();
    assert_eq!(parsed.models.len(), 20);
    assert!(parsed.models[0]
        .description
        .starts_with("Clef-Flash is a 9B multimodal model"));
    assert!(parsed.models.iter().all(|m| m.downloads.is_none()));
    assert_eq!(model(&parsed, "qwen3.8").sizes, strings(&["27b"]));
    // the description says "computer" before the sizes: the sizes are still the sizes
    let chatty = parse_search(
        "<a href=\"/library/foo\"><p title=\"x\">Runs well on any computer</p>\
         <span role=\"tooltip\">Runs on your computer</span><svg></svg><span>4b</span></a>",
    )
    .unwrap();
    assert_eq!(chatty.models[0].sizes, strings(&["4b"]));
}

#[test]
fn p3_u08_a_reference_is_the_name_or_the_name_and_a_size() {
    let popular = read("popular_local.html");
    let qwen = model(&popular, "qwen3.6");
    assert_eq!(qwen.reference(None), "qwen3.6");
    assert_eq!(qwen.reference(Some("35b")), "qwen3.6:35b");
    // every way a row offers to pull a model is a reference the model box takes: lower case name,
    // tag of letters, digits and dots
    for file in ["popular_local.html", "gpt_local.html", "embed_local.html"] {
        let page = read(file);
        for m in &page.models {
            let references = std::iter::once(m.reference(None))
                .chain(m.sizes.iter().map(|s| m.reference(Some(s))));
            for reference in references {
                let (name, tag) = reference.split_once(':').unwrap_or((&reference, ""));
                assert!(valid_name(name), "{reference}");
                assert!(
                    tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '.'),
                    "{reference}"
                );
            }
        }
    }
}

// ---- the document: `[[models.lookup]]` -------------------------------------------------------------------

use quickai_core::config::{load, Layer, LayerSource, LoadOptions, MemorySource};
use quickai_core::diag::Severity;
use quickai_core::models::lookups;
use quickai_core::registry::engine::{CompiledLookup, LookupKind};
use quickai_core::registry::{KnownPrimitives, Registry};

const PROVIDED: &[&str] = &[
    "spawn@1",
    "probe.http@1",
    "locate.setting@1",
    "lookup.ollama_library@1",
];

/// An engine with a `[models]` section that holds the look-ups `lookups` says (TOML for the part
/// after `[models]`).
fn engine_with(models: &str) -> String {
    format!(
        r#"schema = 1
kind = "engine"
id = "acme"
requires = {{ primitives = ["spawn@1", "probe.http@1", "locate.setting@1", "lookup.ollama_library@1"] }}

[engine]
name = "Acme"

[provides]
apis = ["acme.talk"]
default_port = 9000

[locate]
strategies = [ {{ use = "setting", field = "bin_override" }} ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${{runtime.binary}}"
args = ["serve"]

[launch.ready]
probe = {{ use = "http", path = "/api/version", expect = 200 }}

[[field]]
id = "port"
type = "port"
default = 9000
bind.port = "public"

[[field]]
id = "bin_override"
type = "runtime_path"

{models}
"#
    )
}

fn registry(embedded: &str, user: Option<&str>, provided: &[&str]) -> Registry {
    let e = MemorySource::new(&[("embedded:engines/acme.toml", embedded)]);
    let u = MemorySource::new(&[("user:engines/acme.toml", user.unwrap_or(""))]);
    let mut layers: Vec<(Layer, &dyn LayerSource)> = vec![(Layer::Embedded, &e)];
    if user.is_some() {
        layers.push((Layer::User, &u));
    }
    Registry::build(
        load(&layers, &LoadOptions::default()),
        &KnownPrimitives::new(provided),
    )
}

fn texts(registry: &Registry, severity: Severity) -> Vec<String> {
    registry
        .diagnostics
        .iter()
        .filter(|d| d.severity == severity)
        .map(|d| d.to_string())
        .collect()
}

fn library(site: &str) -> CompiledLookup {
    CompiledLookup {
        id: "library".into(),
        kind: LookupKind::OllamaLibrary {
            site: site.into(),
            read: quickai_core::lookup::present::Settings {
                show_variants: 16,
                ..Default::default()
            },
        },
    }
}

#[test]
fn p3_u09_a_look_up_is_what_the_document_declares_and_nothing_else_has_one() {
    // declared with no address: the library's own site
    let declared = registry(
        &engine_with("[models]\n[[models.lookup]]\nid = \"library\"\nuse = \"ollama_library\"\n"),
        None,
        PROVIDED,
    );
    assert!(
        declared.diagnostics.is_empty(),
        "{:#?}",
        declared.diagnostics
    );
    assert_eq!(
        lookups(declared.engine("acme").unwrap()),
        [library("https://ollama.com")]
    );

    // with one: as written, without the slash at its end and the spaces round it
    let moved = registry(
        &engine_with(
            "[models]\n[[models.lookup]]\nid = \"library\"\nuse = \"ollama_library\"\nsite = \" http://127.0.0.1:8080/ \"\n",
        ),
        None,
        PROVIDED,
    );
    assert!(moved.diagnostics.is_empty(), "{:#?}", moved.diagnostics);
    assert_eq!(
        lookups(moved.engine("acme").unwrap()),
        [library("http://127.0.0.1:8080")]
    );

    // two, in the order given
    let two = registry(
        &engine_with(
            "[models]\n[[models.lookup]]\nid = \"b\"\nuse = \"ollama_library\"\n[[models.lookup]]\nid = \"a\"\nuse = \"ollama_library\"\nsite = \"https://x.example\"\n",
        ),
        None,
        PROVIDED,
    );
    let ids: Vec<&str> = lookups(two.engine("acme").unwrap())
        .iter()
        .map(|l| l.id.as_str())
        .collect();
    assert_eq!(ids, ["b", "a"]);

    // a document that says nothing about it has none: no `[models]`, or a `[models]` without
    for models in ["", "[models]\nneeds_server = true\n"] {
        let none = registry(&engine_with(models), None, PROVIDED);
        assert!(none.diagnostics.is_empty(), "{:#?}", none.diagnostics);
        assert!(
            lookups(none.engine("acme").unwrap()).is_empty(),
            "{models:?}"
        );
    }
}

#[test]
fn p3_u10_a_look_up_that_is_wrong_is_refused_with_where_and_what_to_write() {
    let broken = |lookup: &str| {
        registry(
            &engine_with(&format!("[models]\n[[models.lookup]]\n{lookup}")),
            None,
            PROVIDED,
        )
    };
    let one_error = |lookup: &str, words: &[&str]| {
        let r = broken(lookup);
        assert!(
            r.engine("acme").is_none(),
            "a document with `{lookup}` was accepted"
        );
        let errors = texts(&r, Severity::Error);
        assert_eq!(errors.len(), 1, "{lookup}: {errors:?}");
        for word in words {
            assert!(
                errors[0].contains(word),
                "`{word}` is not in: {}",
                errors[0]
            );
        }
    };
    // not one this version has: says which are
    one_error(
        "id = \"x\"\nuse = \"hf_popular\"\n",
        &[
            "`hf_popular` is not a look-up",
            "ollama_library",
            "models.lookup",
        ],
    );
    // addresses that are not
    for site in [
        "\"ftp://x.example\"",
        "\"https://\"",
        "\"x.example\"",
        "5",
        "\"https://a b\"",
        "true",
    ] {
        one_error(
            &format!("id = \"x\"\nuse = \"ollama_library\"\nsite = {site}\n"),
            &["not an http or https address", "https://ollama.com", "site"],
        );
    }
    // names that are not
    one_error(
        "id = \"\"\nuse = \"ollama_library\"\n",
        &["not a name for a look-up"],
    );
    one_error(
        "id = \"two words\"\nuse = \"ollama_library\"\n",
        &["not a name for a look-up"],
    );
    // the same name twice
    let twice = registry(
        &engine_with(
            "[models]\n[[models.lookup]]\nid = \"x\"\nuse = \"ollama_library\"\n[[models.lookup]]\nid = \"x\"\nuse = \"ollama_library\"\n",
        ),
        None,
        PROVIDED,
    );
    let errors = texts(&twice, Severity::Error);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("two look-ups are called `x`"),
        "{}",
        errors[0]
    );

    // a key the look-up does not take is a warning: the document stays and the key does nothing
    let extra = broken(
        "id = \"x\"\nuse = \"ollama_library\"\nsite = \"https://a.example\"\nsort = \"newest\"\n",
    );
    assert!(extra.engine("acme").is_some(), "{:#?}", extra.diagnostics);
    let warnings = texts(&extra, Severity::Warning);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains(
            "`sort` is not a key of the look-up `ollama_library` (site, bytes_per_parameter, show_variants, types)"
        ),
        "{}",
        warnings[0]
    );

    // a build without the primitive skips the document and names what it lacks
    let lacking = registry(
        &engine_with("[models]\n[[models.lookup]]\nid = \"x\"\nuse = \"ollama_library\"\n"),
        None,
        &["spawn@1", "probe.http@1", "locate.setting@1"],
    );
    assert!(lacking.engine("acme").is_none());
    let errors = texts(&lacking, Severity::Error);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("`lookup.ollama_library@1`") && e.contains("requires.primitives")),
        "{errors:?}"
    );
}

#[test]
fn p3_u11_a_document_of_the_persons_can_move_a_look_up_and_leave_the_rest() {
    let embedded = engine_with(
        "[models]\n[[models.lookup]]\nid = \"library\"\nuse = \"ollama_library\"\nsite = \"https://ollama.com\"\n",
    );
    // lookups merge by their id, as every list of tables does: only the address is the person's
    let user = "schema = 1\nkind = \"engine\"\nid = \"acme\"\n\n[[models.lookup]]\nid = \"library\"\nsite = \"http://mirror.example:8000\"\n";
    let r = registry(&embedded, Some(user), PROVIDED);
    assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
    assert_eq!(
        lookups(r.engine("acme").unwrap()),
        [library("http://mirror.example:8000")]
    );

    // a broken one of theirs is skipped, and the document under it stays in force
    let bad = "schema = 1\nkind = \"engine\"\nid = \"acme\"\n\n[[models.lookup]]\nid = \"library\"\nsite = \"gopher://x\"\n";
    let r = registry(&embedded, Some(bad), PROVIDED);
    assert_eq!(
        lookups(r.engine("acme").unwrap()),
        [library("https://ollama.com")]
    );
    assert_eq!(texts(&r, Severity::Error).len(), 1, "{:#?}", r.diagnostics);

    // and they can add a look-up of their own beside it
    let more = "schema = 1\nkind = \"engine\"\nid = \"acme\"\n\n[[models.lookup]]\nid = \"mirror\"\nuse = \"ollama_library\"\nsite = \"http://mirror.example\"\n";
    let r = registry(&embedded, Some(more), PROVIDED);
    let ids: Vec<&str> = lookups(r.engine("acme").unwrap())
        .iter()
        .map(|l| l.id.as_str())
        .collect();
    assert_eq!(ids, ["library", "mirror"]);
}
