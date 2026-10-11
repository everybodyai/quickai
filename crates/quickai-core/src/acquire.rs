//! The pure half of acquiring a build (docs/architecture.md, sections 9.2 and 11.5): which of a
//! release's files is for this machine, what to call the directory a build is installed under,
//! what a file name says about the GPU backend it was made for. Downloading and unpacking are the
//! application's; everything here is a function of the document and the names it is given.

use crate::doc::engine::{Assets, Backend};
use crate::expr::eval;
use crate::plan::resolve::base_context;
use crate::plan::{HostFacts, Selection};
use crate::registry::Engine;
use crate::value::{map, Value};

/// File endings an installer can unpack, longest first so `.tar.gz` is not mistaken for `.gz`.
const ARCHIVE_ENDINGS: [&str; 4] = [".tar.zst", ".tar.gz", ".tgz", ".zip"];

/// `llama-b1-bin-win-vulkan-x64.zip` without its ending.
pub fn archive_stem(name: &str) -> &str {
    ARCHIVE_ENDINGS
        .iter()
        .find_map(|ending| name.strip_suffix(ending))
        .unwrap_or(name)
}

/// The positions in `names` of the files that are a build for this machine, by the engine's
/// `[acquire.assets]` rule. An engine with no rule has none.
pub fn matching_assets(engine: &Engine, names: &[&str], facts: &HostFacts) -> Vec<usize> {
    let Some(assets) = engine.doc.acquire.as_ref().and_then(|a| a.assets.as_ref()) else {
        return Vec::new();
    };
    match assets {
        Assets::Keywords {
            ext,
            exclude,
            os,
            arch,
            ..
        } => {
            let words = |table: &std::collections::BTreeMap<String, Vec<String>>, key: &str| {
                table.get(key).cloned().unwrap_or_default()
            };
            let (os_words, arch_words) = (words(os, &facts.os), words(arch, &facts.arch));
            let any_in = |haystack: &str, needles: &[String]| {
                needles
                    .iter()
                    .any(|n| haystack.contains(&n.to_ascii_lowercase()))
            };
            names
                .iter()
                .enumerate()
                .filter(|(_, name)| {
                    let lower = name.to_ascii_lowercase();
                    ext.iter().any(|e| lower.ends_with(&e.to_ascii_lowercase()))
                        && !any_in(&lower, exclude)
                        && any_in(&lower, &os_words)
                        && any_in(&lower, &arch_words)
                })
                .map(|(i, _)| i)
                .collect()
        }
        // the release's source is not one of its files: the feed makes that build itself
        // the build of these is not a file picked out of a release's files: the feed names it
        Assets::Source { .. } | Assets::Wheel { .. } => Vec::new(),
        Assets::Names { .. } => {
            let Some(compiled) = engine.acquire.as_ref() else {
                return Vec::new();
            };
            let context = base_context(&Selection::default(), facts);
            let chosen = compiled.name_rules.iter().find_map(|(when, name)| {
                eval(when, &context)
                    .ok()
                    .filter(Value::truthy)
                    .map(|_| name.as_str())
            });
            match chosen {
                Some(chosen) => names
                    .iter()
                    .enumerate()
                    .filter(|(_, name)| **name == chosen)
                    .map(|(i, _)| i)
                    .collect(),
                None => Vec::new(),
            }
        }
    }
}

/// Whether a build of this engine is the source of a release (`assets` of `by = "source"`) or its
/// wheel (`by = "wheel"`): an application that is installed, not a program that is unpacked, so a
/// build has no file name that says which graphics card it is for.
pub fn builds_from_source(engine: &Engine) -> bool {
    matches!(
        engine.doc.acquire.as_ref().and_then(|a| a.assets.as_ref()),
        Some(Assets::Source { .. } | Assets::Wheel { .. })
    )
}

/// The name of a release's source archive, which is also what its pin is filed under: `v0.39.0.tar.gz`.
pub fn source_archive_name(tag: &str) -> String {
    format!("{tag}.tar.gz")
}

/// Where the source archive of release `tag` is fetched from: the feed's repository on the host the
/// feed's `archive` names (`https://github.com` by default). `None` for an engine with no such feed.
pub fn source_archive_url(engine: &Engine, tag: &str) -> Option<String> {
    match engine.doc.acquire.as_ref()?.feed.as_ref()? {
        crate::doc::engine::Feed::GithubRelease { repo, archive, .. } => {
            let root = archive.as_deref().unwrap_or("https://github.com");
            Some(format!(
                "{}/{repo}/archive/refs/tags/{tag}.tar.gz",
                root.trim_end_matches('/')
            ))
        }
        // a wheel's address is not made up from its version: the index says where it is
        crate::doc::engine::Feed::Pypi { .. } => None,
    }
}

/// The globs an install leaves out on this machine: the `[acquire.install]` `skip` rules whose
/// condition holds (or that have none), in the document's order.
pub fn skip_globs(engine: &Engine, facts: &HostFacts) -> Vec<String> {
    let Some(compiled) = engine.acquire.as_ref() else {
        return Vec::new();
    };
    let context = base_context(&Selection::default(), facts);
    compiled
        .skip_rules
        .iter()
        .filter(|(when, _)| match when {
            None => true,
            // a condition that cannot be evaluated does not hold: nothing is left out on a guess
            Some(when) => eval(when, &context).is_ok_and(|v| v.truthy()),
        })
        .map(|(_, glob)| glob.clone())
        .collect()
}

/// The pip steps of a `python_app` install that apply to this machine, in the document's order: those
/// with no condition and those whose condition holds. A condition that cannot be evaluated does not
/// hold: nothing is installed on a guess.
pub fn pip_steps<'a>(
    engine: &'a Engine,
    facts: &HostFacts,
) -> Vec<&'a crate::doc::engine::PipStep> {
    let (Some(acquire), Some(compiled)) = (engine.doc.acquire.as_ref(), engine.acquire.as_ref())
    else {
        return Vec::new();
    };
    let Some(crate::doc::engine::Install::PythonApp { pip, .. }) = acquire.install.as_ref() else {
        return Vec::new();
    };
    let context = base_context(&Selection::default(), facts);
    pip.iter()
        .zip(compiled.pip_when.iter().chain(std::iter::repeat(&None)))
        .filter(|(step, when)| step.when.is_none() || holds(when, &context))
        .map(|(step, _)| step)
        .collect()
}

/// The node packs of a `python_app` install that apply to this machine, in the document's order.
pub fn node_packs<'a>(
    engine: &'a Engine,
    facts: &HostFacts,
) -> Vec<&'a crate::doc::engine::NodePack> {
    let (Some(acquire), Some(compiled)) = (engine.doc.acquire.as_ref(), engine.acquire.as_ref())
    else {
        return Vec::new();
    };
    let Some(crate::doc::engine::Install::PythonApp { nodes, .. }) = acquire.install.as_ref()
    else {
        return Vec::new();
    };
    let context = base_context(&Selection::default(), facts);
    nodes
        .iter()
        .zip(compiled.node_when.iter().chain(std::iter::repeat(&None)))
        .filter(|(pack, when)| pack.when.is_none() || holds(when, &context))
        .map(|(pack, _)| pack)
        .collect()
}

/// Whether a parsed condition holds in `context`; one that is absent (it did not parse) or cannot be
/// evaluated does not.
fn holds(when: &Option<crate::expr::Expr>, context: &Value) -> bool {
    when.as_ref()
        .is_some_and(|expr| eval(expr, context).is_ok_and(|v| v.truthy()))
}

/// What installing a build of a `python_app` engine puts on this machine, in a sentence for the window:
/// `Installs a Python environment with PyTorch for AMD GPUs (ROCm 7.2), what ComfyUI needs and
/// ComfyUI-GGUF; it needs 30 GiB free.` `None` for an engine that unpacks an archive.
pub fn install_summary(engine: &Engine, facts: &HostFacts) -> Option<String> {
    let Some(crate::doc::engine::Install::PythonApp { space, .. }) =
        engine.doc.acquire.as_ref()?.install.as_ref()
    else {
        return None;
    };
    let mut parts: Vec<String> = pip_steps(engine, facts)
        .into_iter()
        .map(|step| step.label.clone().unwrap_or_else(|| step.id.clone()))
        .collect();
    parts.extend(
        node_packs(engine, facts)
            .into_iter()
            .map(|pack| pack.label.clone().unwrap_or_else(|| pack.id.clone())),
    );
    let list = match parts.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    };
    let room = space
        .as_deref()
        .map(|s| format!("; it needs {s} free"))
        .unwrap_or_default();
    Some(if list.is_empty() {
        format!("Installs a Python environment{room}.")
    } else {
        format!("Installs a Python environment with {list}{room}.")
    })
}

/// An overlay this machine has, and the file that is it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayChoice {
    pub id: String,
    pub label: String,
    pub asset_name: String,
}

/// The overlays of the document that exist for this machine (the first of each one's rules whose
/// condition holds names its file), in the document's order. A platform no rule matches has none.
pub fn overlay_choices(engine: &Engine, facts: &HostFacts) -> Vec<OverlayChoice> {
    let Some(compiled) = engine.acquire.as_ref() else {
        return Vec::new();
    };
    let context = base_context(&Selection::default(), facts);
    compiled
        .overlay_rules
        .iter()
        .filter_map(|overlay| {
            let name = overlay.rules.iter().find_map(|(when, name)| {
                eval(when, &context)
                    .ok()
                    .filter(Value::truthy)
                    .map(|_| name.clone())
            })?;
            Some(OverlayChoice {
                id: overlay.id.clone(),
                label: overlay.label.clone(),
                asset_name: name,
            })
        })
        .collect()
}

/// Fills a path template (`${data}/ollama`) in from what is known of this machine: how a locate
/// strategy and the installer learn where an engine's builds are. `None` for a template that does
/// not parse or cannot be filled.
pub fn render_template(text: &str, facts: &HostFacts) -> Option<String> {
    let template = crate::expr::Template::parse(text).ok()?;
    let context = base_context(&Selection::default(), facts);
    template.render(&context).ok().map(|v| v.to_string())
}

/// The release the document was tested with.
pub fn tested_tag(engine: &Engine) -> Option<&str> {
    engine
        .doc
        .acquire
        .as_ref()?
        .tested
        .as_ref()
        .map(|t| t.tag.as_str())
}

/// The sha256 (hex, lower case) the document pins for a file of the tested release.
pub fn pin_for(engine: &Engine, asset_name: &str) -> Option<String> {
    let pin = engine.doc.acquire.as_ref()?.pins.get(asset_name)?;
    Some(
        pin.trim()
            .trim_start_matches("sha256:")
            .to_ascii_lowercase(),
    )
}

/// The directory name a build of `asset_name` from release `tag` is installed under.
pub fn install_label(engine: &Engine, tag: &str, asset_name: &str) -> String {
    let stem = archive_stem(asset_name);
    let fallback = || format!("{tag}-{stem}");
    let Some(compiled) = engine.acquire.as_ref() else {
        return fallback();
    };
    let context = map([
        ("tag", tag.into()),
        ("stem", stem.into()),
        ("asset", asset_name.into()),
    ]);
    compiled
        .label
        .render(&context)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| fallback())
}

/// A directory name for a build downloaded from a bare URL: the file name without its ending, or
/// `fallback` for a URL with no file-name-shaped last segment.
pub fn label_from_url(url: &str, fallback: &str) -> String {
    let name = url.trim_end_matches('/').rsplit('/').next().unwrap_or(url);
    let stem = archive_stem(name);
    if stem.is_empty() {
        fallback.to_string()
    } else {
        stem.to_string()
    }
}

/// The first backend rule whose word is in `asset_name`.
pub fn backend<'a>(engine: &'a Engine, asset_name: &str) -> Option<&'a Backend> {
    let lower = asset_name.to_ascii_lowercase();
    engine
        .doc
        .acquire
        .as_ref()?
        .backends
        .iter()
        .find(|b| lower.contains(&b.needle.to_ascii_lowercase()))
}

/// What the interface calls the backend a file was built for, or `default` when its name says
/// nothing (CPU and Vulkan builds are often just left unlabelled).
pub fn backend_label<'a>(engine: &'a Engine, asset_name: &str) -> &'a str {
    backend(engine, asset_name).map_or("default", |b| b.label.as_str())
}

/// The backends in a form for a sentence: the labels, each cut at its first `/`, joined with `/`
/// (`CUDA/Vulkan/SYCL/ROCm/CPU`), each once.
pub fn backend_words(engine: &Engine) -> String {
    let mut words: Vec<&str> = Vec::new();
    for backend in engine.doc.acquire.iter().flat_map(|a| a.backends.iter()) {
        let word = backend.label.split('/').next().unwrap_or_default();
        if !words.contains(&word) {
            words.push(word);
        }
    }
    words.join("/")
}

/// `github.com/ggml-org/llama.cpp`: the feed's repository as a sentence names it.
pub fn feed_repo_label(engine: &Engine) -> Option<String> {
    match engine.doc.acquire.as_ref()?.feed.as_ref()? {
        crate::doc::engine::Feed::GithubRelease { repo, .. } => Some(format!("github.com/{repo}")),
        crate::doc::engine::Feed::Pypi { package, .. } => {
            Some(format!("pypi.org/project/{package}"))
        }
    }
}

/// Where installed builds live, as the document says (`${data}/llama-bin`, with the data directory
/// filled in), or `None` when it names no root.
pub fn root_dir(engine: &Engine, facts: &HostFacts) -> Option<String> {
    let root = engine.acquire.as_ref()?.root.as_ref()?;
    let context = base_context(&Selection::default(), facts);
    root.render(&context).ok().map(|v| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::fixtures::{registry_from, GOOD};

    /// llama.cpp's section, as the embedded document writes it.
    const LLAMA: &str = r#"
[acquire]
root = "${data}/llama-bin"
feed = { use = "github_release", repo = "ggml-org/llama.cpp", per_page = 10, show = 5, prereleases = true }
assets = { by = "keywords", ext = [".zip", ".tar.gz"], exclude = ["cudart"], os = { windows = ["win"], macos = ["macos", "osx"], linux = ["ubuntu", "linux"] }, arch = { x86_64 = ["x64", "x86_64", "amd64"], aarch64 = ["arm64", "aarch64"] }, label = "${tag}-${stem}" }
install = { use = "archive", binary = { name = "llama-server", search_depth = 4 }, chmod = true }
filter_hint = "e.g. vulkan, rocm, b110..."
backends = [
  { match = "cuda", label = "CUDA", needs_gpu = "nvidia" },
  { match = "vulkan", label = "Vulkan" },
  { match = "sycl", label = "SYCL" },
  { match = "hip", label = "ROCm/HIP", needs_gpu = "amd" },
  { match = "rocm", label = "ROCm/HIP", needs_gpu = "amd" },
  { match = "cpu", label = "CPU" },
]
"#;

    /// Ollama's rules, as the example document writes them.
    const OLLAMA: &str = r#"
[acquire]
root = "${data}/ollama"
feed = { use = "github_release", repo = "ollama/ollama" }
assets = { by = "names", rules = [
  { when = "os == 'linux' and arch == 'x86_64'", name = "ollama-linux-amd64.tar.zst" },
  { when = "os == 'linux' and arch == 'aarch64'", name = "ollama-linux-arm64.tar.zst" },
  { when = "os == 'windows' and arch == 'x86_64'", name = "ollama-windows-amd64.zip" },
  { when = "os == 'macos'", name = "ollama-darwin.tgz" },
] }
install = { use = "archive", binary = { name = "ollama" } }
"#;

    fn engine_with(section: &str) -> Registry {
        let r = registry_from(&format!("{GOOD}{section}"));
        assert!(r.diagnostics.is_empty(), "{:#?}", r.diagnostics);
        r
    }

    use crate::registry::Registry;

    fn facts(os: &str, arch: &str) -> HostFacts {
        HostFacts {
            os: os.into(),
            arch: arch.into(),
            data_dir: "/home/u/.quickai".into(),
            ..Default::default()
        }
    }

    fn pick(registry: &Registry, names: &[&str], os: &str, arch: &str) -> Vec<String> {
        let engine = registry.engine("llamacpp").unwrap();
        matching_assets(engine, names, &facts(os, arch))
            .into_iter()
            .map(|i| names[i].to_string())
            .collect()
    }

    #[test]
    fn a_file_needs_a_word_for_the_os_and_one_for_the_cpu() {
        let r = engine_with(LLAMA);
        let release = [
            "llama-b1-bin-win-vulkan-x64.zip",
            "llama-b1-bin-win-vulkan-arm64.zip",
            "llama-b1-bin-win-cpu-x64.rar",
            "llama-b1-bin-macos-arm64.zip",
            "llama-b1-bin-ubuntu-vulkan-x64.tar.gz",
            "cudart-llama-b1-bin-win-cuda-12.4-x64.zip",
            "llama-b1-xcframework.zip",
        ];
        assert_eq!(
            pick(&r, &release, "windows", "x86_64"),
            ["llama-b1-bin-win-vulkan-x64.zip"],
            "the arm64 build, the .rar and the CUDA runtime package are not for this machine"
        );
        assert_eq!(
            pick(&r, &release, "windows", "aarch64"),
            ["llama-b1-bin-win-vulkan-arm64.zip"]
        );
        assert_eq!(
            pick(&r, &release, "macos", "aarch64"),
            ["llama-b1-bin-macos-arm64.zip"]
        );
        // .tar.gz is accepted next to .zip: llama.cpp ships Linux and macOS as tarballs
        assert_eq!(
            pick(&r, &release, "linux", "x86_64"),
            ["llama-b1-bin-ubuntu-vulkan-x64.tar.gz"]
        );
        assert!(pick(&r, &release, "freebsd", "x86_64").is_empty());
        assert!(pick(&r, &release, "linux", "riscv64").is_empty());
    }

    #[test]
    fn matching_ignores_case() {
        let r = engine_with(LLAMA);
        assert_eq!(
            pick(&r, &["LLAMA-B1-BIN-WIN-CPU-X64.ZIP"], "windows", "x86_64").len(),
            1
        );
    }

    #[test]
    fn a_name_rule_picks_the_one_file_whose_condition_holds() {
        let r = engine_with(OLLAMA);
        let release = [
            "ollama-linux-amd64.tar.zst",
            "ollama-linux-arm64.tar.zst",
            "ollama-linux-amd64-rocm.tar.zst",
            "ollama-windows-amd64.zip",
            "ollama-darwin.tgz",
            "sha256sum.txt",
        ];
        assert_eq!(
            pick(&r, &release, "linux", "x86_64"),
            ["ollama-linux-amd64.tar.zst"]
        );
        assert_eq!(
            pick(&r, &release, "linux", "aarch64"),
            ["ollama-linux-arm64.tar.zst"]
        );
        assert_eq!(
            pick(&r, &release, "windows", "x86_64"),
            ["ollama-windows-amd64.zip"]
        );
        assert_eq!(
            pick(&r, &release, "macos", "aarch64"),
            ["ollama-darwin.tgz"]
        );
        assert!(
            pick(&r, &release, "windows", "aarch64").is_empty(),
            "no rule, no build"
        );
        assert!(
            pick(&r, &["ollama-other.zip"], "linux", "x86_64").is_empty(),
            "a rule that names a file the release lacks selects nothing"
        );
    }

    #[test]
    fn an_engine_with_no_rule_has_no_builds() {
        let r = registry_from(GOOD);
        let engine = r.engine("llamacpp").unwrap();
        assert!(matching_assets(engine, &["a.zip"], &facts("linux", "x86_64")).is_empty());
    }

    #[test]
    fn a_build_is_installed_under_its_release_and_file_name() {
        let r = engine_with(LLAMA);
        let e = r.engine("llamacpp").unwrap();
        assert_eq!(
            install_label(e, "b11007", "llama-b11007-bin-win-vulkan-x64.zip"),
            "b11007-llama-b11007-bin-win-vulkan-x64"
        );
        assert_eq!(
            install_label(e, "b11026", "llama-b11026-bin-ubuntu-vulkan-x64.tar.gz"),
            "b11026-llama-b11026-bin-ubuntu-vulkan-x64"
        );
        assert_eq!(
            install_label(e, "v0.35.1", "ollama-linux-amd64.tar.zst"),
            "v0.35.1-ollama-linux-amd64"
        );
        // a label that names other parts of the file
        let custom = engine_with(&LLAMA.replace("${tag}-${stem}", "${asset}@${tag}"));
        assert_eq!(
            install_label(custom.engine("llamacpp").unwrap(), "b1", "x.zip"),
            "x.zip@b1"
        );
        // with no [acquire] at all the default still applies
        let none = registry_from(GOOD);
        assert_eq!(
            install_label(none.engine("llamacpp").unwrap(), "b1", "x.tar.gz"),
            "b1-x"
        );
    }

    #[test]
    fn a_label_from_a_url_is_the_file_name_without_its_ending() {
        let fallback = "llama-server-build";
        assert_eq!(
            label_from_url(
                "https://example.com/x/llama-b1-bin-ubuntu-vulkan-x64.tar.gz",
                fallback
            ),
            "llama-b1-bin-ubuntu-vulkan-x64"
        );
        assert_eq!(
            label_from_url(
                "https://example.com/x/llama-b1-bin-win-vulkan-x64.zip",
                fallback
            ),
            "llama-b1-bin-win-vulkan-x64"
        );
        assert_eq!(
            label_from_url("https://example.com/download", fallback),
            "download"
        );
        assert_eq!(
            label_from_url("https://example.com/download/", fallback),
            "download"
        );
        assert_eq!(
            label_from_url("https://example.com/.tar.gz", fallback),
            fallback
        );
        assert_eq!(
            label_from_url("https://example.com/x.tar.zst", fallback),
            "x"
        );
    }

    #[test]
    fn archive_endings_are_stripped_longest_first() {
        assert_eq!(archive_stem("a.tar.gz"), "a");
        assert_eq!(archive_stem("a.tgz"), "a");
        assert_eq!(archive_stem("a.tar.zst"), "a");
        assert_eq!(archive_stem("a.zip"), "a");
        assert_eq!(archive_stem("a.gz"), "a.gz");
        assert_eq!(archive_stem("plain"), "plain");
    }

    #[test]
    fn backends_are_named_by_the_first_matching_word() {
        let r = engine_with(LLAMA);
        let e = r.engine("llamacpp").unwrap();
        for (name, label) in [
            ("llama-b11007-bin-win-cuda-cu12.4-x64.zip", "CUDA"),
            ("llama-b11007-bin-win-vulkan-x64.zip", "Vulkan"),
            ("llama-b11007-bin-win-sycl-x64.zip", "SYCL"),
            ("llama-b11007-bin-win-hip-x64.zip", "ROCm/HIP"),
            ("llama-b11007-bin-ubuntu-rocm-7.0-x64.tar.gz", "ROCm/HIP"),
            ("llama-b11007-bin-win-cpu-x64.zip", "CPU"),
            ("llama-b11007-bin-macos-arm64.zip", "default"),
            ("LLAMA-B1-BIN-WIN-CUDA-X64.ZIP", "CUDA"),
        ] {
            assert_eq!(backend_label(e, name), label, "{name}");
        }
        assert_eq!(
            backend(e, "llama-b1-bin-win-cuda-x64.zip").and_then(|b| b.needs_gpu.as_deref()),
            Some("nvidia")
        );
        assert_eq!(
            backend(e, "llama-b1-bin-win-vulkan-x64.zip")
                .unwrap()
                .needs_gpu,
            None
        );
        assert!(backend(e, "llama-b1-bin-macos-arm64.zip").is_none());
    }

    #[test]
    fn the_backends_read_as_a_phrase() {
        let r = engine_with(LLAMA);
        assert_eq!(
            backend_words(r.engine("llamacpp").unwrap()),
            "CUDA/Vulkan/SYCL/ROCm/CPU"
        );
        let none = registry_from(GOOD);
        assert_eq!(backend_words(none.engine("llamacpp").unwrap()), "");
    }

    #[test]
    fn the_feeds_repository_reads_as_a_place() {
        let r = engine_with(LLAMA);
        assert_eq!(
            feed_repo_label(r.engine("llamacpp").unwrap()).as_deref(),
            Some("github.com/ggml-org/llama.cpp")
        );
        let none = registry_from(GOOD);
        assert_eq!(feed_repo_label(none.engine("llamacpp").unwrap()), None);
    }

    #[test]
    fn the_root_is_the_documents_with_the_data_directory_filled_in() {
        let r = engine_with(LLAMA);
        assert_eq!(
            root_dir(r.engine("llamacpp").unwrap(), &facts("linux", "x86_64")).as_deref(),
            Some("/home/u/.quickai/llama-bin")
        );
        let none = registry_from(GOOD);
        assert_eq!(
            root_dir(none.engine("llamacpp").unwrap(), &facts("linux", "x86_64")),
            None
        );
    }
}
