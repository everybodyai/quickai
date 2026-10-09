//! Group X of the test plan: what the documents the person reads say about Ollama (work package
//! 3.9), checked against what the application does. Until the generated privacy table of Phase 5
//! exists, these keep the prose, the engine documents and the recordings in step.

use std::path::{Path, PathBuf};

use crate::config::{build_registry, ConfigDirs};
use crate::testkit::recorded;

fn repo_file(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(name)
}

fn text(name: &str) -> String {
    std::fs::read_to_string(repo_file(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn p3_x01_the_privacy_text_says_what_ollama_contacts_what_it_writes_and_when() {
    let privacy = text("PRIVACY.md");
    for needle in [
        "Ollama",
        "registry.ollama.ai",
        "OLLAMA_NO_CLOUD",
        ".ollama/id_ed25519",
        "~/.quickai/ollama",
        "ollama/ollama", // where the releases are looked up
    ] {
        assert!(
            privacy.contains(needle),
            "PRIVACY.md does not mention `{needle}`"
        );
    }
    // the promises that must stay true when a second engine arrives
    assert!(privacy.contains("only when you ask") || privacy.contains("only when you click"));
}

#[test]
fn p3_x07_the_privacy_text_and_the_licences_say_what_the_comfyui_install_contacts_writes_and_is() {
    let privacy = text("PRIVACY.md");
    for needle in [
        "ComfyUI",
        "Comfy-Org/ComfyUI",
        "city96/ComfyUI-GGUF",
        "codeload.github.com",
        "download.pytorch.org",
        "download-r2.pytorch.org",
        "pypi.org",
        "files.pythonhosted.org",
        "~/.quickai/comfyui",
        "--offline",
        ".quickai-install.json",
        "ui-stalls.log",
    ] {
        assert!(
            privacy.contains(needle),
            "PRIVACY.md does not mention `{needle}`"
        );
    }
    // the hosts of the install are the ones the document's own addresses name
    let registry = build_registry(&ConfigDirs::default());
    let engine = registry.engine("comfyui").expect("the ComfyUI document");
    let acquire = engine.doc.acquire.as_ref().expect("an [acquire] section");
    let mut addresses: Vec<String> = acquire
        .install
        .iter()
        .flat_map(|install| match install {
            quickai_core::doc::engine::Install::PythonApp { pip, nodes, .. } => pip
                .iter()
                .filter_map(|step| step.index.clone())
                .chain(nodes.iter().map(|pack| pack.url.clone()))
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    addresses.push(
        quickai_core::acquire::source_archive_url(engine, "v1").expect("the address of a source"),
    );
    for address in addresses {
        let host = address
            .split("//")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .expect("a host");
        assert!(
            privacy.contains(host),
            "the install fetches from {host}, which PRIVACY.md does not name"
        );
    }
    let licences = text("THIRD_PARTY_LICENSES.md");
    let at = licences
        .find("**ComfyUI**")
        .expect("THIRD_PARTY_LICENSES.md does not mention ComfyUI");
    let near = &licences[at..(at + 700).min(licences.len())];
    assert!(near.contains("GPL-3.0"), "no licence: {near}");
    assert!(near.contains("not* part of this package"), "{near}");
}

#[test]
fn p3_x02_the_licences_say_ollama_is_mit_and_is_not_part_of_the_package() {
    let licences = text("THIRD_PARTY_LICENSES.md");
    let at = licences
        .find("Ollama")
        .expect("THIRD_PARTY_LICENSES.md does not mention Ollama");
    let near = &licences[at..(at + 600).min(licences.len())];
    assert!(
        near.contains("MIT"),
        "the Ollama entry does not give its licence: {near}"
    );
    assert!(
        near.to_lowercase().contains("not bundled")
            || near.to_lowercase().contains("not included")
            || near.to_lowercase().contains("downloaded"),
        "the Ollama entry does not say it is not part of the package: {near}"
    );
}

#[test]
fn p3_x04_every_host_an_engine_document_declares_is_named_in_the_privacy_text() {
    let privacy = text("PRIVACY.md");
    let registry = build_registry(&ConfigDirs::default());
    for engine in registry.engines() {
        for host in &engine.doc.engine.egress {
            assert!(
                privacy.contains(host.as_str()),
                "engine `{}` declares that it contacts {host}, which PRIVACY.md does not name",
                engine.id
            );
        }
    }
}

#[test]
fn p3_x05_the_hosts_a_real_pull_contacted_are_declared_by_the_ollama_document() {
    let registry = build_registry(&ConfigDirs::default());
    let engine = registry
        .engine("ollama")
        .expect("the embedded Ollama document");
    let recorded = recorded::load();
    assert!(
        !recorded.egress.is_empty() && !recorded.egress_hf.is_empty(),
        "the recordings of what a real pull contacted are missing"
    );
    for contacted in recorded.egress.iter().chain(&recorded.egress_hf) {
        let host = contacted
            .rsplit_once(':')
            .map_or(contacted.as_str(), |(h, _)| h);
        assert!(
            engine
                .doc
                .engine
                .egress
                .iter()
                .any(|declared| declares(declared, host)),
            "a real pull contacted {host}, which the document does not declare"
        );
    }
    // and nothing is declared that was not seen: the document says only what was measured
    let seen: Vec<&str> = recorded
        .egress
        .iter()
        .chain(&recorded.egress_hf)
        .map(|c| c.rsplit_once(':').map_or(c.as_str(), |(h, _)| h))
        .collect();
    for declared in &engine.doc.engine.egress {
        assert!(
            seen.iter().any(|host| declares(declared, host)),
            "the document declares {declared}, which no recorded pull contacted"
        );
    }
}

#[test]
fn p3_x08_the_hosts_a_real_fetch_of_the_page_contacted_are_declared_by_the_comfyui_document() {
    let registry = build_registry(&ConfigDirs::default());
    let engine = registry
        .engine("comfyui")
        .expect("the embedded ComfyUI document");
    let seen: Vec<String> = recorded::comfyui_egress()
        .iter()
        .map(|c| {
            c.rsplit_once(':')
                .map_or(c.as_str(), |(h, _)| h)
                .to_string()
        })
        .collect();
    assert!(
        !seen.is_empty(),
        "the recording of what ComfyUI contacted is missing"
    );
    for host in &seen {
        assert!(
            engine
                .doc
                .engine
                .egress
                .iter()
                .any(|declared| declares(declared, host)),
            "a real fetch contacted {host}, which the document does not declare"
        );
    }
    // and nothing is declared that was not seen: the document says only what was measured
    for declared in &engine.doc.engine.egress {
        assert!(
            seen.iter().any(|host| declares(declared, host)),
            "the document declares {declared}, which no recorded fetch contacted"
        );
    }
}

/// Whether a declared host covers a contacted one: the same name, or `*.suffix` for any host under it.
fn declares(declared: &str, host: &str) -> bool {
    match declared.strip_prefix("*.") {
        Some(suffix) => host.len() > suffix.len() && host.ends_with(&format!(".{suffix}")),
        None => declared == host,
    }
}

#[test]
fn p3_x06_a_declared_host_with_a_star_covers_the_hosts_under_it_and_only_those() {
    assert!(declares("registry.ollama.ai", "registry.ollama.ai"));
    assert!(!declares("registry.ollama.ai", "ollama.ai"));
    assert!(declares("*.cdn.hf.co", "us.aws.cdn.hf.co"));
    assert!(declares("*.cdn.hf.co", "eu.cdn.hf.co"));
    assert!(
        !declares("*.cdn.hf.co", "cdn.hf.co"),
        "the suffix alone is not under itself"
    );
    assert!(!declares("*.cdn.hf.co", "evilcdn.hf.co"));
    assert!(!declares("*.cdn.hf.co", "hf.co"));
}
