//! Group Y of docs/phase3-test-plan.md, the pure half of WP4.20: what the computer has to have for a
//! configuration to work (docs/headless.md, section 12). The engine's document says what can be looked at, how,
//! and which packages bring it; a configuration says which of them it needs; what applies to a computer is a
//! function of the documents and the facts of the computer. Looking at the computer and installing are the
//! runtime's (`host.rs`, `provision/prerequisites.rs`).

mod common;

use quickai_core::diag::Severity;
use quickai_core::doc::engine::{PackageManager, PrerequisiteCheck};
use quickai_core::plan::HostFacts;
use quickai_core::prerequisite::{packages, wanted};
use quickai_core::profile::{Configuration, NeedKind};
use quickai_core::registry::Registry;

const VERSION: &str = "0.1.0";
const USER_ENGINE: &str = "/home/u/.config/quickai/engines/comfyui.toml";
const USER_PROFILE: &str = "/home/u/.config/quickai/profiles/given.toml";

fn facts(os: &str, vendors: &[&str]) -> HostFacts {
    HostFacts {
        os: os.into(),
        arch: "x86_64".into(),
        ram_bytes: 64 << 30,
        gpu_known: !vendors.is_empty(),
        gpu_vendors: vendors.iter().map(|v| v.to_string()).collect(),
        ..HostFacts::default()
    }
}

/// The shipped ComfyUI document with the part that names the prerequisites replaced by `prerequisites`, standing
/// in for the shipped one: a document that is a real `python_app` engine and says only what a test wants.
fn comfyui_with(prerequisites: &str) -> String {
    let text = common::manifest("engines/comfyui.toml");
    let start = text
        .find("# ---- what the computer has to have")
        .expect("the document has a section for the prerequisites");
    let end = text
        .find("# ---- advice for a run that went wrong")
        .expect("and one for the advice");
    let whole = format!("{}{}\n{}", &text[..start], prerequisites, &text[end..]);
    whole.replacen(
        "id = \"comfyui\"\n",
        "id = \"comfyui\"\nreplace = true\n",
        1,
    )
}

fn registry_with(prerequisites: &str) -> Registry {
    common::shipped_registry(&[(USER_ENGINE, &comfyui_with(prerequisites))])
}

fn errors(registry: &Registry) -> Vec<String> {
    common::texts(registry, Severity::Error)
}

/// The errors that keep a document with these prerequisites from being used.
fn refused(prerequisites: &str) -> String {
    let r = registry_with(prerequisites);
    assert!(
        r.engine("comfyui")
            .is_none_or(|e| e.doc.prerequisites.is_empty()),
        "the engine was used, and should not have been:\n{prerequisites}"
    );
    let found = errors(&r).join("\n");
    assert!(!found.is_empty(), "no error for\n{prerequisites}");
    found
}

fn profile(body: &str) -> String {
    format!(
        "schema = 1\nkind = \"profile\"\nid = \"given\"\nname = \"Given\"\nengine = \"comfyui\"\n{body}"
    )
}

fn read(body: &str) -> (Configuration, Registry) {
    let text = profile(body);
    let r = common::shipped_registry(&[(USER_PROFILE, &text)]);
    let c = r
        .profile("given")
        .unwrap_or_else(|| panic!("not read: {:?}\n{text}", errors(&r)))
        .configuration
        .clone();
    (c, r)
}

/// Y79: the shipped document of ComfyUI names what H3 needed to run on a rented server (the venv module of Python,
/// a compiler, the headers of Python, a driver of the 580 series, and a test that Triton builds its helper), each with a
/// kind of check the program does, and the packages only for what a package manager can bring.
#[test]
fn p3_y79_the_comfyui_document_names_what_the_computer_has_to_have() {
    let r = common::shipped_registry(&[]);
    assert!(errors(&r).is_empty(), "{:?}", errors(&r));
    let engine = r.engine("comfyui").unwrap();
    let ids: Vec<&str> = engine
        .doc
        .prerequisites
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "python_venv",
            "c_compiler",
            "python_headers",
            "nvidia_driver",
            "triton"
        ]
    );
    let by = |id: &str| {
        engine
            .doc
            .prerequisites
            .iter()
            .find(|p| p.id == id)
            .unwrap()
    };
    // the first thing that failed on the server: no `ensurepip`, which Debian puts in python3.X-venv
    assert_eq!(
        by("python_venv").check,
        PrerequisiteCheck::PythonModule {
            module: "ensurepip".into()
        }
    );
    assert_eq!(by("python_venv").fix.apt, ["python${python.version}-venv"]);
    assert!(
        by("python_venv").fix.dnf.is_empty(),
        "a Fedora Python has the module"
    );
    // the compiler is the one Triton looks for: $CC, else gcc, else clang
    assert_eq!(
        by("c_compiler").check,
        PrerequisiteCheck::Program {
            any_of: vec!["gcc".into(), "clang".into()],
            env: Some("CC".into())
        }
    );
    assert_eq!(by("c_compiler").fix.apt, ["build-essential"]);
    assert_eq!(
        by("python_headers").check,
        PrerequisiteCheck::PythonHeader {
            file: "Python.h".into()
        }
    );
    assert_eq!(
        by("python_headers").fix.apt,
        ["python${python.version}-dev"]
    );
    assert_eq!(
        by("nvidia_driver").check,
        PrerequisiteCheck::NvidiaDriver {
            at_least: "580".into()
        }
    );
    assert!(
        by("nvidia_driver").fix.is_empty(),
        "a driver is not installed by this program"
    );
    assert!(matches!(
        by("triton").check,
        PrerequisiteCheck::PythonScript { .. }
    ));
    // the condition of each is parsed, and the timeout of the script is the document's
    assert_eq!(engine.prerequisites.len(), 5);
    assert_eq!(
        engine.prerequisites[4].timeout,
        Some(std::time::Duration::from_secs(180))
    );
    // the pip step of the NVIDIA PyTorch and the driver it needs are kept in step: the same condition on cards
    let pip = engine
        .doc
        .acquire
        .as_ref()
        .and_then(|a| a.install.as_ref())
        .map(|i| format!("{i:?}"))
        .unwrap();
    assert!(
        pip.contains("cu130"),
        "the driver of the 580 series is for CUDA 13"
    );
    // no other engine says anything about it yet
    for other in ["llamacpp", "ollama", "openwebui"] {
        assert!(
            r.engine(other).unwrap().doc.prerequisites.is_empty(),
            "{other}"
        );
    }
}

/// Y80: a configuration says that it needs something of the computer by the id of a prerequisite of the
/// engine's document, with its own words about why, and it reads back as written; nobody gives it, so it is
/// not among the things that are asked of the person.
#[test]
fn p3_y80_a_configuration_says_what_it_needs_of_the_computer_and_reads_back_as_written() {
    let (c, r) = read(
        r#"
[[needs]]
system = "c_compiler"
why = "The int8 weights run through Triton."

[[needs]]
setting = "comfyui.models_dir"

[[needs]]
system = "nvidia_driver"
"#,
    );
    assert_eq!(c.needs.len(), 3);
    assert_eq!(
        c.needs[0].kind,
        NeedKind::System {
            id: "c_compiler".into()
        }
    );
    assert_eq!(
        c.needs[0].why.as_deref(),
        Some("The int8 weights run through Triton.")
    );
    assert!(!c.needs[0].secret);
    assert_eq!(c.needs[0].label(), "c_compiler");
    let text = c.to_toml(r.engine_map(), VERSION, false);
    assert!(text.contains("system = \"c_compiler\""), "{text}");
    assert!(text.contains("system = \"nvidia_driver\""), "{text}");
    let again = common::shipped_registry(&[(USER_PROFILE, &text)]);
    assert_eq!(
        again
            .profile("given")
            .expect("read again")
            .configuration
            .needs,
        c.needs,
        "{text}"
    );
    // what is asked of the person is the setting, and not what the computer has to have
    use quickai_core::profile::environment::Environment;
    let unmet = Environment::default().unmet(&c, &Default::default());
    assert_eq!(unmet.len(), 1, "{unmet:?}");
    assert_eq!(
        unmet[0].kind,
        NeedKind::Setting {
            key: "comfyui.models_dir".into()
        }
    );
}

/// Y81: a need of the computer that the engine's document does not know is refused with the ones it does, and
/// a need is still exactly one thing.
#[test]
fn p3_y81_a_need_of_the_computer_the_document_does_not_know_is_refused_with_the_ones_it_does() {
    let text = profile("[[needs]]\nsystem = \"a_better_compiler\"\n");
    let r = common::shipped_registry(&[(USER_PROFILE, &text)]);
    assert!(r.profile("given").is_none());
    let found = errors(&r).join("\n");
    assert!(
        found.contains(
            "`a_better_compiler` is not something the document of this engine knows how to check"
        ),
        "{found}"
    );
    assert!(
        found.contains("python_venv, c_compiler, python_headers, nvidia_driver, triton"),
        "{found}"
    );
    // an engine whose document names none says so
    let text = "schema = 1\nkind = \"profile\"\nid = \"given\"\nname = \"Given\"\nengine = \"ollama\"\n[[needs]]\nsystem = \"c_compiler\"\n";
    let r = common::shipped_registry(&[(USER_PROFILE, text)]);
    assert!(errors(&r).join("\n").contains("names no prerequisite"));
    // one thing at a time
    let text = profile("[[needs]]\nsystem = \"c_compiler\"\ntoken = \"huggingface\"\n");
    let r = common::shipped_registry(&[(USER_PROFILE, &text)]);
    assert!(errors(&r)
        .join("\n")
        .contains("exactly one of `token`, `setting`, `env` and `system`"));
}

/// Y82: which prerequisites a configuration asks for apply to a computer is worked out from the conditions of
/// the document and the facts of the computer; each is once, in the order of the configuration, with the words
/// the configuration gave; and one that does not apply is left out.
#[test]
fn p3_y82_what_applies_to_a_computer_is_worked_out_from_the_documents_conditions() {
    let (c, _) = read(
        r#"
[[needs]]
system = "nvidia_driver"
why = "CUDA 13."

[[needs]]
system = "c_compiler"

[[needs]]
system = "nvidia_driver"
why = "said twice"

[[needs]]
system = "triton"
"#,
    );
    let r = common::shipped_registry(&[]);
    let ids = |facts: &HostFacts| -> Vec<String> {
        wanted(&r, &c, facts)
            .iter()
            .map(|w| w.item.id.clone())
            .collect()
    };
    assert_eq!(
        ids(&facts("linux", &["nvidia"])),
        ["nvidia_driver", "c_compiler", "triton"],
        "in the order of the configuration, once each"
    );
    assert_eq!(
        ids(&facts("linux", &["amd"])),
        ["c_compiler"],
        "the driver and the test are for a card of NVIDIA's"
    );
    assert_eq!(
        ids(&facts("linux", &[])),
        ["c_compiler"],
        "cards that could not be told"
    );
    assert!(
        ids(&facts("macos", &["apple"])).is_empty(),
        "the document says Linux"
    );
    let found = wanted(&r, &c, &facts("linux", &["nvidia"]));
    assert_eq!(found[0].why, Some("CUDA 13."), "the first words win");
    assert_eq!(found[2].timeout(), std::time::Duration::from_secs(180));
    assert_eq!(
        found[1].timeout(),
        std::time::Duration::from_secs(120),
        "the default"
    );
}

/// Y83: the name of a package is the document's, with the version of the Python put in; nothing else gets in.
#[test]
fn p3_y83_the_packages_of_a_prerequisite_are_the_documents_with_the_pythons_version_in() {
    let r = common::shipped_registry(&[]);
    let engine = r.engine("comfyui").unwrap();
    let headers = &engine
        .doc
        .prerequisites
        .iter()
        .find(|p| p.id == "python_headers")
        .unwrap()
        .fix;
    assert_eq!(
        packages(headers, PackageManager::Apt, Some((3, 12))),
        Some(vec!["python3.12-dev".to_string()])
    );
    assert_eq!(
        packages(headers, PackageManager::Dnf, Some((3, 13))),
        Some(vec!["python3.13-devel".to_string()])
    );
    assert_eq!(
        packages(headers, PackageManager::Zypper, Some((3, 12))),
        Some(vec!["python312-devel".to_string()])
    );
    // a package named for a Python nobody knows cannot be named, and one that the document gives none for
    assert_eq!(packages(headers, PackageManager::Apt, None), None);
    assert_eq!(
        packages(headers, PackageManager::Pacman, Some((3, 12))),
        None
    );
    let compiler = &engine
        .doc
        .prerequisites
        .iter()
        .find(|p| p.id == "c_compiler")
        .unwrap()
        .fix;
    assert_eq!(
        packages(compiler, PackageManager::Apt, None),
        Some(vec!["build-essential".to_string()]),
        "no Python needed for a name that has no word of it"
    );
}

/// Y84: what a document says about the computer is held to what the program can do when it is loaded: a kind
/// of check it has, plain names of programs and packages (a package is installed as the administrator, so it
/// cannot be taken for an option or carry a command), a version that is numbers, a script that is one of the
/// application's own, and the checks of a Python environment only for an engine that makes one.
#[test]
fn p3_y84_a_document_that_says_something_the_program_cannot_do_is_refused_with_its_place() {
    let cases: &[(&str, &str)] = &[
        // the id and the label
        (
            "[[prerequisite]]\nid = \"a b\"\nlabel = \"x\"\ncheck = { use = \"program\", any_of = [\"gcc\"] }\n",
            "is not a valid prerequisite id",
        ),
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"program\", any_of = [\"gcc\"] }\n[[prerequisite]]\nid = \"x\"\nlabel = \"y\"\ncheck = { use = \"program\", any_of = [\"gcc\"] }\n",
            "the prerequisite id `x` is used twice",
        ),
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \" \"\ncheck = { use = \"program\", any_of = [\"gcc\"] }\n",
            "has a label of up to 120 characters",
        ),
        // the condition
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\nwhen = \"os ==\"\ncheck = { use = \"program\", any_of = [\"gcc\"] }\n",
            "`os ==`",
        ),
        // a program
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"program\", any_of = [] }\n",
            "names no program",
        ),
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"program\", any_of = [\"/usr/bin/gcc\"] }\n",
            "is not the name of a program",
        ),
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"program\", any_of = [\"gcc\"], env = \"1CC\" }\n",
            "is not the name of a variable",
        ),
        // a kind of check the program does not have is told with the ones it has
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"magic\" }\n",
            "python_module",
        ),
        // a module
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"python_module\", module = \"os; import evil\" }\n",
            "is not the name of a module",
        ),
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"python_module\", module = \"\" }\n",
            "is not the name of a module",
        ),
        // a header, a version, a script
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"python_header\", file = \"../Python.h\" }\n",
            "is not the name of a header file",
        ),
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"nvidia_driver\", at_least = \"five-eighty\" }\n",
            "is not a version",
        ),
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"python_script\", asset = \"comfyui/quickai_bridge\" }\n",
            "is not the name of one of the application's own scripts",
        ),
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"python_script\", asset = \"comfyui/selftest/triton_kernel.py\", timeout = \"soon\" }\n",
            "soon",
        ),
        // the packages
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"program\", any_of = [\"gcc\"] }\nfix = { apt = [\"-oAPT::Update::Pre-Invoke::=sh\"] }\n",
            "is not the name of a package",
        ),
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"program\", any_of = [\"gcc\"] }\nfix = { dnf = [\"gcc; rm -rf /\"] }\n",
            "is not the name of a package",
        ),
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"program\", any_of = [\"gcc\"] }\nfix = { apt = [\"python${home}-dev\"] }\n",
            "is not the name of a package",
        ),
        // what cannot be installed
        (
            "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = { use = \"nvidia_driver\", at_least = \"580\" }\nfix = { apt = [\"nvidia-driver-580\"] }\n",
            "a driver is not installed by this program",
        ),
        // a hint that is not a hint
        (
            &format!(
                "[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = {{ use = \"program\", any_of = [\"gcc\"] }}\nhint = \"{}\"\n",
                "long ".repeat(130)
            ),
            "the hint is at most 600 characters",
        ),
    ];
    for (snippet, why) in cases {
        let found = refused(snippet);
        assert!(
            found.contains(why),
            "{snippet:?}: wanted `{why}` in\n{found}"
        );
    }
    // a check of a Python environment belongs to an engine that makes one
    let llama = common::manifest("engines/llamacpp.toml");
    for check in [
        "{ use = \"python_header\", file = \"Python.h\" }",
        "{ use = \"python_module\", module = \"ensurepip\" }",
    ] {
        let text = format!(
            "{}\n[[prerequisite]]\nid = \"x\"\nlabel = \"x\"\ncheck = {check}\n",
            llama.replacen(
                "id = \"llamacpp\"\n",
                "id = \"llamacpp\"\nreplace = true\n",
                1
            )
        );
        let r =
            common::shipped_registry(&[("/home/u/.config/quickai/engines/llamacpp.toml", &text)]);
        let found = errors(&r).join("\n");
        assert!(
            found.contains("is checked in the environment of a `python_app` install"),
            "{check}: {found}"
        );
    }
    // and what is right is accepted, whatever the managers
    let right = "[[prerequisite]]\nid = \"x\"\nlabel = \"a thing\"\nwhen = \"os == 'linux'\"\ncheck = { use = \"program\", any_of = [\"g++\", \"x86_64-linux-gnu-gcc-13\"], env = \"CXX\" }\nfix = { apt = [\"g++\", \"libstdc++-13-dev\"], dnf = [\"gcc-c++\"], zypper = [\"gcc-c++\"], pacman = [\"gcc\"], apk = [\"build-base\"] }\nhint = \"Install it.\"\n";
    let r = registry_with(right);
    assert!(errors(&r).is_empty(), "{:?}", errors(&r));
    assert_eq!(r.engine("comfyui").unwrap().doc.prerequisites.len(), 1);
}
