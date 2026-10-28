//! Group J of docs/phase3-test-plan.md, the pure half: the list of recommended models (work package
//! 4.10, docs/catalog.md). How a catalog document is read and refused, how a person's document patches
//! it, what an engine offers of it and how the entries look, how a list is browsed, and how the engine's
//! mode reaches the command line. The engines and the vocabulary are the documents the application
//! ships; the models are made up (`acme/...`) so that no test depends on what the real list holds.

mod common;

use common::{registry, shipped_catalogs, shipped_registry, texts};
use quickai_core::diag::Severity;
use quickai_core::lookup::catalog::{entries, Context};
use quickai_core::lookup::present::{Download, Entry, Fetch, Offer};
use quickai_core::lookup::view::{browse, Browse, Scope};
use quickai_core::models::{bundles, modes, recommended};
use quickai_core::plan::{plan, HostFacts, ModelInfo, Selection, Start};
use quickai_core::registry::catalog::{FilesRun, Run};

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const REV: &str = "0123456789abcdef0123456789abcdef01234567";

/// A catalog document of the person's, or of a test: `id` names the document.
fn doc(id: &str, body: &str) -> String {
    format!(
        "schema = 1\nkind = \"catalog\"\nid = \"{id}\"\ntitle = \"Test {id}\"\nverified = \"2026-10-07\"\n\n{body}"
    )
}

/// A model that has everything: two quantizations and a projector for llama.cpp, a name for Ollama.
fn alpha() -> String {
    format!(
        r#"[[model]]
id = "alpha-4b"
name = "Alpha 4B"
by = "Acme"
type = "chat"
note = "A small chat model."
license = "Apache-2.0"
params = "4B"
context = 131072
input = ["text", "image"]
features = ["thinking", "tools"]
start = true

[model.run.llamacpp]
repo = "acme/Alpha-4B-GGUF"
rev = "{REV}"
files = [
  {{ file = "Alpha-4B-Q8_0.gguf", size = 4300000000, sha256 = "{B}" }},
  {{ file = "Alpha-4B-Q4_K_M.gguf", size = 2500000000, sha256 = "{A}", best = true }},
]
[model.run.llamacpp.companions]
mmproj = {{ file = "mmproj-Alpha-4B-F16.gguf", size = 800000000, sha256 = "{C}" }}

[model.run.ollama]
ref = "alpha:4b"
size = 3300000000
"#
    )
}

fn with_alpha() -> quickai_core::registry::Registry {
    registry(&[("user:catalog/t.toml", &doc("t", &alpha()))])
}

fn errors(r: &quickai_core::registry::Registry) -> Vec<String> {
    texts(r, Severity::Error)
}

fn ctx<'a>(
    r: &'a quickai_core::registry::Registry,
    engine: &'a str,
    site: Option<&'a str>,
) -> Context<'a> {
    Context {
        engine,
        sizes: r.vocabulary().size_classes(),
        bundles: bundles(r.engine(engine).expect("the engine")),
        site,
    }
}

fn files_of(offer: &Offer) -> &Download {
    match &offer.fetch {
        Fetch::Files(d) => d,
        other => panic!("not files: {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------- J01

#[test]
fn p3_j01_a_catalog_document_is_read_with_what_it_says_and_sensible_defaults() {
    let r = with_alpha();
    assert!(errors(&r).is_empty(), "{:#?}", r.diagnostics);
    let model = r.catalog().model("alpha-4b").expect("the model");
    assert_eq!(model.name, "Alpha 4B");
    assert_eq!(model.by, "Acme");
    assert_eq!(model.kind, "chat");
    assert_eq!(model.note, "A small chat model.");
    assert_eq!(model.license, "Apache-2.0");
    assert_eq!(model.params, "4B");
    assert_eq!(model.context, Some(131072));
    assert_eq!(model.input, ["text", "image"]);
    assert_eq!(model.features, ["thinking", "tools"]);
    assert!(model.start && !model.bundled);
    assert_eq!(model.document, "t");
    match &model.runs["llamacpp"] {
        Run::Files(FilesRun {
            repo,
            rev,
            mode,
            files,
            companions,
            bundle,
            place,
        }) => {
            assert_eq!(*place, None);
            assert_eq!(repo, "acme/Alpha-4B-GGUF");
            assert_eq!(rev, REV);
            assert_eq!(*mode, None);
            // in the order the document gives them
            assert_eq!(files[0].path, "Alpha-4B-Q8_0.gguf");
            assert_eq!(files[1].sha256.as_deref(), Some(A));
            assert!(files[1].best && !files[0].best);
            assert_eq!(companions.len(), 1);
            assert_eq!(companions[0].0, "mmproj");
            // the engine's bundle that pairs weights with exactly a projector
            assert_eq!(bundle.as_deref(), Some("vision"));
        }
        other => panic!("{other:?}"),
    }
    match &model.runs["ollama"] {
        Run::Reference(run) => {
            assert_eq!(run.reference, "alpha:4b");
            assert_eq!(run.size, Some(3_300_000_000));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(r.catalog().verified(), Some("2026-10-07"));
    assert_eq!(r.catalog().pages()[0].title, "Test t");

    // a model that says only what it must: the name is the id, the type the vocabulary's default,
    // what it reads is text, and the revision is the branch
    let r = registry(&[(
        "user:catalog/bare.toml",
        &doc(
            "bare",
            &format!(
                "[[model]]\nid = \"bare\"\n[model.run.llamacpp]\nrepo = \"acme/Bare\"\nfiles = [ {{ file = \"bare.gguf\" }} ]\n[model.run.ollama]\nref = \"bare\"\n{}",
                ""
            ),
        ),
    )]);
    assert!(errors(&r).is_empty(), "{:#?}", r.diagnostics);
    let bare = r.catalog().model("bare").unwrap();
    assert_eq!(bare.name, "bare");
    assert_eq!(bare.kind, "chat");
    assert_eq!(bare.input, ["text"]);
    assert_eq!((bare.by.as_str(), bare.note.as_str()), ("", ""));
    assert!(!bare.start);
    match &bare.runs["llamacpp"] {
        Run::Files(run) => {
            assert_eq!(run.rev, "main");
            assert_eq!(run.files[0].size, None);
            assert_eq!(run.files[0].sha256, None);
            assert_eq!(run.bundle, None);
        }
        other => panic!("{other:?}"),
    }
    // a library name with no tag is the model's default tag
    match &bare.runs["ollama"] {
        Run::Reference(run) => assert_eq!((run.reference.as_str(), run.size), ("bare", None)),
        other => panic!("{other:?}"),
    }
    // a registry with no catalog document has an empty list, which is not an error
    let none = registry(&[]);
    assert!(none.catalog().models().is_empty() || !shipped_catalogs().is_empty());
}

// ---------------------------------------------------------------------------------------------- J02

/// A document whose model says `edit` of the model alpha (a replacement of a line of it): the error it
/// is refused with, and that none of its models is in the list.
fn refused(edit: impl Fn(&str) -> String, expect: &str) {
    let r = registry(&[("user:catalog/t.toml", &doc("t", &edit(&alpha())))]);
    let found = errors(&r);
    assert!(
        found.iter().any(|e| e.contains(expect)),
        "expected `{expect}` in {found:#?}"
    );
    assert!(
        r.catalog().model("alpha-4b").is_none(),
        "a document with an error is refused as a whole ({expect})"
    );
}

#[test]
fn p3_j02_what_cannot_be_right_is_refused_where_it_is_written() {
    // the model itself
    refused(
        |a| a.replace("id = \"alpha-4b\"", "id = \"Alpha 4B\""),
        "not a valid model id",
    );
    refused(
        |a| a.replace("type = \"chat\"", "type = \"chatt\""),
        "is not a model type any vocabulary defines",
    );
    refused(
        |a| {
            a.replace(
                "input = [\"text\", \"image\"]",
                "input = [\"text\", \"imagee\"]",
            )
        },
        "is not a modality any vocabulary defines",
    );
    refused(
        |a| {
            a.replace(
                "features = [\"thinking\", \"tools\"]",
                "features = [\"Thinking\"]",
            )
        },
        "is not a short lower case word",
    );
    refused(
        |a| {
            a.replace(
                "features = [\"thinking\", \"tools\"]",
                "features = [\"a\", \"b\", \"c\", \"d\", \"e\", \"f\", \"g\", \"h\", \"i\"]",
            )
        },
        "shows at most 8 features",
    );
    refused(
        |a| a.replace("context = 131072", "context = 0"),
        "a context of 0 tokens",
    );
    // the engine it names
    refused(
        |a| a.replace("[model.run.ollama]", "[model.run.llamacp]"),
        "there is no engine called `llamacp`",
    );
    // files: where they are and what they are
    refused(
        |a| a.replace("repo = \"acme/Alpha-4B-GGUF\"", "repo = \"Alpha\""),
        "is not a repository",
    );
    refused(
        |a| a.replace(REV, "main/../x"),
        "is not a commit or a branch",
    );
    refused(
        |a| {
            a.replace(
                "file = \"Alpha-4B-Q4_K_M.gguf\"",
                "file = \"../Alpha-4B-Q4_K_M.gguf\"",
            )
        },
        "is not a path inside the repository",
    );
    refused(
        |a| {
            a.replace(
                "file = \"Alpha-4B-Q4_K_M.gguf\"",
                "file = \"Alpha-4B-Q4_K_M.bin\"",
            )
        },
        "is not a file of a format the engine takes",
    );
    refused(
        |a| a.replace("size = 2500000000", "size = 0"),
        "a size of 0 bytes",
    );
    refused(|a| a.replace(A, "nothex"), "is not a SHA-256");
    refused(
        |a| {
            a.replace(
                "{ file = \"Alpha-4B-Q8_0.gguf\"",
                "{ best = true, file = \"Alpha-4B-Q8_0.gguf\"",
            )
        },
        "only one of the weights is the best",
    );
    refused(
        |a| {
            a.replace(
                "file = \"Alpha-4B-Q8_0.gguf\"",
                "file = \"Alpha-4B-Q4_K_M.gguf\"",
            )
        },
        "is listed twice",
    );
    refused(
        |a| {
            let start = a.find("files = [").unwrap();
            let end = a.find("[model.run.llamacpp.companions]").unwrap();
            format!("{}files = []\n{}", &a[..start], &a[end..])
        },
        "a model with no weights to fetch",
    );
    // what goes with the weights, and how the engine is started
    refused(
        |a| a.replace("mmproj = {", "lora = {"),
        "has no component called `lora`",
    );
    refused(
        |a| {
            a.replace(
                "[model.run.llamacpp]",
                "[model.run.llamacpp]\nmode = \"embeddings\"",
            )
        },
        "has no mode called `embeddings`",
    );
    // a name where files are wanted and the reverse
    refused(
        |a| {
            a.replace(
                "[model.run.ollama]\nref",
                "[model.run.ollama]\nrepo = \"acme/x\"\nref",
            )
        },
        "pulls a model by name, so `repo` has no meaning here",
    );
    refused(
        |a| {
            a.replace(
                "[model.run.llamacpp]",
                "[model.run.llamacpp]\nref = \"alpha:4b\"",
            )
        },
        "takes files, so `ref` has no meaning here",
    );
    refused(
        |a| a.replace("ref = \"alpha:4b\"", "ref = \"Alpha:4B\""),
        "is not a name `ollama` can pull",
    );
    refused(
        |a| a.replace("ref = \"alpha:4b\"\n", ""),
        "pulls a model by name: write `ref`",
    );
    refused(
        |a| a.replace("size = 3300000000", "size = 0"),
        "a size of 0 bytes",
    );
    // a hint says what to write, and the nearest word when there is one
    let r = registry(&[(
        "user:catalog/t.toml",
        &doc("t", &alpha().replace("type = \"chat\"", "type = \"vison\"")),
    )]);
    let hints: Vec<String> = r
        .diagnostics
        .iter()
        .filter_map(|d| d.hint.clone())
        .collect();
    assert!(
        hints.iter().any(|h| h.contains("did you mean `vision`?")),
        "{hints:?}"
    );
    // a document that does not say what it must is told so too
    let r = registry(&[(
        "user:catalog/t.toml",
        &doc("t", "[[model]]\nname = \"No id\"\n"),
    )]);
    assert!(!errors(&r).is_empty());
    // the same id twice in one document
    let both = format!("{}\n{}", alpha(), alpha());
    let r = registry(&[("user:catalog/t.toml", &doc("t", &both))]);
    assert!(errors(&r).iter().any(|e| e.contains("is written twice")));
}

#[test]
fn p3_j02b_a_companion_the_engine_cannot_pair_with_weights_is_refused_with_the_bundles_it_has() {
    // an engine document of the person's adds a second kind of component but no bundle for it
    let engine = r#"schema = 1
kind = "engine"
id = "llamacpp"

[components.draft]
bind.arg = ["--model-draft", "${component.path}"]
"#;
    let both = alpha().replace(
        "mmproj = {",
        &format!("draft = {{ file = \"draft.gguf\", size = 100, sha256 = \"{D}\" }}\nmmproj = {{"),
    );
    let r = registry(&[
        ("user:engines/llamacpp.toml", engine),
        ("user:catalog/t.toml", &doc("t", &both)),
    ]);
    let found = errors(&r);
    assert!(
        found
            .iter()
            .any(|e| e.contains("no bundle that pairs weights with exactly draft and mmproj")),
        "{found:#?}"
    );
    let hints: Vec<String> = r
        .diagnostics
        .iter()
        .filter_map(|d| d.hint.clone())
        .collect();
    assert!(
        hints.iter().any(|h| h.contains("vision (mmproj)")),
        "{hints:?}"
    );
    assert!(r.catalog().model("alpha-4b").is_none());
}

// ---------------------------------------------------------------------------------------------- J03

#[test]
fn p3_j03_a_persons_documents_patch_hide_and_add_and_a_mistake_leaves_the_shipped_one_in_force() {
    // the embedded layer says the model, the person's layer patches it by id and adds one of its own
    let embedded = ("embedded:catalog/t.toml", doc("t", &alpha()));
    let patch = doc(
        "t",
        "[[model]]\nid = \"alpha-4b\"\nnote = \"Changed by a person.\"\nfeatures = [\"fast\"]\n",
    );
    let mine = doc(
        "mine",
        &format!(
            "[[model]]\nid = \"mine-1b\"\nname = \"Mine 1B\"\n[model.run.ollama]\nref = \"mine:1b\"\n{}",
            ""
        ),
    );
    let r = registry_layers(
        std::slice::from_ref(&embedded),
        &[
            ("user:catalog/t.toml", patch),
            ("user:catalog/mine.toml", mine),
        ],
    );
    assert!(errors(&r).is_empty(), "{:#?}", r.diagnostics);
    let alpha = r.catalog().model("alpha-4b").unwrap();
    assert_eq!(
        alpha.note, "Changed by a person.",
        "a key the patch says is replaced"
    );
    assert_eq!(alpha.features, ["fast"], "and so is a list");
    assert_eq!(alpha.by, "Acme", "what it leaves out stays");
    assert_eq!(
        alpha.runs.len(),
        2,
        "the tables of the model merge key by key"
    );
    assert!(r.catalog().model("mine-1b").is_some());

    // hidden: out of the list, not checked, and no name taken: another document may write it again
    let hide = doc("t", "[[model]]\nid = \"alpha-4b\"\nhidden = true\n");
    let r = registry_layers(
        std::slice::from_ref(&embedded),
        &[("user:catalog/t.toml", hide)],
    );
    assert!(errors(&r).is_empty(), "{:#?}", r.diagnostics);
    assert!(r.catalog().model("alpha-4b").is_none());
    let again = doc("again", &alpha_with_id("alpha-4b"));
    let hide = doc("t", "[[model]]\nid = \"alpha-4b\"\nhidden = true\n");
    let r = registry_layers(
        std::slice::from_ref(&embedded),
        &[
            ("user:catalog/t.toml", hide),
            ("user:catalog/again.toml", again),
        ],
    );
    assert!(errors(&r).is_empty(), "{:#?}", r.diagnostics);
    assert_eq!(r.catalog().model("alpha-4b").unwrap().document, "again");

    // a model two documents both write is refused in the one that comes second, and says where it is
    let dup = doc("zzz", &alpha_with_id("alpha-4b"));
    let r = registry_layers(
        std::slice::from_ref(&embedded),
        &[("user:catalog/zzz.toml", dup)],
    );
    let found = errors(&r);
    assert!(
        found
            .iter()
            .any(|e| e.contains("already written in the catalog `t`")),
        "{found:#?}"
    );
    assert!(
        r.catalog().model("alpha-4b").is_some(),
        "the first document stays"
    );

    // a mistake in the patch leaves the shipped document in force, and the error says so
    let bad = doc("t", "[[model]]\nid = \"alpha-4b\"\ntype = \"nope\"\n");
    let r = registry_layers(&[embedded], &[("user:catalog/t.toml", bad)]);
    assert!(errors(&r).iter().any(|e| e.contains("is not a model type")));
    assert_eq!(
        r.catalog().model("alpha-4b").unwrap().note,
        "A small chat model."
    );
    assert!(
        r.diagnostics.iter().any(|d| d
            .hint
            .as_deref()
            .is_some_and(|h| h.contains("was skipped; the document below it stays in force"))),
        "{:#?}",
        r.diagnostics
    );
}

fn alpha_with_id(id: &str) -> String {
    alpha().replace("id = \"alpha-4b\"", &format!("id = \"{id}\""))
}

/// A registry with the shipped engines and vocabulary, the catalog documents of a test in the
/// embedded layer and those of a person above them.
fn registry_layers(
    embedded: &[(&str, String)],
    user: &[(&str, String)],
) -> quickai_core::registry::Registry {
    use quickai_core::config::{load, Layer, LayerSource, LoadOptions, MemorySource};
    use quickai_core::registry::{KnownPrimitives, Registry};
    let llama = common::manifest("engines/llamacpp.toml");
    let ollama = common::manifest("engines/ollama.toml");
    let vocab = common::manifest("vocab/core.toml");
    let mut docs: Vec<(&str, &str)> = vec![
        ("embedded:engines/llamacpp.toml", &llama),
        ("embedded:engines/ollama.toml", &ollama),
        ("embedded:vocab/core.toml", &vocab),
    ];
    docs.extend(embedded.iter().map(|(n, t)| (*n, t.as_str())));
    let below = MemorySource::new(&docs);
    let udocs: Vec<(&str, &str)> = user.iter().map(|(n, t)| (*n, t.as_str())).collect();
    let above = MemorySource::new(&udocs);
    let layers: Vec<(Layer, &dyn LayerSource)> =
        vec![(Layer::Embedded, &below), (Layer::User, &above)];
    Registry::build(
        load(&layers, &LoadOptions::default()),
        &KnownPrimitives::new(common::PROVIDED),
    )
}

// ---------------------------------------------------------------------------------------------- J04

#[test]
fn p3_j04_an_engine_offers_the_models_that_say_how_it_runs_them() {
    let two = format!(
        "{}\n[[model]]\nid = \"only-ollama\"\n[model.run.ollama]\nref = \"x:1b\"\n\n[[model]]\nid = \"only-files\"\n[model.run.llamacpp]\nrepo = \"acme/Only\"\nfiles = [ {{ file = \"only.gguf\", size = 10 }} ]\n",
        alpha()
    );
    let r = registry(&[("user:catalog/t.toml", &doc("t", &two))]);
    assert!(errors(&r).is_empty(), "{:#?}", r.diagnostics);
    let ids = |engine: &str| -> Vec<String> {
        r.catalog()
            .for_engine(engine)
            .map(|m| m.id.clone())
            .collect()
    };
    assert_eq!(ids("llamacpp"), ["alpha-4b", "only-files"]);
    assert_eq!(ids("ollama"), ["alpha-4b", "only-ollama"]);
    assert!(ids("whisper").is_empty());
    // the entries of an engine are those models, smallest first
    let llama = entries(
        r.catalog(),
        &ctx(&r, "llamacpp", Some("https://huggingface.co")),
    );
    let names: Vec<&str> = llama.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(
        names,
        ["catalog:only-files", "catalog:alpha-4b"],
        "10 bytes before 2.3 GB"
    );
    // without a hub to fetch files from, a model that is files is not offered; a name still is
    let nowhere = entries(r.catalog(), &ctx(&r, "llamacpp", None));
    assert!(nowhere.is_empty());
    let named = entries(r.catalog(), &ctx(&r, "ollama", None));
    assert_eq!(named.len(), 2);
    // the same through the helper the window calls: the hub the engine's own look-up names
    let engine = r.engine("llamacpp").unwrap();
    let helped = recommended(&r, engine);
    assert_eq!(helped.len(), 2);
    let url = &files_of(&helped[1].offers[0]).files[0].url;
    assert!(
        url.starts_with("https://huggingface.co/acme/Alpha-4B-GGUF/resolve/"),
        "{url}"
    );
}

// ---------------------------------------------------------------------------------------------- J05

#[test]
fn p3_j05_a_model_becomes_an_entry_with_a_button_for_every_way_to_get_it() {
    let r = with_alpha();
    let site = Some("https://huggingface.co");
    let list = entries(r.catalog(), &ctx(&r, "llamacpp", site));
    let e: &Entry = &list[0];
    assert_eq!(e.key, "catalog:alpha-4b");
    assert_eq!(e.name, "Alpha 4B");
    assert_eq!(e.description, "A small chat model.");
    assert_eq!(e.model_type.as_deref(), Some("chat"));
    let card = e.card.as_ref().expect("a card");
    assert_eq!(
        (
            card.by.as_str(),
            card.license.as_str(),
            card.params.as_str()
        ),
        ("Acme", "Apache-2.0", "4B")
    );
    assert_eq!(card.context, Some(131072));
    assert_eq!(card.input, ["text", "image"]);
    assert_eq!(card.features, ["thinking", "tools"]);
    assert!(card.start && !card.bundled);

    // what is wanted comes first: the weights with the projector as one download; the weights alone
    // follow, because fetching the projector is the person's choice
    let first_plain = e.offers.iter().position(|o| o.bundle.is_none()).unwrap();
    assert!(
        e.offers[..first_plain]
            .iter()
            .all(|o| o.bundle.as_deref() == Some("vision"))
            && e.offers[first_plain..].iter().all(|o| o.bundle.is_none()),
        "{:?}",
        e.offers
            .iter()
            .map(|o| (&o.bundle, &o.label))
            .collect::<Vec<_>>()
    );
    assert_eq!(first_plain, 2);

    // the weights alone: smallest first, the size of each said, the recommended one marked
    let plain: Vec<&Offer> = e.offers.iter().filter(|o| o.bundle.is_none()).collect();
    assert_eq!(
        plain.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(),
        ["Q4_K_M · 2.3 GB", "Q8_0 · 4.0 GB"]
    );
    assert_eq!(plain[0].bytes, Some(2_500_000_000));
    assert!(plain[0].recommended && !plain[1].recommended);
    assert_eq!(
        (plain[0].class.as_deref(), plain[1].class.as_deref()),
        (Some("small"), Some("medium"))
    );
    assert!(!plain[0].estimated);
    assert!(plain[0]
        .hover
        .contains("Download Alpha-4B-Q4_K_M.gguf (2.3 GB) from huggingface.co/acme/Alpha-4B-GGUF"));
    assert!(plain[0].hover.contains("checked against its SHA-256"));
    assert!(plain[0].hover.contains("the one the list recommends"));
    assert!(!plain[1].hover.contains("recommends"));

    // with the projector: the same buttons, the size of both, one download of two files
    let bundled: Vec<&Offer> = e
        .offers
        .iter()
        .filter(|o| o.bundle.as_deref() == Some("vision"))
        .collect();
    assert_eq!(
        bundled.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(),
        ["Q4_K_M · 3.1 GB", "Q8_0 · 4.7 GB"]
    );
    assert_eq!(bundled[0].bytes, Some(3_300_000_000));
    assert_eq!(
        (bundled[0].class.as_deref(), bundled[1].class.as_deref()),
        (Some("small"), Some("medium"))
    );
    let d = files_of(bundled[0]);
    assert_eq!(d.bundle.as_deref(), Some("vision"));
    assert_eq!(d.source, "huggingface:acme/Alpha-4B-GGUF");
    assert_eq!(d.files.len(), 2);
    assert_eq!(d.files[0].kind, None);
    assert_eq!(d.files[1].kind.as_deref(), Some("mmproj"));
    assert_eq!(d.files[1].label, "projector");
    assert!(bundled[0]
        .hover
        .contains("and the projector mmproj-Alpha-4B-F16.gguf"));
    assert!(bundled[0].hover.contains("(3.1 GB in all)"));
    assert!(bundled[0].hover.contains("checked against its SHA-256"));

    // the name an engine pulls: one button, the size of what is pulled, no bundle
    let named = entries(r.catalog(), &ctx(&r, "ollama", site));
    let o = &named[0].offers;
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].label, "4b · 3.1 GB");
    assert_eq!(o[0].fetch, Fetch::Reference("alpha:4b".to_string()));
    assert_eq!(
        (o[0].bytes, o[0].estimated, o[0].recommended),
        (Some(3_300_000_000), false, true)
    );
    assert_eq!(o[0].class.as_deref(), Some("small"));
    assert!(o[0].hover.starts_with("Pull alpha:4b (3.1 GB)"));
    // a name with no tag says `latest`, and no size says no size
    let r = registry(&[(
        "user:catalog/n.toml",
        &doc(
            "n",
            "[[model]]\nid = \"n\"\n[model.run.ollama]\nref = \"plain\"\n",
        ),
    )]);
    let o = &entries(r.catalog(), &ctx(&r, "ollama", None))[0].offers[0];
    assert_eq!(
        (o.label.as_str(), o.bytes, o.class.clone()),
        ("latest", None, None)
    );
}

#[test]
fn p3_j05b_the_words_of_a_button_are_the_lists_or_the_quantizations_or_the_names() {
    let body = format!(
        r#"[[model]]
id = "w"
[model.run.llamacpp]
repo = "acme/W"
files = [
  {{ file = "w-Q4_K_M.gguf", size = 100, label = "Balanced" }},
  {{ file = "w-Q8_0.gguf", size = 200 }},
  {{ file = "sub/w-IQ4_XS.gguf", size = 150 }},
]

[[model]]
id = "x"
[model.run.llamacpp]
repo = "acme/X"
files = [ {{ file = "x-Q4_K_M.gguf", size = 100 }}, {{ file = "x-v2-Q4_K_M.gguf", size = 120 }}, {{ file = "x.gguf", size = 130 }} ]
{}"#,
        ""
    );
    let r = registry(&[("user:catalog/w.toml", &doc("w", &body))]);
    assert!(errors(&r).is_empty(), "{:#?}", r.diagnostics);
    let list = entries(
        r.catalog(),
        &ctx(&r, "llamacpp", Some("https://h.example/")),
    );
    let by = |key: &str| list.iter().find(|e| e.key == key).unwrap();
    let w: Vec<&str> = by("catalog:w")
        .offers
        .iter()
        .map(|o| o.label.as_str())
        .collect();
    assert_eq!(w, ["Balanced · 100 B", "IQ4_XS · 150 B", "Q8_0 · 200 B"]);
    // two that say the same quantization are told by their names, the third by its name
    let x: Vec<&str> = by("catalog:x")
        .offers
        .iter()
        .map(|o| o.label.as_str())
        .collect();
    assert_eq!(x, ["x-Q4_K_M · 100 B", "x-v2-Q4_K_M · 120 B", "x · 130 B"]);
    // with no `best` the first of the document is the recommended one, wherever its size puts it
    let rec: Vec<bool> = by("catalog:w")
        .offers
        .iter()
        .map(|o| o.recommended)
        .collect();
    assert_eq!(rec, [true, false, false]);
    // a path in a folder: kept by its name, fetched by its path, with the site's slash not doubled
    let sub = &files_of(&by("catalog:w").offers[1]).files[0];
    assert_eq!(sub.store_as, "w-IQ4_XS.gguf");
    assert_eq!(
        sub.url,
        "https://h.example/acme/W/resolve/main/sub/w-IQ4_XS.gguf"
    );
}

// ---------------------------------------------------------------------------------------------- J06

#[test]
fn p3_j06_what_a_button_fetches_is_kept_by_a_name_that_stays_and_carries_what_checks_it() {
    let r = with_alpha();
    let list = entries(
        r.catalog(),
        &ctx(&r, "llamacpp", Some("https://huggingface.co")),
    );
    let e = &list[0];
    for offer in &e.offers {
        let d = files_of(offer);
        for piece in &d.files {
            // every piece has the address of the commit it was checked at, and what it must hash to
            assert!(
                piece.url.contains(&format!("/resolve/{REV}/")),
                "{}",
                piece.url
            );
            assert!(piece.sha256.is_some() && piece.size.is_some());
            assert_eq!(
                quickai_core::lookup::present::safe_file_name(&piece.store_as),
                piece.store_as
            );
        }
        // weights keep the name they have in the repository; a companion is kept by its repository first
        assert_eq!(d.files[0].store_as, d.files[0].name);
        if let Some(p) = d.files.get(1) {
            assert_eq!(p.store_as, "acme--Alpha-4B-GGUF--mmproj-Alpha-4B-F16.gguf");
        }
    }
    // a quantization's weights and the projector are different files with different hashes
    let d = files_of(&e.offers[0]);
    assert_eq!(d.files[0].sha256.as_deref(), Some(A));
    let with = files_of(e.offers.iter().find(|o| o.bundle.is_some()).unwrap());
    assert_eq!(with.files[1].sha256.as_deref(), Some(C));
    // the mode the engine is started in rides on every download of the model, bundle or not
    let embed = format!(
        "[[model]]\nid = \"e\"\ntype = \"embedding\"\n[model.run.llamacpp]\nrepo = \"acme/E\"\nmode = \"embedding\"\nfiles = [ {{ file = \"e-Q8_0.gguf\", size = 9 }} ]\n{}",
        ""
    );
    let r = registry(&[("user:catalog/e.toml", &doc("e", &embed))]);
    assert!(errors(&r).is_empty(), "{:#?}", r.diagnostics);
    let list = entries(
        r.catalog(),
        &ctx(&r, "llamacpp", Some("https://huggingface.co")),
    );
    assert_eq!(
        files_of(&list[0].offers[0]).mode.as_deref(),
        Some("embedding")
    );
    assert_eq!(files_of(&e.offers[0]).mode, None);
}

// ---------------------------------------------------------------------------------------------- J07

/// Six models of different types and sizes, for the tests of browsing.
fn six() -> quickai_core::registry::Registry {
    let model = |id: &str, name: &str, kind: &str, size: u64, extra: &str| {
        format!(
            "[[model]]\nid = \"{id}\"\nname = \"{name}\"\ntype = \"{kind}\"\n{extra}\n[model.run.ollama]\nref = \"{id}:1b\"\nsize = {size}\n\n"
        )
    };
    let gib = 1024u64 * 1024 * 1024;
    let body = [
        model("tiny", "Tiny Talker", "chat", gib / 2, "by = \"Acme\"\nnote = \"Fast everyday answers.\"\nfeatures = [\"fast\"]\nstart = true\n"),
        model("big", "Big Talker", "chat", 8 * gib, "by = \"Acme\"\nnote = \"Better answers.\"\nlicense = \"MIT\"\n"),
        model("coder", "Code Helper", "code", 3 * gib, "by = \"Other Labs\"\nnote = \"Writes and explains programs.\"\nstart = true\n"),
        model("eye", "Eye Model", "vision", 2 * gib, "by = \"Acme\"\nnote = \"Describes pictures.\"\ninput = [\"text\", \"image\"]\nstart = true\n"),
        model("vec", "Vector Maker", "embedding", gib / 4, "by = \"Other Labs\"\nnote = \"For search.\"\n"),
        model("ear", "Ear Model", "audio", gib, "by = \"Acme\"\nnote = \"Transcribes speech.\"\ninput = [\"text\", \"audio\"]\n"),
    ]
    .concat();
    registry(&[("user:catalog/six.toml", &doc("six", &body))])
}

fn browsed(
    list: &[Entry],
    r: &quickai_core::registry::Registry,
    what: &Browse,
) -> (Vec<String>, quickai_core::lookup::view::Counts) {
    let b = browse(list, r.vocabulary().model_types(), what);
    let names = b
        .sections
        .iter()
        .flat_map(|s| s.entries.iter().map(|e| list[e.entry].name.clone()))
        .collect();
    (names, b.counts)
}

#[test]
fn p3_j07_a_list_is_browsed_by_scope_words_and_size_with_counts_for_each_choice() {
    let r = six();
    assert!(errors(&r).is_empty(), "{:#?}", r.diagnostics);
    let list = entries(r.catalog(), &ctx(&r, "ollama", None));
    // smallest first
    assert_eq!(
        list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        [
            "Vector Maker",
            "Tiny Talker",
            "Ear Model",
            "Eye Model",
            "Code Helper",
            "Big Talker"
        ]
    );
    let all = Browse::default();
    let (names, counts) = browsed(&list, &r, &all);
    // sections by the order of the types: chat, code, vision, embedding, audio; inside, smallest first
    assert_eq!(
        names,
        [
            "Tiny Talker",
            "Big Talker",
            "Code Helper",
            "Eye Model",
            "Vector Maker",
            "Ear Model"
        ]
    );
    assert_eq!(counts.all, 6);
    assert_eq!(counts.start, 3);
    assert_eq!(
        counts.types,
        [
            ("chat".to_string(), 2),
            ("code".to_string(), 1),
            ("vision".to_string(), 1),
            ("embedding".to_string(), 1),
            ("audio".to_string(), 1)
        ]
    );

    // "Start here" and a type are scopes
    let start = Browse {
        scope: Scope::Start,
        ..Browse::default()
    };
    assert_eq!(
        browsed(&list, &r, &start).0,
        ["Tiny Talker", "Code Helper", "Eye Model"]
    );
    let chat = Browse {
        scope: Scope::Type("chat".into()),
        ..Browse::default()
    };
    assert_eq!(browsed(&list, &r, &chat).0, ["Tiny Talker", "Big Talker"]);
    // a scope does not change the counts of the others: they say what each would show
    assert_eq!(browsed(&list, &r, &chat).1, counts);

    // every word has to be somewhere: the name, the maker, what it is for, the licence, the features,
    // what it reads, the words of its type; case is ignored
    let words = |w: &str| Browse {
        words: w.into(),
        ..Browse::default()
    };
    assert_eq!(
        browsed(&list, &r, &words("acme")).0,
        ["Tiny Talker", "Big Talker", "Eye Model", "Ear Model"]
    );
    assert_eq!(
        browsed(&list, &r, &words("ACME talker")).0,
        ["Tiny Talker", "Big Talker"]
    );
    assert_eq!(browsed(&list, &r, &words("mit")).0, ["Big Talker"]);
    assert_eq!(browsed(&list, &r, &words("fast")).0, ["Tiny Talker"]);
    assert_eq!(browsed(&list, &r, &words("audio")).0, ["Ear Model"]);
    // a plural finds the singular the list says: what a person types of what they want to look at
    assert_eq!(browsed(&list, &r, &words("images")).0, ["Eye Model"]);
    assert_eq!(
        browsed(&list, &r, &words("ACME talkers")).0,
        ["Tiny Talker", "Big Talker"]
    );
    assert!(browsed(&list, &r, &words("ear modelz")).0.is_empty());
    assert_eq!(browsed(&list, &r, &words("programs")).0, ["Code Helper"]);
    assert_eq!(browsed(&list, &r, &words("embedding")).0, ["Vector Maker"]);
    assert!(browsed(&list, &r, &words("acme nothing")).0.is_empty());
    // the counts follow the words
    let (_, c) = browsed(&list, &r, &words("other labs"));
    assert_eq!((c.all, c.start), (2, 1));
    assert_eq!(
        c.types,
        [("code".to_string(), 1), ("embedding".to_string(), 1)]
    );

    // a size keeps the buttons of its class and the entries that have one
    let small = Browse {
        size: Some("small".into()),
        ..Browse::default()
    };
    let (names, c) = browsed(&list, &r, &small);
    assert_eq!(
        names,
        [
            "Tiny Talker",
            "Code Helper",
            "Eye Model",
            "Vector Maker",
            "Ear Model"
        ]
    );
    assert_eq!(c.all, 5);
    let medium = Browse {
        size: Some("medium".into()),
        ..Browse::default()
    };
    assert_eq!(browsed(&list, &r, &medium).0, ["Big Talker"]);
    let large = Browse {
        size: Some("large".into()),
        ..Browse::default()
    };
    let (names, c) = browsed(&list, &r, &large);
    assert!(names.is_empty() && c.all == 0 && c.types.is_empty());
    // words and size and scope together
    let all_three = Browse {
        scope: Scope::Start,
        words: "acme".into(),
        size: Some("small".into()),
    };
    assert_eq!(
        browsed(&list, &r, &all_three).0,
        ["Tiny Talker", "Eye Model"]
    );
    // a type the vocabulary does not have is shown with the others of no type, as Other
    let mut odd = list.clone();
    odd[0].model_type = Some("nonsense".into());
    let b = browse(&odd, r.vocabulary().model_types(), &Browse::default());
    assert_eq!(b.sections.last().unwrap().type_id, "other");
}

// ---------------------------------------------------------------------------------------------- J10

#[test]
fn p3_j10_the_modes_of_an_engine_are_checked_and_a_persons_document_can_change_them() {
    let r = registry(&[]);
    let llama = r.engine("llamacpp").unwrap();
    let m = modes(llama);
    assert_eq!(
        m.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["embedding", "rerank"]
    );
    assert_eq!(m[0].arg, ["--embeddings"]);
    assert_eq!(m[1].arg, ["--reranking"]);
    assert_eq!(m[0].label, "embedding server");
    assert!(m[0].help.contains("/v1/embeddings"));
    assert!(modes(r.engine("ollama").unwrap()).is_empty());

    // a person's document patches a mode by its id, and adds one
    let patch = r#"schema = 1
kind = "engine"
id = "llamacpp"

[[models.mode]]
id = "embedding"
arg = ["--embeddings", "--pooling", "mean"]

[[models.mode]]
id = "long"
label = "long context"
arg = ["--rope-scaling", "yarn"]
"#;
    let r = registry(&[("user:engines/llamacpp.toml", patch)]);
    assert!(errors(&r).is_empty(), "{:#?}", r.diagnostics);
    let m = modes(r.engine("llamacpp").unwrap());
    assert_eq!(m.len(), 3);
    assert_eq!(m[0].arg, ["--embeddings", "--pooling", "mean"]);
    assert_eq!(
        m[0].label, "embedding server",
        "what the patch leaves out stays"
    );
    assert_eq!(
        (m[2].id.as_str(), m[2].label.as_str()),
        ("long", "long context")
    );

    // what is wrong is refused where it is written, and the shipped document stays in force
    for (bad, expect) in [
        (
            "[[models.mode]]\nid = \"Bad Id\"\narg = [\"--x\"]\n",
            "is not a name for a mode",
        ),
        (
            "[[models.mode]]\nid = \"nothing\"\narg = []\n",
            "adds nothing to the command line",
        ),
        (
            "[[models.mode]]\nid = \"blank\"\narg = [\"--x\", \" \"]\n",
            "an empty word is not an argument",
        ),
    ] {
        let doc = format!("schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n\n{bad}");
        let r = registry(&[("user:engines/llamacpp.toml", &doc)]);
        let found = errors(&r);
        assert!(
            found.iter().any(|e| e.contains(expect)),
            "{expect}: {found:#?}"
        );
        assert_eq!(
            modes(r.engine("llamacpp").unwrap()).len(),
            2,
            "the shipped document stays"
        );
    }
}

// ---------------------------------------------------------------------------------------------- J11

fn facts() -> HostFacts {
    HostFacts {
        os: "linux".into(),
        arch: "x86_64".into(),
        ram_bytes: 32 * 1024 * 1024 * 1024,
        cpus: 8,
        binary: Some("/opt/quickai/llama-server".into()),
        ..Default::default()
    }
}

fn selection() -> Selection {
    Selection::new("llamacpp")
        .with_model(ModelInfo {
            id: "e-Q8_0.gguf".into(),
            display: "e-Q8_0.gguf".into(),
            path: "/models/e-Q8_0.gguf".into(),
            size_bytes: 9,
        })
        .set("llamacpp.port", 8080u16)
        .set("llamacpp.ctx_size", 4096u32)
        .set("llamacpp.n_gpu_layers", 999u32)
}

fn args_of(r: &quickai_core::registry::Registry, s: &Selection) -> Vec<(String, String)> {
    let plan = plan(r, s, &facts()).unwrap_or_else(|e| panic!("{e:#?}"));
    match &plan.units[0].start[0] {
        Start::Spawn(spawn) => spawn
            .args
            .iter()
            .map(|a| (a.value.clone(), a.origin.clone()))
            .collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn p3_j11_the_mode_of_a_model_is_on_the_command_line_after_its_files_and_before_the_persons_own_words(
) {
    let r = registry(&[]);
    let plain = args_of(&r, &selection());
    let words = |a: &[(String, String)]| a.iter().map(|(v, _)| v.clone()).collect::<Vec<_>>();
    // no mode, and the plan is the one it always was
    assert_eq!(args_of(&r, &selection().with_mode("")), plain);
    // a mode the engine does not have adds nothing
    assert_eq!(args_of(&r, &selection().with_mode("telepathy")), plain);
    // the mode's words come after the fields, after a projector and before the extra arguments
    let with = args_of(
        &r,
        &selection()
            .with_component("mmproj", "/models/proj.gguf")
            .with_mode("embedding")
            .set("llamacpp.extra_args", "--flash-attn on"),
    );
    let w = words(&with);
    let at = |needle: &str| {
        w.iter()
            .position(|x| x == needle)
            .unwrap_or_else(|| panic!("{needle} in {w:?}"))
    };
    assert!(at("--n-gpu-layers") < at("--mmproj"));
    assert!(at("--mmproj") < at("--embeddings"));
    assert!(at("--embeddings") < at("--flash-attn"));
    // and the plan says where it came from
    assert_eq!(with[at("--embeddings")].1, "mode embedding");
    // the words of the other mode are another set
    let rerank = args_of(&r, &selection().with_mode("rerank"));
    assert!(words(&rerank).contains(&"--reranking".to_string()));
    assert!(!words(&rerank).contains(&"--embeddings".to_string()));
    // an engine that has no modes is told none
    let ollama_plain = plan(&r, &Selection::new("ollama"), &facts());
    let ollama_mode = plan(
        &r,
        &Selection::new("ollama").with_mode("embedding"),
        &facts(),
    );
    assert_eq!(
        ollama_plain.map(|p| p.units[0].start.len()).ok(),
        ollama_mode.map(|p| p.units[0].start.len()).ok()
    );
}

// ---------------------------------------------------------------------------------------------- J12

#[test]
fn p3_j12_llamacpps_document_says_how_it_serves_embeddings_and_reranking() {
    let r = registry(&[]);
    assert!(
        r.diagnostics.iter().all(|d| d.severity != Severity::Error),
        "{:#?}",
        r.diagnostics
    );
    let m = modes(r.engine("llamacpp").unwrap());
    let embedding = m
        .iter()
        .find(|m| m.id == "embedding")
        .expect("an embedding mode");
    assert_eq!(
        embedding.arg,
        ["--embeddings"],
        "without it /v1/embeddings answers 501"
    );
    let rerank = m.iter().find(|m| m.id == "rerank").expect("a rerank mode");
    assert_eq!(rerank.arg, ["--reranking"]);
    // the vocabulary has the types the list sorts into, in the order of the sidebar
    let order: Vec<&str> =
        quickai_core::lookup::types::in_display_order(r.vocabulary().model_types())
            .iter()
            .map(|t| t.id.as_str())
            .collect();
    assert_eq!(
        order,
        [
            "chat",
            "reasoning",
            "code",
            "translation",
            "vision",
            "documents",
            "embedding",
            "audio",
            "video",
            "image",
            "text_encoder",
            "other"
        ]
    );
}

// ---------------------------------------------------------------------------------------------- J08

#[test]
fn p3_j08_the_list_the_application_ships_is_valid_complete_and_pinned() {
    let r = shipped_registry(&[]);
    let problems: Vec<String> = r
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error | Severity::Warning))
        .map(|d| d.to_string())
        .collect();
    assert!(
        problems.is_empty(),
        "the shipped list does not load clean: {problems:#?}"
    );
    let models = r.catalog().models();
    assert!(models.len() >= 60, "{} models", models.len());
    assert_eq!(
        r.catalog().pages().len(),
        shipped_catalogs().len(),
        "every page is in the list"
    );
    assert_eq!(
        r.catalog().verified(),
        Some("2026-10-07"),
        "every page says when it was checked"
    );

    // every category has something, and the vocabulary's other models are not what the list is for
    // (the video page is ComfyUI's, and holds the models that were run on this machine: docs/comfyui.md;
    // pictures and text encoders are what ComfyUI's look-up sorts into, and the list has none of either)
    for t in r
        .vocabulary()
        .model_types()
        .iter()
        .filter(|t| !["other", "video", "image", "text_encoder"].contains(&t.id.as_str()))
    {
        let n = models.iter().filter(|m| m.kind == t.id).count();
        assert!(n >= 3, "the page `{}` has {n} models", t.id);
    }
    assert!(models.iter().all(|m| m.kind != "other"));

    // a model that does its job through a second file has that file in the list, to be fetched with it as
    // one download: nothing ships that reads pictures or listens and leaves the projector to be found
    for m in models {
        let Some(Run::Files(run)) = m.runs.get("llamacpp") else {
            continue;
        };
        let reads_more = m.input.iter().any(|i| i != "text");
        let made_for_it = ["vision", "documents", "audio"].contains(&m.kind.as_str());
        if reads_more || made_for_it {
            assert!(
                run.companions.iter().any(|(kind, _)| kind == "mmproj"),
                "{} reads {:?} and has no projector in the list",
                m.id,
                m.input
            );
            assert_eq!(run.bundle.as_deref(), Some("vision"), "{}", m.id);
        }
        if !reads_more && !made_for_it {
            assert!(
                run.companions.is_empty(),
                "{} lists a projector it does not use",
                m.id
            );
        }
    }
    // and the choice stays the person's: every model with a companion is offered with it and without it
    for e in recommended(&r, r.engine("llamacpp").unwrap()) {
        let model = r
            .catalog()
            .model(e.key.trim_start_matches("catalog:"))
            .unwrap();
        let Run::Files(run) = &model.runs["llamacpp"] else {
            continue;
        };
        if !run.companions.is_empty() {
            assert!(
                e.offers.iter().any(|o| o.bundle.is_some()),
                "{}: no bundled download",
                e.key
            );
            assert!(
                e.offers.iter().any(|o| o.bundle.is_none()),
                "{}: the weights alone are the person's to choose",
                e.key
            );
            assert!(
                e.offers[0].bundle.is_some(),
                "{}: the bundled download comes first",
                e.key
            );
        }
    }

    let mut start = 0;
    let mut large = 0;
    let sizes = r.vocabulary().size_classes();
    for m in models {
        // what a person reads
        assert!(
            !m.by.is_empty() && !m.license.is_empty() && !m.note.is_empty(),
            "{} says too little",
            m.id
        );
        assert!(
            m.note.chars().count() <= 400,
            "{}: a note is a sentence or two",
            m.id
        );
        assert!(
            m.runs.contains_key("llamacpp")
                || m.runs.contains_key("ollama")
                || m.runs.contains_key("comfyui"),
            "{}",
            m.id
        );
        start += usize::from(m.start);
        for (engine, run) in &m.runs {
            match run {
                // a model ComfyUI runs: pinned the same way, in the formats ComfyUI reads, with the files
                // that go with it fetched as one download into their folders
                Run::Files(run) if engine == "comfyui" => {
                    assert!(
                        run.rev.len() == 40 && run.rev.chars().all(|c| c.is_ascii_hexdigit()),
                        "{}: `{}` is not a commit",
                        m.id,
                        run.rev
                    );
                    for f in run
                        .files
                        .iter()
                        .chain(run.companions.iter().map(|(_, f)| f))
                    {
                        assert!(
                            f.size.is_some_and(|s| s > 0),
                            "{}: {} has no size",
                            m.id,
                            f.path
                        );
                        assert!(
                            f.sha256.as_ref().is_some_and(|h| h.len() == 64),
                            "{}: {} has no hash",
                            m.id,
                            f.path
                        );
                        let lower = f.path.to_lowercase();
                        assert!(
                            lower.ends_with(".gguf") || lower.ends_with(".safetensors"),
                            "{}",
                            f.path
                        );
                    }
                    let kinds: Vec<&str> = run.companions.iter().map(|(k, _)| k.as_str()).collect();
                    assert_eq!(
                        kinds,
                        ["text_encoder", "vae"],
                        "{}: a video model needs both",
                        m.id
                    );
                    assert_eq!(run.bundle.as_deref(), Some("video"), "{}", m.id);
                    assert_eq!(run.mode, None, "{}", m.id);
                    assert_eq!(
                        m.kind, "video",
                        "{}: ComfyUI's page is the video page",
                        m.id
                    );
                }
                Run::Files(run) => {
                    assert_eq!(engine, "llamacpp");
                    // pinned: the commit the files were checked at, and what each file is
                    assert!(
                        run.rev.len() == 40 && run.rev.chars().all(|c| c.is_ascii_hexdigit()),
                        "{}: `{}` is not a commit",
                        m.id,
                        run.rev
                    );
                    for f in run
                        .files
                        .iter()
                        .chain(run.companions.iter().map(|(_, f)| f))
                    {
                        assert!(
                            f.size.is_some_and(|s| s > 0),
                            "{}: {} has no size",
                            m.id,
                            f.path
                        );
                        assert!(
                            f.sha256.as_ref().is_some_and(|h| h.len() == 64),
                            "{}: {} has no hash",
                            m.id,
                            f.path
                        );
                        assert!(f.path.to_lowercase().ends_with(".gguf"), "{}", f.path);
                        assert!(
                            !f.path.contains("-of-"),
                            "{}: a shard is not offered",
                            f.path
                        );
                    }
                    if run.companions.is_empty() {
                        assert!(run.bundle.is_none());
                    } else {
                        assert_eq!(run.bundle.as_deref(), Some("vision"), "{}", m.id);
                    }
                    // an embedding model answers 501 unless it is started in its mode
                    if m.kind == "embedding" {
                        let mode = run.mode.as_deref();
                        assert!(
                            mode == Some("embedding") || mode == Some("rerank"),
                            "{}: an embedding model needs its mode",
                            m.id
                        );
                    } else {
                        assert_eq!(run.mode, None, "{}", m.id);
                    }
                    // the weights the window shows first fit in a band
                    let smallest = run.files.iter().filter_map(|f| f.size).min().unwrap();
                    let class = quickai_core::lookup::types::size_class(sizes, smallest).unwrap();
                    large += usize::from(class.id == "large");
                }
                Run::Reference(run) => {
                    assert_eq!(engine, "ollama");
                    assert!(
                        run.size.is_some_and(|s| s > 0),
                        "{}: {} has no size",
                        m.id,
                        run.reference
                    );
                }
            }
        }
    }
    // a few to start with, one for each need, and a list that is about small and medium models
    assert!((8..=16).contains(&start), "{start} models to start with");
    assert!(
        large * 20 <= models.len(),
        "{large} of {} are large",
        models.len()
    );
    for need in [
        "chat",
        "code",
        "vision",
        "embedding",
        "audio",
        "reasoning",
        "translation",
        "documents",
    ] {
        assert!(
            models.iter().any(|m| m.start && m.kind == need),
            "nothing to start with for `{need}`"
        );
    }

    // what an engine offers of it is every model that says so, each with at least a button in a band
    for engine in ["llamacpp", "ollama"] {
        let e = r.engine(engine).unwrap();
        let list = recommended(&r, e);
        assert_eq!(
            list.len(),
            r.catalog().for_engine(engine).count(),
            "{engine}"
        );
        assert!(list.len() >= 30, "{engine} offers {}", list.len());
        for entry in &list {
            assert!(!entry.offers.is_empty(), "{}", entry.key);
            assert!(
                entry.offers.iter().all(|o| o.class.is_some()),
                "{}: a button with no size",
                entry.key
            );
        }
    }

    // the model that ships with the packages is the one the packaging pins, byte for byte (when the packaging's
    // checkout is beside this one, or this one is inside it)
    let above = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let config = ["quickai_base/build/config.env", "build/config.env"]
        .iter()
        .find_map(|c| std::fs::read_to_string(above.join(c)).ok());
    if let Some(text) = config {
        let pin = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("{key}=")))
                .map(str::to_string)
                .unwrap_or_else(|| panic!("{key} is not in build/config.env"))
        };
        {
            let (id, prefix) = ("bonsai-2-4b", "BONSAI2_4B");
            let m = r.catalog().model(id).unwrap();
            assert!(m.bundled, "{id}");
            let Run::Files(run) = &m.runs["llamacpp"] else {
                panic!()
            };
            assert_eq!(run.files[0].path, pin(&format!("{prefix}_FILE")), "{id}");
            assert_eq!(
                run.files[0].sha256.as_deref(),
                Some(pin(&format!("{prefix}_SHA256")).as_str()),
                "{id}"
            );
            assert!(
                pin(&format!("{prefix}_URL")).contains(&run.rev),
                "{id}: the commit of config.env"
            );
        }
    }
    // and nothing else claims to ship with them
    assert_eq!(models.iter().filter(|m| m.bundled).count(), 1);
}

// ---------------------------------------------------------------------------------------------- J09

#[test]
fn p3_j09_the_reference_page_of_catalog_documents_is_the_one_generated_from_its_types() {
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/reference/catalog-fields.md"),
    )
    .expect("the page is checked in")
    .replace("\r\n", "\n");
    assert_eq!(
        page,
        quickai_core::doc::reference::render_catalog(),
        "run `quickai config docs catalog`"
    );
    // it says every key of every table
    for group in quickai_core::doc::reference::CATALOG_GROUPS {
        for attribute in group.attributes {
            assert!(
                page.contains(&format!("`{}`", attribute.name)),
                "{}",
                attribute.name
            );
        }
    }
}
