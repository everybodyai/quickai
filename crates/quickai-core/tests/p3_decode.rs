//! How the proxy reads an API is the vocabulary's to say (work package 3.6, docs/phase3-test-plan.md
//! group D): an API that only a document defines is read with no change to the code, a document
//! that says it wrongly is reported where it is wrong, and an engine that names an API no
//! vocabulary defines does not load.

use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
use quickai_core::plan::{plan, HostFacts, Selection, TRAFFIC_PROXY_KEY};
use quickai_core::registry::{KnownPrimitives, Registry};
use serde_json::json;

/// An API of a made-up engine, with its own paths and pointers, not one of the code's.
const ACME: &str = r#"schema = 1
kind = "vocab"
id = "acme"
requires = { primitives = ["decode@1"] }

[[api]]
id = "acme.talk"
paths = ["/v2/talk"]
capabilities = ["chat"]

[api.decode]
framing = ["ndjson", "json"]
delta = ["/piece"]
final = ["/answer"]
error = ["/problem"]

[api.decode.chat]
messages = "/turns"
role = "/who"
content = "/say"
skip_roles = ["rules"]
images = "/pictures"
"#;

const ENGINE: &str = r#"schema = 1
kind = "engine"
id = "acme"
requires = { primitives = ["spawn@1", "probe.http@1", "locate.setting@1"] }

[engine]
name = "Acme"

[provides]
apis = ["acme.talk"]
default_port = 9000

[locate]
strategies = [ { use = "setting", field = "bin_override" } ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${runtime.binary}"
args = ["serve", "--port", "${listen_port}"]

[launch.ready]
probe = { use = "http", path = "/health", expect = 200 }

[[field]]
id = "port"
type = "port"
default = 9000
bind.port = "public"

[[field]]
id = "bin_override"
type = "runtime_path"
"#;

fn registry(docs: &[(&str, &str)]) -> Registry {
    let source = MemorySource::new(docs);
    Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &KnownPrimitives::new(&["spawn@1", "probe.http@1", "locate.setting@1", "decode@1"]),
    )
}

fn error_texts(registry: &Registry) -> Vec<String> {
    registry
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.to_string())
        .collect()
}

#[test]
fn p3_d08a_an_api_only_a_document_defines_is_read_by_its_pointers_with_no_code_change() {
    let registry = registry(&[("embedded:vocab/acme.toml", ACME)]);
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let ids = ["acme.talk".to_string()];
    let decoders = registry.decoders_for(&ids);
    assert_eq!(decoders.len(), 1);
    let talk = &decoders[0];
    assert!(talk.matches("/v2/talk") && !talk.matches("/v1/chat/completions"));

    // the rules come last here: the last turn that is not theirs is what the person said
    let request = json!({"turns": [
        {"who": "person", "say": "hello", "pictures": ["iVBORw0KGgoAAAA", "AAAA"]},
        {"who": "rules", "say": "be brief"},
    ]});
    let said = talk.request(Some(&request)).expect("a conversation");
    assert_eq!(said.text.as_deref(), Some("hello"));
    assert_eq!(said.attachments.len(), 2);
    assert_eq!(said.attachments[0].mime.as_deref(), Some("image/png"));

    assert_eq!(talk.delta(&json!({"piece": "Hel"})).as_deref(), Some("Hel"));
    assert_eq!(talk.delta(&json!({"message": {"content": "x"}})), None);
    assert_eq!(
        talk.final_text(&json!({"answer": "Hello"})).as_deref(),
        Some("Hello")
    );
    assert_eq!(
        talk.error(&json!({"problem": "out of memory"})).as_deref(),
        Some("out of memory")
    );
    // nothing to say is a load, as for every API
    assert_eq!(talk.request(Some(&json!({"turns": []}))), None);
}

#[test]
fn p3_d08b_an_engine_naming_an_api_the_vocabulary_lacks_is_a_load_diagnostic() {
    let engine = ENGINE.replace("apis = [\"acme.talk\"]", "apis = [\"acme.talkk\"]");
    let registry = registry(&[
        ("embedded:vocab/acme.toml", ACME),
        ("embedded:engines/acme.toml", &engine),
    ]);
    assert!(registry.engine("acme").is_none(), "the engine loaded");
    let errors = error_texts(&registry);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].contains("provides.apis[0]")
            && errors[0].contains("`acme.talkk` is not an API any vocabulary defines")
            && errors[0].contains("did you mean `acme.talk`?"),
        "{}",
        errors[0]
    );

    // the same engine with the right name loads
    let ok = registry_with_engine(ENGINE);
    assert!(ok.engine("acme").is_some(), "{:#?}", ok.diagnostics);

    // with no vocabulary at all there is nothing to hold the names to (a registry of one engine)
    let alone = self::registry(&[("embedded:engines/acme.toml", &engine)]);
    assert!(alone.engine("acme").is_some(), "{:#?}", alone.diagnostics);
}

/// The layers named: the embedded documents first, then the person's.
fn layered(embedded: &[(&str, &str)], user: &[(&str, &str)]) -> Registry {
    let (embedded, user) = (MemorySource::new(embedded), MemorySource::new(user));
    Registry::build(
        load(
            &[(Layer::Embedded, &embedded), (Layer::User, &user)],
            &LoadOptions::default(),
        ),
        &KnownPrimitives::new(&["spawn@1", "probe.http@1", "locate.setting@1", "decode@1"]),
    )
}

fn registry_with_engine(engine: &str) -> Registry {
    registry(&[
        ("embedded:vocab/acme.toml", ACME),
        ("embedded:engines/acme.toml", engine),
    ])
}

#[test]
fn p3_d08c_a_vocabulary_that_says_it_wrongly_is_reported_where_it_is_wrong() {
    let cases: [(&str, &str, &str); 11] = [
        ("framing = [\"ndjson\", \"json\"]", "framing = [\"xml\"]", "`xml` is not a framing"),
        ("framing = [\"ndjson\", \"json\"]", "framing = []", "`framing` says how a reply comes"),
        ("delta = [\"/piece\"]", "delta = [\"piece\"]", "`piece` is not a JSON pointer"),
        ("final = [\"/answer\"]", "final = [\"/answer\", \"answer\"]", "api[acme.talk].decode.final[1]"),
        ("messages = \"/turns\"", "messages = \"turns\"", "api[acme.talk].decode.chat.messages"),
        ("paths = [\"/v2/talk\"]", "paths = [\"v2/talk\"]", "is not a path"),
        ("paths = [\"/v2/talk\"]", "paths = [\"/v2/talk?x=1\"]", "is not a path"),
        ("paths = [\"/v2/talk\"]", "paths = []", "is decoded but has no `paths`"),
        ("id = \"acme.talk\"", "id = \"Acme Talk\"", "is not a valid API id"),
        (
            "error = [\"/problem\"]",
            "error = [\"/problem\"]\nprompt = \"/prompt\"",
            "not both",
        ),
        (
            "[api.decode.chat]\nmessages = \"/turns\"\nrole = \"/who\"\ncontent = \"/say\"\nskip_roles = [\"rules\"]\nimages = \"/pictures\"\n",
            "",
            "says where the text of a request is",
        ),
    ];
    for (old, new, wanted) in cases {
        assert!(ACME.contains(old), "the document has no `{old}`");
        let registry = registry(&[("embedded:vocab/acme.toml", &ACME.replace(old, new))]);
        let errors = error_texts(&registry);
        assert!(
            errors.iter().any(|e| e.contains(wanted)),
            "`{new}` should be reported with `{wanted}`: {errors:#?}"
        );
        assert!(
            registry.vocabulary().api("acme.talk").is_none(),
            "a vocabulary with an error defines nothing"
        );
    }

    // two documents may not claim the same API or the same path
    let twin = ACME.replace("id = \"acme\"", "id = \"twin\"");
    let registry = registry(&[
        ("embedded:vocab/acme.toml", ACME),
        ("embedded:vocab/twin.toml", &twin),
    ]);
    let errors = error_texts(&registry);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("the API `acme.talk` is already defined by the vocabulary `acme`")),
        "{errors:#?}"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("the path `/v2/talk` already belongs to the API `acme.talk`")),
        "{errors:#?}"
    );
    assert!(
        registry.vocabulary().api("acme.talk").is_some(),
        "the first one stands"
    );

    // a vocabulary that needs a primitive this build lacks is skipped with it named
    let old = ACME.replace("decode@1", "decode@9");
    let registry = self::registry(&[("embedded:vocab/acme.toml", &old)]);
    assert!(
        error_texts(&registry)
            .iter()
            .any(|e| e.contains("`decode@9`")),
        "{:#?}",
        registry.diagnostics
    );
}

#[test]
fn p3_d08d_the_plan_carries_the_decoders_of_the_engines_apis_when_the_proxy_is_on() {
    let registry = registry_with_engine(ENGINE);
    assert!(
        registry.engine("acme").is_some(),
        "{:#?}",
        registry.diagnostics
    );
    let facts = HostFacts {
        os: "linux".into(),
        arch: "x86_64".into(),
        binary: Some("/opt/acme/acme".into()),
        internal_port: Some(19000),
        ..Default::default()
    };
    let with_proxy = plan(
        &registry,
        &Selection::new("acme").set(TRAFFIC_PROXY_KEY, true),
        &facts,
    )
    .unwrap_or_else(|e| panic!("{e:#?}"));
    assert_eq!(
        with_proxy
            .decode
            .iter()
            .map(|d| d.id.as_str())
            .collect::<Vec<_>>(),
        ["acme.talk"]
    );
    assert_eq!(with_proxy.decode[0].paths, ["/v2/talk"]);
    let without = plan(&registry, &Selection::new("acme"), &facts).unwrap();
    assert!(
        without.decode.is_empty(),
        "no gateway, nothing to read with"
    );
    assert_eq!(
        without.units[0].apis,
        ["acme.talk"],
        "the unit still says what it serves"
    );
}

#[test]
fn p3_d08f_a_vocabulary_of_the_person_patches_the_embedded_one_by_api_id() {
    // what the reference page says: same `id`, tables by key, `[[api]]` by `id`
    let patch = r#"schema = 1
kind = "vocab"
id = "acme"

[[api]]
id = "acme.talk"
paths = ["/v2/talk", "/talk"]

[api.decode]
delta = ["/piece", "/chunk"]

[[api]]
id = "acme.listen"
paths = ["/v2/listen"]

[api.decode]
framing = ["json"]
prompt = "/said"
"#;
    let registry = layered(
        &[("embedded:vocab/acme.toml", ACME)],
        &[("user:vocab/acme.toml", patch)],
    );
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let ids = ["acme.talk".to_string(), "acme.listen".to_string()];
    let decoders = registry.decoders_for(&ids);
    assert_eq!(decoders.len(), 2);
    let talk = &decoders[0];
    // the paths were replaced (a list is not merged) and the pointers of the table were merged
    assert!(talk.matches("/talk") && talk.matches("/v2/talk"));
    assert_eq!(talk.delta(&json!({"chunk": "x"})).as_deref(), Some("x"));
    assert_eq!(
        talk.final_text(&json!({"answer": "all"})).as_deref(),
        Some("all"),
        "what the patch did not say stands"
    );
    assert_eq!(
        talk.chat.as_ref().map(|c| c.messages.as_str()),
        Some("/turns")
    );
    // and an API the patch adds is read like any other
    let listen = &decoders[1];
    assert_eq!(
        listen
            .request(Some(&json!({"said": "hi"})))
            .and_then(|r| r.text)
            .as_deref(),
        Some("hi")
    );
}
