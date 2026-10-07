//! Group M of the test plan: the models of an engine that keeps its own, listed, pulled and removed
//! over HTTP (work package 3.13). The engine is the fake of `testkit`, which answers the way the
//! recordings of a real Ollama say it does; the calls are the ones the engine's document describes.

use quickai_core::models::{list_call, pull_call, remove_call};
use quickai_core::plan::{Endpoint, HostFacts, ModelInfo, Selection};
use quickai_core::registry::Registry;
use serde_json::{json, Value};

use crate::hardware;
use crate::models::ModelEntry;
use crate::prims::engine_models::{
    ask_to_remove, list_models, models_to_show, spawn_pull, ListSpec, PullSpec,
};
use crate::prims::http_call::{call, Call, Framing};
use crate::testkit::fake_ollama::FakeOllama;

/// An engine whose models are listed, pulled and removed by calls, as the Ollama document says.
const DOC: &str = r#"schema = 1
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
args = ["serve"]

[launch.ready]
probe = { use = "http", path = "/api/version", expect = 200 }

[[field]]
id = "port"
type = "port"
default = 9000
bind.port = "public"

[[field]]
id = "bin_override"
type = "runtime_path"

[models]
needs_server = true
list = { call = { method = "GET", path = "/api/tags" }, items = "/models", map = { id = "/name", display = "/name", size_bytes = "/size", digest = "/digest", capabilities = "/capabilities" }, cache = "${data}/acme/catalog-cache.json" }
pull = { call = { method = "POST", path = "/api/pull", body = { model = "${ref}" }, stream = "ndjson" }, progress = { key = "/digest", completed = "/completed", total = "/total", status = "/status" } }
remove = { call = { method = "DELETE", path = "/api/delete", body = { model = "${ref}" } } }
"#;

fn registry() -> Registry {
    use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
    let source = MemorySource::new(&[("embedded:engines/acme.toml", DOC)]);
    let registry = Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &crate::prims::known(),
    );
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    registry
}

fn facts(data_dir: &std::path::Path) -> HostFacts {
    HostFacts {
        os: "linux".into(),
        arch: "x86_64".into(),
        data_dir: data_dir.display().to_string(),
        ..HostFacts::default()
    }
}

fn endpoint(port: u16) -> Endpoint {
    Endpoint {
        host: "127.0.0.1".into(),
        port: Some(port),
    }
}

/// The list spec the document gives, with its cache in `dir`.
fn list_spec(registry: &Registry, dir: &std::path::Path) -> ListSpec {
    let engine = registry.engine("acme").unwrap();
    let list = list_call(engine, &facts(dir))
        .expect("the document lists models")
        .expect("and it renders");
    ListSpec::from_call(&list)
}

#[test]
fn p3_m02_the_model_list_is_asked_of_a_running_daemon_only_and_a_stopped_one_shows_what_was_kept() {
    let dir = crate::testkit::scratch("m02");
    let registry = registry();
    assert!(quickai_core::models::needs_server(
        registry.engine("acme").unwrap()
    ));
    let spec = list_spec(&registry, &dir);
    assert_eq!(
        spec.cache.as_deref(),
        Some(dir.join("acme/catalog-cache.json").as_path()),
        "the cache is where the document says"
    );

    let fake = FakeOllama::start();
    fake.add_model("qwen3:4b", 2_500_000_000)
        .add_model("bonsai-q1", 248_302_385);
    // choosing the engine, and working out what to ask, asks nothing
    assert!(fake.requests().is_empty());

    // stopped, with nothing kept: an empty list that says it is not fresh, and nothing is asked
    let nothing = models_to_show(None, &spec).unwrap();
    assert_eq!((nothing.rows.len(), nothing.stale), (0, true));
    assert!(fake.requests().is_empty(), "a stopped daemon is not asked");

    // running: asked once, and what it says is kept
    let live = models_to_show(Some(&endpoint(fake.port())), &spec).unwrap();
    assert_eq!((live.rows.len(), live.stale), (2, false));
    assert_eq!(fake.requests_to("/api/tags").len(), 1);

    // stopped again: the same list, marked as kept, with no new request
    let kept = models_to_show(None, &spec).unwrap();
    assert!(kept.stale, "what is shown from the cache says so");
    assert_eq!(kept.rows, live.rows);
    assert_eq!(fake.requests_to("/api/tags").len(), 1, "still the one");

    // a daemon that was running and has gone: asking fails over to the same remembered list
    fake.go_down();
    let gone = models_to_show(Some(&endpoint(fake.port())), &spec).unwrap();
    assert!(gone.stale);
    assert_eq!(gone.rows, live.rows);
}

#[test]
fn p3_m03_removing_a_model_asks_first_sends_the_delete_with_its_name_and_never_the_loaded_one() {
    let dir = crate::testkit::scratch("m03");
    let registry = registry();
    let engine = registry.engine("acme").unwrap();
    let spec = list_spec(&registry, &dir);
    let fake = FakeOllama::start();
    fake.add_model("bonsai-q1", 248_302_385)
        .add_model("qwen3:4b", 2_500_000_000);
    let at = endpoint(fake.port());
    // qwen3:4b is the loaded one
    call(
        &at,
        &Call {
            method: "POST".into(),
            path: "/api/generate".into(),
            body: Some(json!({"model": "qwen3:4b", "keep_alive": -1})),
            framing: Framing::Json,
            timeout: std::time::Duration::from_secs(5),
        },
    )
    .unwrap();
    assert_eq!(fake.loaded().as_deref(), Some("qwen3:4b"));
    let loaded = fake.loaded();

    // the loaded model is refused, in either spelling, and nothing is sent
    for name in ["qwen3:4b", "library/qwen3:4b"] {
        let refused = ask_to_remove(name, loaded.as_deref()).expect_err("it is loaded");
        assert!(
            refused.contains("is loaded") && refused.contains("Stop the server"),
            "{refused}"
        );
    }
    assert!(fake.requests_to("/api/delete").is_empty());

    // another model asks first: until the person says yes nothing is sent, and no is nothing sent
    let pending = ask_to_remove("bonsai-q1:latest", loaded.as_deref()).expect("it may go");
    assert!(fake.requests_to("/api/delete").is_empty(), "asked first");
    drop(pending);
    assert!(
        fake.requests_to("/api/delete").is_empty(),
        "no is no request"
    );

    // yes: the engine is asked, by the name it lists, and the list is asked for again
    let pending = ask_to_remove("bonsai-q1:latest", loaded.as_deref()).unwrap();
    let remove = remove_call(engine, &pending.name)
        .expect("the document removes models")
        .expect("and it renders");
    pending
        .confirm(&at, &remove)
        .expect("the engine removed it");
    let deletes = fake.requests_to("/api/delete");
    assert_eq!(deletes.len(), 1);
    assert_eq!(deletes[0].method, "DELETE");
    assert_eq!(deletes[0].body, Some(json!({"model": "bonsai-q1:latest"})));
    let after = list_models(&at, &spec).unwrap();
    let names: Vec<&str> = after.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(names, ["qwen3:4b"]);

    // a removal the engine will not do says why in its own words
    let pending = ask_to_remove("not-there:latest", None).unwrap();
    let remove = remove_call(engine, &pending.name).unwrap().unwrap();
    let why = pending
        .confirm(&at, &remove)
        .expect_err("there is no such model");
    assert!(why.contains("not found"), "{why}");
}

#[test]
fn p3_m05_the_size_the_listing_gives_is_the_size_the_heuristics_use_for_the_model() {
    let dir = crate::testkit::scratch("m05");
    let registry = registry();
    let spec = list_spec(&registry, &dir);
    let fake = FakeOllama::start();
    fake.add_model("qwen3:4b", 2_500_000_000)
        .add_model("bonsai-q1", 248_302_385);
    let rows = list_models(&endpoint(fake.port()), &spec).unwrap().rows;
    let entries: Vec<ModelEntry> = rows.iter().map(ModelEntry::from_row).collect();

    // the entry is what the listing says: the name the engine knows, no file, the listed size
    let qwen = entries
        .iter()
        .find(|e| e.filename.starts_with("qwen3"))
        .unwrap();
    assert_eq!(qwen.size_bytes, 2_500_000_000);
    assert!(qwen.path.as_os_str().is_empty());
    assert_eq!(
        qwen.info(),
        ModelInfo {
            id: "qwen3:4b".into(),
            display: "qwen3:4b".into(),
            path: String::new(),
            size_bytes: 2_500_000_000,
        }
    );

    // a model the listing gives no size for counts as 0, as a file that cannot be read does
    let mut no_size = rows[0].clone();
    no_size.size_bytes = None;
    assert_eq!(ModelEntry::from_row(&no_size).size_bytes, 0);

    // the recommendation and the speed estimate work from that size, exactly as for a file of it
    let file = |name: &str, size: u64| ModelEntry {
        filename: name.into(),
        path: name.into(),
        display: name.into(),
        note: String::new(),
        bundled: false,
        size_bytes: size,
    };
    let ram = 16 * 1024 * 1024 * 1024;
    for gpu in [false, true] {
        let as_files: Vec<ModelEntry> = entries
            .iter()
            .map(|e| file(&e.filename, e.size_bytes))
            .collect();
        assert_eq!(
            hardware::recommend(&entries, ram, gpu),
            hardware::recommend(&as_files, ram, gpu),
            "gpu offload {gpu}"
        );
        assert_eq!(
            hardware::speed_tier(qwen.size_bytes, ram, gpu),
            hardware::speed_tier(file("q", qwen.size_bytes).size_bytes, ram, gpu)
        );
    }
    assert_eq!(
        hardware::recommend(&entries, ram, false).as_deref(),
        Some("qwen3:4b"),
        "the larger of two that fit is the pick"
    );

    // and the context size the form proposes comes from it, as it does from a file's size
    let llama = crate::config::build_registry(&crate::config::ConfigDirs::default());
    let form = |entry: &ModelEntry| {
        let facts = HostFacts {
            ram_bytes: ram,
            ..HostFacts::default()
        };
        let selection = Selection::new("llamacpp").with_model(entry.info());
        let form = quickai_core::form::FormModel::build(&llama, &selection, &facts)
            .unwrap_or_else(|e| panic!("{e:#?}"));
        form.sections
            .iter()
            .flat_map(|s| &s.fields)
            .find(|f| f.id == "ctx_size")
            .map(|f| f.value.clone())
            .expect("the context size field")
    };
    assert_eq!(
        form(qwen),
        form(&file("qwen3-4b.gguf", 2_500_000_000)),
        "the same size gives the same proposal"
    );
    assert_ne!(
        form(qwen),
        form(&file("tiny.gguf", 1_000)),
        "and it does depend on the size"
    );
}

#[test]
fn p3_m06c_the_document_gives_the_calls_the_built_in_specs_make() {
    let dir = crate::testkit::scratch("m06c");
    let registry = registry();
    let engine = registry.engine("acme").unwrap();
    let fake = FakeOllama::start();
    fake.add_model("bonsai-q1", 248_302_385);

    // the list: the same rows through the document's call as through the built-in one
    let from_document = list_models(&endpoint(fake.port()), &list_spec(&registry, &dir)).unwrap();
    let built_in = list_models(&endpoint(fake.port()), &ListSpec::ollama(None)).unwrap();
    assert_eq!(from_document.rows, built_in.rows);
    assert_eq!(from_document.rows.len(), 1);

    // the pull: what the document's call asks the fake for is pulled, with progress to the end
    let pull = pull_call(engine, "qwen3:4b")
        .expect("pulls")
        .expect("renders");
    let (mut job, _) = spawn_pull(
        endpoint(fake.port()),
        PullSpec::from_call(&pull),
        "qwen3:4b".into(),
        || {},
    );
    let outcome = job
        .wait(std::time::Duration::from_secs(20))
        .expect("the pull ended");
    assert!(outcome.is_ok(), "{outcome:?}");
    let bodies: Vec<Option<Value>> = fake
        .requests_to("/api/pull")
        .into_iter()
        .map(|r| r.body)
        .collect();
    assert_eq!(bodies, [Some(json!({"model": "qwen3:4b"}))]);
    assert!(fake.model_names().iter().any(|n| n == "qwen3:4b"));
}
