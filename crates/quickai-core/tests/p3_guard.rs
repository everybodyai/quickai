//! The foreign-program guard (work package 3.10, docs/phase3-test-plan.md group G): a document that
//! says `program = ...` is code, so one from the person's own configuration that starts a program the
//! application did not find asks once, and is remembered by what the merged document says.

use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
use quickai_core::guard::{document_hash, judge, Foreign, Verdict};
use quickai_core::registry::{KnownPrimitives, Registry};

const EMBEDDED: &str = r#"schema = 1
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
probe = { use = "http", path = "/health", expect = 200 }

[[field]]
id = "port"
type = "port"
default = 9000
bind.port = "public"

[[field]]
id = "ctx_size"
type = "int"
default = 4096

[[field]]
id = "bin_override"
type = "runtime_path"
"#;

const FOUND: &str = "/opt/acme/bin/acme";

fn registry(user: Option<&str>) -> Registry {
    let embedded = MemorySource::new(&[("embedded:engines/acme.toml", EMBEDDED)]);
    let user_source = MemorySource::new(&[("user:engines/acme.toml", user.unwrap_or(""))]);
    let mut layers: Vec<(Layer, &dyn quickai_core::config::LayerSource)> =
        vec![(Layer::Embedded, &embedded)];
    if user.is_some() {
        layers.push((Layer::User, &user_source));
    }
    let registry = Registry::build(
        load(&layers, &LoadOptions::default()),
        &KnownPrimitives::new(&["spawn@1", "probe.http@1", "locate.setting@1"]),
    );
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    registry
}

/// What the application would say of the programs it knows: the one it found.
fn found(program: &str) -> bool {
    program == FOUND
}

const ENVELOPE: &str = "schema = 1\nkind = \"engine\"\nid = \"acme\"\n";

fn user(body: &str) -> String {
    format!("{ENVELOPE}{body}")
}

#[test]
fn p3_g01_an_embedded_document_never_asks_and_neither_does_a_program_the_application_found() {
    // the application's own document: whatever it starts is its own business
    let registry = registry(None);
    let engine = registry.engine("acme").unwrap();
    assert_eq!(
        judge(engine, "/anywhere/else", &|_| false, &[]),
        Verdict::Trusted
    );

    // a document of the person's that starts what was found, or what is in the installed root
    let registry = registry_with(&user("[launch.spawn]\nargs = [\"serve\", \"--quiet\"]\n"));
    let engine = registry.engine("acme").unwrap();
    assert_eq!(judge(engine, FOUND, &found, &[]), Verdict::Trusted);
    let in_the_root = |p: &str| p.starts_with("/data/acme/");
    assert_eq!(
        judge(engine, "/data/acme/v1/bin/acme", &in_the_root, &[]),
        Verdict::Trusted
    );
}

fn registry_with(user_text: &str) -> Registry {
    registry(Some(user_text))
}

#[test]
fn p3_g02_a_document_of_the_persons_that_starts_another_program_is_refused_with_the_program_and_the_hash_named(
) {
    let text = user("[launch.spawn]\nprogram = \"/tmp/shared/run-me\"\n");
    let registry = registry_with(&text);
    let engine = registry.engine("acme").unwrap();
    let verdict = judge(engine, "/tmp/shared/run-me", &found, &[]);
    let Verdict::NeedsConfirmation(Foreign {
        program,
        hash,
        files,
    }) = verdict
    else {
        panic!("{verdict:?}");
    };
    assert_eq!(program, "/tmp/shared/run-me");
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "{hash}");
    assert_eq!(hash, document_hash(engine));
    assert_eq!(
        files,
        ["user:engines/acme.toml"],
        "the file that makes it foreign is named"
    );
}

#[test]
fn p3_g03_the_hash_the_person_said_yes_to_is_confirmed_and_another_documents_is_not() {
    let text = user("[launch.spawn]\nprogram = \"/tmp/shared/run-me\"\n");
    let registry = registry_with(&text);
    let engine = registry.engine("acme").unwrap();
    let hash = document_hash(engine);
    assert_eq!(
        judge(
            engine,
            "/tmp/shared/run-me",
            &found,
            std::slice::from_ref(&hash)
        ),
        Verdict::Confirmed
    );
    // somebody else's hash, or a hash that is not even one, confirms nothing
    let others = ["0".repeat(64), "not a hash".to_string(), String::new()];
    assert!(matches!(
        judge(engine, "/tmp/shared/run-me", &found, &others),
        Verdict::NeedsConfirmation(_)
    ));
}

#[test]
fn p3_g04_any_change_of_what_the_document_says_changes_the_hash_and_a_change_of_how_it_is_written_does_not(
) {
    let hash = |body: &str| {
        let registry = registry_with(&user(body));
        document_hash(registry.engine("acme").unwrap())
    };
    let base = hash("[launch.spawn]\nprogram = \"/tmp/shared/run-me\"\nargs = [\"serve\"]\n");
    // spacing, comments, a different order of the keys, an inline table for a table: the same document
    assert_eq!(
        base,
        hash("# a note\n[launch.spawn]\n\n  args   = [ \"serve\" ]   # the arguments\nprogram=\"/tmp/shared/run-me\"\n")
    );
    assert_eq!(
        base,
        hash("launch.spawn = { args = [\"serve\"], program = \"/tmp/shared/run-me\" }\n")
    );
    // another program, an argument more or less, another value anywhere
    assert_ne!(
        base,
        hash("[launch.spawn]\nprogram = \"/tmp/shared/other\"\nargs = [\"serve\"]\n")
    );
    assert_ne!(
        base,
        hash("[launch.spawn]\nprogram = \"/tmp/shared/run-me\"\nargs = [\"serve\", \"-v\"]\n")
    );
    assert_ne!(
        base,
        hash("[launch.spawn]\nprogram = \"/tmp/shared/run-me\"\nargs = [\"serve\"]\n\n[[field]]\nid = \"ctx_size\"\ndefault = 8192\n")
    );
    // and the same text that says the same thing differently quoted is the same thing
    assert_eq!(
        hash("[launch.spawn]\nprogram = 'C:\\tools\\run-me'\n"),
        hash("[launch.spawn]\nprogram = \"C:\\\\tools\\\\run-me\"\n")
    );
}

#[test]
fn p3_g05_a_patch_that_changes_only_a_default_does_not_ask() {
    let registry = registry_with(&user("[[field]]\nid = \"ctx_size\"\ndefault = 8192\n"));
    let engine = registry.engine("acme").unwrap();
    // the program is still the one the application's own strategies find
    assert_eq!(judge(engine, FOUND, &found, &[]), Verdict::Trusted);
    // but it is a different document from the embedded one, which is what a later change of program
    // would be remembered against
    let plain = self::registry(None);
    assert_ne!(
        document_hash(engine),
        document_hash(plain.engine("acme").unwrap())
    );
}

#[test]
fn p3_g06_a_program_that_was_declined_is_asked_about_again_and_the_guard_remembers_nothing() {
    let registry = registry_with(&user("[launch.spawn]\nprogram = \"/tmp/shared/run-me\"\n"));
    let engine = registry.engine("acme").unwrap();
    for _ in 0..3 {
        assert!(matches!(
            judge(engine, "/tmp/shared/run-me", &found, &[]),
            Verdict::NeedsConfirmation(_)
        ));
    }
}
