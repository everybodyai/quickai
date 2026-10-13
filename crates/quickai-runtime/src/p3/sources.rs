//! Group Y of the test plan, the runtime's half: a configuration that runs on any computer (work package
//! 4.19, docs/headless.md). The configuration says where each file of a model can be had from; the person who
//! runs it says what is not where the file says (their own copy of a file, another address, a folder to look
//! in, a mirror, no network at all) and what belongs to their computer (an environment file, `--set`, `--env`,
//! the logins). Everything runs as the program runs it: `provision::up` on a file, in a data directory of the
//! test's own, against the world of group O (a fake GitHub, a file server, a stand-in `llama-server`) and a hub
//! that remembers who asked and with what login.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::ConfigDirs;
use crate::provision::{
    self, Event, FailureKind, FetchWord, FileInfo, Options, Outcome, Request, Secret,
};
use crate::testkit::archives::sha256_hex;
use crate::testkit::fake_github::Mode;
use crate::testkit::free_port;
use crate::testkit::headless_world::{model_bytes, Hub, World, FILE, REPO};

fn bytes_of(name: &str) -> Vec<u8> {
    model_bytes(name)
}

/// The shipped llama.cpp document patched, as `World` patches it, to fetch its models from `site`.
fn site_of_models(world: &World, site: &str) {
    let path = world.config.join("engines/llamacpp.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let old = format!("site = \"{}\"", world.files.base());
    assert!(text.contains(&old), "{text}");
    std::fs::write(path, text.replace(&old, &format!("site = \"{site}\""))).unwrap();
}

/// A configuration for llama.cpp on this world's port.
fn llama(world: &World, name: &str, models: &str) -> PathBuf {
    world.file(
        name,
        &format!("{models}\n[values.llamacpp]\nport = {}\n", world.port),
    )
}

fn plan_steps(events: &[Event]) -> Vec<provision::StepInfo> {
    events
        .iter()
        .find_map(|e| match e {
            Event::Plan { steps } => Some(steps.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn fetch_files(events: &[Event]) -> Vec<FileInfo> {
    plan_steps(events)
        .into_iter()
        .flat_map(|s| s.files)
        .collect()
}

fn failure_of(outcome: Outcome) -> provision::Failure {
    match outcome {
        Outcome::Failed(failure) => failure,
        other => panic!("it should have failed, and it was {other:?}"),
    }
}

fn prepare(world: &World, accept: bool) -> Options {
    Options {
        prepare_only: true,
        accept_review: accept,
        ..world.options()
    }
}

fn check(world: &World, accept: bool) -> Options {
    Options {
        dry_run: true,
        accept_review: accept,
        ..world.options()
    }
}

fn say_login(token: &str) -> Vec<FetchWord> {
    vec![FetchWord {
        key: "huggingface_token".into(),
        value: Secret::new(token),
        origin: "--hf-token".into(),
    }]
}

// ---------------------------------------------------------------------------------------------- Y30-Y33

/// Y30: a file of a place with a path in the repository and a revision is fetched from the address those make,
/// into the models folder, once; the plan says the address and the place it goes to; a second run asks for
/// nothing.
#[test]
fn p3_y30_a_file_with_a_path_and_a_revision_is_fetched_from_the_address_those_make() {
    let world = World::new("y30");
    let deep = bytes_of("Deep.gguf");
    let sum = sha256_hex(&deep);
    let path = format!("/{REPO}/resolve/abc123/sub%20dir/Deep.gguf");
    world.files.add(&path, deep.clone());
    let file = llama(
        &world,
        "deep",
        &format!(
            "[[model]]\nfile = \"Deep.gguf\"\nfrom = \"huggingface:{REPO}\"\npath = \"sub dir/Deep.gguf\"\nrevision = \"abc123\"\nsize = {}\nsha256 = \"{sum}\"\n",
            deep.len()
        ),
    );

    let (outcome, events) = world.up(&file, check(&world, true));
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
    let files = fetch_files(&events);
    assert_eq!(
        files,
        [FileInfo {
            name: "Deep.gguf".into(),
            how: "fetch".into(),
            from: world.files.url(&path),
            to: world.data.join("models/Deep.gguf").display().to_string(),
            bytes: Some(deep.len() as u64),
            sha256: Some(sum.clone()),
        }]
    );
    let step = &plan_steps(&events)[1];
    assert!(
        step.text.starts_with("fetch Deep.gguf (")
            && step.text.ends_with(&format!("from huggingface:{REPO}")),
        "{}",
        step.text
    );
    assert!(
        !world.files.paths_asked().contains(&path),
        "a plan asks for no file"
    );

    let (outcome, events) = world.up(&file, prepare(&world, true));
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(
        std::fs::read(world.data.join("models/Deep.gguf")).unwrap(),
        deep
    );
    assert_eq!(
        world
            .files
            .paths_asked()
            .iter()
            .filter(|p| **p == path)
            .count(),
        1
    );
    let leftovers: Vec<String> = std::fs::read_dir(world.data.join("models"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, ["Deep.gguf"], "no partial file is left");

    // a second run finds it, and asks for nothing
    let asked = world.files.requests();
    let (outcome, events) = world.up(&file, prepare(&world, true));
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(world.files.requests(), asked);
    assert!(fetch_files(&events).is_empty());
}

/// A stand-in for ComfyUI's launcher: any program that exists, since the files are what these tests are about.
fn launcher(world: &World) -> PathBuf {
    let path = world.scratch.join("comfy-launcher");
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// ComfyUI's document patched to fetch its models from the world's file server.
fn comfy_world(name: &str) -> (World, PathBuf) {
    let world = World::new(name);
    std::fs::write(
        world.config.join("engines/comfyui.toml"),
        format!(
            "schema = 1\nkind = \"engine\"\nid = \"comfyui\"\n\n[[models.lookup]]\nid = \"hub\"\nsite = \"{}\"\n",
            world.files.base()
        ),
    )
    .unwrap();
    let program = launcher(&world);
    (world, program)
}

fn comfy_file(world: &World, name: &str, body: &str) -> PathBuf {
    let path = world.scratch.join(name);
    std::fs::write(
        &path,
        format!(
            "schema = 1\nkind = \"profile\"\nid = \"{name}\"\nname = \"{name}\"\nengine = \"comfyui\"\n\n{body}"
        ),
    )
    .unwrap();
    path
}

fn comfy_options(world: &World, program: &Path, prepare_only: bool) -> Options {
    Options {
        prepare_only,
        dry_run: !prepare_only,
        accept_review: true,
        sets: vec![format!("comfyui.bin_override={}", program.display())],
        ..world.options()
    }
}

/// Y31: the files of a video model, three kinds from two repositories and an address, each in a folder of its
/// own, are put where ComfyUI looks for each, by the engine's own document: the folder the name suggests, or
/// the one the file says; and a folder the engine has not is refused with the ones it has.
#[test]
fn p3_y31_the_files_of_a_set_land_in_the_folders_the_engine_keeps_each_kind_in() {
    let (world, program) = comfy_world("y31");
    let (a, b, c, d) = (
        b"diffusion weights".to_vec(),
        b"text encoder".to_vec(),
        b"variational".to_vec(),
        b"a style".to_vec(),
    );
    world.files.add(
        "/Comfy-Org/Wan_2.2_ComfyUI_Repackaged/resolve/main/split_files/diffusion_models/wan2.2_ti2v_5B_fp16.safetensors",
        a.clone(),
    );
    world.files.add(
        "/Comfy-Org/Wan_2.1_ComfyUI_repackaged/resolve/main/split_files/text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors",
        b.clone(),
    );
    world.files.add("/mirror/wan2.2_vae.safetensors", c.clone());
    world
        .files
        .add("/acme/Styles/resolve/main/style.safetensors", d.clone());
    let body = format!(
        r#"[[model]]
role = "diffusion"
file = "wan2.2_ti2v_5B_fp16.safetensors"
from = "huggingface:Comfy-Org/Wan_2.2_ComfyUI_Repackaged"
path = "split_files/diffusion_models/wan2.2_ti2v_5B_fp16.safetensors"

[[model]]
role = "text_encoder"
file = "umt5_xxl_fp8_e4m3fn_scaled.safetensors"
from = "huggingface:Comfy-Org/Wan_2.1_ComfyUI_repackaged"
path = "split_files/text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors"

[[model]]
role = "vae"
file = "wan2.2_vae.safetensors"
url = "{}"

[[model]]
role = "lora"
file = "style.safetensors"
from = "huggingface:acme/Styles"
into = "loras"
"#,
        world.files.url("/mirror/wan2.2_vae.safetensors")
    );
    let file = comfy_file(&world, "wan", &body);
    let (outcome, events) = world.up(&file, comfy_options(&world, &program, true));
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    let models = world.data.join("comfyui/models");
    for (path, bytes) in [
        ("diffusion_models/wan2.2_ti2v_5B_fp16.safetensors", &a),
        ("text_encoders/umt5_xxl_fp8_e4m3fn_scaled.safetensors", &b),
        ("vae/wan2.2_vae.safetensors", &c),
        ("loras/style.safetensors", &d),
    ] {
        assert_eq!(
            std::fs::read(models.join(path)).unwrap_or_else(|e| panic!("{path}: {e}")),
            *bytes,
            "{path}"
        );
    }
    // one step for each, and the plan said where each went
    let steps = plan_steps(&events);
    let tos: Vec<String> = steps
        .iter()
        .flat_map(|s| s.files.iter().map(|f| f.to.clone()))
        .collect();
    assert_eq!(tos.len(), 4, "{steps:#?}");
    assert!(
        tos[2].ends_with("comfyui/models/vae/wan2.2_vae.safetensors"),
        "{tos:?}"
    );

    // a folder the engine does not keep files in is told, with the ones it does
    let wrong = comfy_file(
        &world,
        "wrong",
        "[[model]]\nfile = \"x.safetensors\"\nurl = \"http://127.0.0.1:1/x\"\ninto = \"nowhere\"\n",
    );
    let (outcome, _) = world.up(&wrong, comfy_options(&world, &program, false));
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Needs);
    assert!(
        failure.message.contains("`nowhere`")
            && failure.message.contains("diffusion_models")
            && failure.message.contains("vae"),
        "{}",
        failure.message
    );
    // and an engine that keeps all its files in one folder has no folder to put one in
    let flat = llama(
        &world,
        "flat",
        "[[model]]\nfile = \"x.gguf\"\nurl = \"http://127.0.0.1:1/x\"\ninto = \"vae\"\n",
    );
    let (outcome, _) = world.up(&flat, check(&world, true));
    let failure = failure_of(outcome);
    assert!(
        failure
            .message
            .contains("keeps all its files in one folder"),
        "{}",
        failure.message
    );
}

/// Y32: what arrives is what the configuration says: an address is fetched from, and a file whose checksum or
/// size is not the one stated is not kept (nothing partial is left), with a sentence that says which.
#[test]
fn p3_y32_what_arrives_is_checked_against_what_the_configuration_states() {
    let world = World::new("y32");
    let good = bytes_of("Url.gguf");
    world.files.add("/mirror/Url.gguf", good.clone());
    let url = world.files.url("/mirror/Url.gguf");
    let lie = "0".repeat(64);

    let bad_sum = llama(
        &world,
        "bad-sum",
        &format!("[[model]]\nfile = \"Url.gguf\"\nurl = \"{url}\"\nsha256 = \"{lie}\"\n"),
    );
    let (outcome, _) = world.up(&bad_sum, prepare(&world, true));
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Fetch);
    assert!(
        failure.message.contains("does not match the checksum")
            && failure.message.contains("not kept"),
        "{}",
        failure.message
    );
    let models = world.data.join("models");
    let names = |dir: &Path| -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    assert!(names(&models).is_empty(), "{:?}", names(&models));

    let bad_size = llama(
        &world,
        "bad-size",
        &format!(
            "[[model]]\nfile = \"Url.gguf\"\nurl = \"{url}\"\nsize = {}\n",
            good.len() + 1
        ),
    );
    let (outcome, _) = world.up(&bad_size, prepare(&world, true));
    let failure = failure_of(outcome);
    assert!(
        failure
            .message
            .contains(&format!("came to {} bytes", good.len()))
            && failure.message.contains("the configuration says"),
        "{}",
        failure.message
    );
    assert!(names(&models).is_empty(), "{:?}", names(&models));

    // the right checksum and size are kept
    let right = llama(
        &world,
        "right",
        &format!(
            "[[model]]\nfile = \"Url.gguf\"\nurl = \"{url}\"\nsize = {}\nsha256 = \"{}\"\n",
            good.len(),
            sha256_hex(&good)
        ),
    );
    let (outcome, events) = world.up(&right, prepare(&world, true));
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(std::fs::read(models.join("Url.gguf")).unwrap(), good);
}

/// Y33: where a file from outside says to fetch from is read by the person before it is used: the sentence has
/// the whole address (the repository, the file in it, the revision; or the address), nothing is fetched
/// without the yes, a configuration the person keeps asks nothing, and a person who says where to get the file
/// themselves is not asked about the place the file names.
#[test]
fn p3_y33_a_place_a_file_from_outside_names_is_read_first() {
    let world = World::new("y33");
    world.files.add("/mirror/Url.gguf", bytes_of("Url.gguf"));
    let by_repo = llama(
        &world,
        "by-repo",
        &format!(
            "[[model]]\nfile = \"Hinted.gguf\"\nfrom = \"huggingface:{REPO}\"\npath = \"Hinted.gguf\"\nrevision = \"v2\"\n"
        ),
    );
    let (outcome, events) = world.up(&by_repo, prepare(&world, false));
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Asks, "{events:#?}");
    assert!(
        failure
            .message
            .contains(&format!("huggingface:{REPO} (the file Hinted.gguf at v2)"))
            && failure.message.contains("--accept-review")
            && failure.message.contains("--model-source Hinted.gguf=PATH"),
        "{}",
        failure.message
    );
    let url = world.files.url("/mirror/Url.gguf?sig=abc");
    let by_url = llama(
        &world,
        "by-url",
        &format!("[[model]]\nfile = \"Url.gguf\"\nurl = \"{url}\"\n"),
    );
    let (outcome, _) = world.up(&by_url, prepare(&world, false));
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Asks);
    assert!(
        failure.message.contains("/mirror/Url.gguf?sig=abc"),
        "{}",
        failure.message
    );
    assert_eq!(world.files.requests(), 0, "nothing was fetched");
    assert!(!world.data.exists(), "nothing was written");

    // a folder the person points at that has a file of that name that is not the right one does not make the place
    // the file names a place they have read: it is asked about all the same (found by writing the rule down: the
    // first version let a file of the wrong size in a folder stand for the person's yes)
    let wrong_folder = world.scratch.join("wrong-folder");
    std::fs::create_dir_all(&wrong_folder).unwrap();
    std::fs::write(wrong_folder.join("Hinted.gguf"), b"short").unwrap();
    let sized = llama(
        &world,
        "sized",
        &format!(
            "[[model]]\nfile = \"Hinted.gguf\"\nfrom = \"huggingface:{REPO}\"\nsize = 40000\n"
        ),
    );
    let (outcome, _) = world.up(
        &sized,
        Options {
            models_from: vec![wrong_folder],
            ..prepare(&world, false)
        },
    );
    assert_eq!(failure_of(outcome).kind, FailureKind::Asks);
    assert_eq!(world.files.requests(), 0, "nothing was fetched");

    // the person's own copy of the file is the place, and the file's is not asked about
    let own = world.scratch.join("mine.gguf");
    std::fs::write(&own, bytes_of("Hinted.gguf")).unwrap();
    let (outcome, events) = world.up(
        &by_repo,
        Options {
            model_sources: vec![format!("Hinted.gguf={}", own.display())],
            ..prepare(&world, false)
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");

    // a configuration the person keeps is not a file from outside
    std::fs::create_dir_all(world.config.join("profiles")).unwrap();
    std::fs::write(
        world.config.join("profiles/kept.toml"),
        format!(
            "schema = 1\nkind = \"profile\"\nid = \"kept\"\nname = \"Kept\"\nengine = \"llamacpp\"\n\n[[model]]\nfile = \"Url.gguf\"\nurl = \"{}\"\n\n[values.llamacpp]\nport = {}\n",
            world.files.url("/mirror/Url.gguf"),
            world.port
        ),
    )
    .unwrap();
    let (outcome, events) = world.up(Path::new("kept"), prepare(&world, false));
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert!(world.data.join("models/Url.gguf").is_file());
}

// ---------------------------------------------------------------------------------------------- Y34-Y36

/// Y34: what the person says about where a file is wins over where the configuration says: their own file is
/// checked and used where it is (a link, nothing fetched), a folder is looked in for the name, another address
/// or repository is fetched from, and a name the configuration fetches no file by is a mistake that lists the
/// names it has.
#[test]
fn p3_y34_what_is_said_about_where_a_file_is_wins_over_the_configuration() {
    let world = World::new("y34");
    let model = bytes_of("Own.gguf");
    let sum = sha256_hex(&model);
    let configuration = llama(
        &world,
        "own",
        &format!(
            "[[model]]\nfile = \"Own.gguf\"\nfrom = \"huggingface:{REPO}\"\nsize = {}\nsha256 = \"{sum}\"\n",
            model.len()
        ),
    );
    let mine = world.scratch.join("mine/Own.gguf");
    std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
    std::fs::write(&mine, &model).unwrap();

    // a file of their own: linked, not copied, and the hub is not asked
    let (outcome, events) = world.up(
        &configuration,
        Options {
            model_sources: vec![format!("Own.gguf={}", mine.display())],
            ..prepare(&world, false)
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    let kept = world.data.join("models/Own.gguf");
    assert!(
        std::fs::symlink_metadata(&kept)
            .unwrap()
            .file_type()
            .is_symlink(),
        "it is a link to the file, not a copy of it"
    );
    assert_eq!(std::fs::read(&kept).unwrap(), model);
    assert!(
        !world
            .files
            .paths_asked()
            .iter()
            .any(|p| p.contains("Own.gguf")),
        "{:?}",
        world.files.paths_asked()
    );
    let step = plan_steps(&events)
        .into_iter()
        .find(|s| s.kind == "link")
        .expect("a step that uses it");
    assert!(
        step.text.starts_with("use Own.gguf") && step.text.ends_with("that you have"),
        "{}",
        step.text
    );
    assert_eq!(step.files[0].how, "link");
    assert_eq!(step.files[0].from, mine.display().to_string());
    std::fs::remove_file(&kept).unwrap();

    // a folder that has it by name
    let (outcome, events) = world.up(
        &configuration,
        Options {
            model_sources: vec![format!("Own.gguf={}", mine.parent().unwrap().display())],
            ..prepare(&world, false)
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(std::fs::read(&kept).unwrap(), model);
    std::fs::remove_file(&kept).unwrap();

    // a file that is not the one: the size, then the checksum
    let wrong = world.scratch.join("mine/Wrong.gguf");
    std::fs::write(&wrong, b"short").unwrap();
    let (outcome, _) = world.up(
        &configuration,
        Options {
            model_sources: vec![format!("Own.gguf={}", wrong.display())],
            ..prepare(&world, false)
        },
    );
    let failure = failure_of(outcome);
    assert!(
        failure.message.contains("5 bytes") && failure.message.contains("it was not used"),
        "{}",
        failure.message
    );
    let same_size = world.scratch.join("mine/Same.gguf");
    std::fs::write(&same_size, vec![b'x'; model.len()]).unwrap();
    let (outcome, _) = world.up(
        &configuration,
        Options {
            model_sources: vec![format!("Own.gguf={}", same_size.display())],
            ..prepare(&world, false)
        },
    );
    let failure = failure_of(outcome);
    assert!(
        failure.message.contains("does not match the checksum"),
        "{}",
        failure.message
    );
    assert!(
        !kept.exists() || std::fs::read(&kept).unwrap() == model,
        "nothing wrong was put in its place"
    );
    assert!(
        std::fs::symlink_metadata(&kept).is_err(),
        "and nothing was linked"
    );

    // another address, another repository
    world.files.add("/elsewhere/Own.gguf", model.clone());
    let (outcome, events) = world.up(
        &configuration,
        Options {
            model_sources: vec![format!(
                "Own.gguf={}",
                world.files.url("/elsewhere/Own.gguf")
            )],
            ..prepare(&world, false)
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert!(world
        .files
        .paths_asked()
        .contains(&"/elsewhere/Own.gguf".to_string()));
    std::fs::remove_file(&kept).unwrap();
    world
        .files
        .add("/me/My-Copy/resolve/main/Own.gguf", model.clone());
    let (outcome, events) = world.up(
        &configuration,
        Options {
            model_sources: vec!["Own.gguf=huggingface:me/My-Copy".to_string()],
            ..prepare(&world, false)
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert!(world
        .files
        .paths_asked()
        .contains(&"/me/My-Copy/resolve/main/Own.gguf".to_string()));

    // a name the configuration fetches no file by
    let (outcome, _) = world.up(
        &configuration,
        Options {
            model_sources: vec!["Typo.gguf=/x".to_string()],
            ..check(&world, false)
        },
    );
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Mistake);
    assert!(
        failure.message.contains("`Typo.gguf`") && failure.message.contains("Own.gguf"),
        "{}",
        failure.message
    );
    // a file that is not there
    std::fs::remove_file(&kept).unwrap();
    let (outcome, _) = world.up(
        &configuration,
        Options {
            model_sources: vec!["Own.gguf=/nowhere/Own.gguf".to_string()],
            ..prepare(&world, false)
        },
    );
    let failure = failure_of(outcome);
    assert!(
        failure
            .message
            .contains("there is no file at /nowhere/Own.gguf"),
        "{}",
        failure.message
    );
}

/// Y35: a folder the person points at is looked in before anything is fetched: for the name the file is kept
/// under, in the folder of its kind, and by the path it has in its repository; a file there of the wrong size
/// is said and not used; what is used is linked, and what is not found is fetched as before.
#[test]
fn p3_y35_a_folder_the_person_points_at_is_looked_in_first() {
    let (world, program) = comfy_world("y35");
    let (a, b) = (b"weights".to_vec(), b"vae bytes".to_vec());
    world.files.add(
        "/org/Repo/resolve/main/split_files/diffusion_models/m.safetensors",
        a.clone(),
    );
    world.files.add(
        "/org/Repo/resolve/main/split_files/vae/v.safetensors",
        b.clone(),
    );
    let body = format!(
        r#"[[model]]
file = "m.safetensors"
from = "huggingface:org/Repo"
path = "split_files/diffusion_models/m.safetensors"
size = {}

[[model]]
file = "v.safetensors"
from = "huggingface:org/Repo"
path = "split_files/vae/v.safetensors"
into = "vae"
size = {}
"#,
        a.len(),
        b.len()
    );
    let file = comfy_file(&world, "pair", &body);
    // a copy of the repository's layout, and a folder with the files by name: the first holds one, the second
    // the other, and the first also has a file of the same name that is not the right size
    let layout = world.scratch.join("mirror");
    let by_name = world.scratch.join("flat");
    std::fs::create_dir_all(layout.join("split_files/diffusion_models")).unwrap();
    std::fs::create_dir_all(&by_name).unwrap();
    std::fs::write(
        layout.join("split_files/diffusion_models/m.safetensors"),
        &a,
    )
    .unwrap();
    std::fs::write(by_name.join("v.safetensors"), b"not it").unwrap();
    let options = |dirs: Vec<PathBuf>| Options {
        models_from: dirs,
        ..comfy_options(&world, &program, true)
    };
    let (outcome, events) = world.up(&file, options(vec![by_name.clone(), layout.clone()]));
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    let models = world.data.join("comfyui/models");
    // the one found by its path in the repository is a link, the one of the wrong size is said and fetched
    assert!(
        std::fs::symlink_metadata(models.join("diffusion_models/m.safetensors"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read(models.join("diffusion_models/m.safetensors")).unwrap(),
        a
    );
    assert!(!std::fs::symlink_metadata(models.join("vae/v.safetensors"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read(models.join("vae/v.safetensors")).unwrap(), b);
    let said: Vec<&String> = events
        .iter()
        .filter_map(|e| match e {
            Event::Say { text } => Some(text),
            _ => None,
        })
        .collect();
    assert!(
        said.iter()
            .any(|t| t.contains("v.safetensors is 6 bytes") && t.contains("not used")),
        "{said:?}"
    );
    let asked = world.files.paths_asked();
    assert!(
        !asked
            .iter()
            .any(|p| p.ends_with("diffusion_models/m.safetensors")),
        "the one that was found is not fetched: {asked:?}"
    );
    assert!(asked.iter().any(|p| p.ends_with("vae/v.safetensors")));
}

/// Y36: offline, nothing is fetched and nothing is asked for: what is not here is said, one file at a time,
/// with the place it goes and the address it would have been fetched from; what is all here runs; a program
/// that is not installed is said to be one of the things to bring.
#[test]
fn p3_y36_offline_nothing_is_fetched_and_what_is_missing_is_said_with_where_it_goes() {
    let world = World::new("y36");
    let file = llama(
        &world,
        "offline",
        &format!("[[model]]\nfile = \"Hinted.gguf\"\nfrom = \"huggingface:{REPO}\"\n"),
    );
    // no program either: it cannot be installed from here
    let (outcome, events) = world.up(
        &file,
        Options {
            offline: true,
            ..check(&world, true)
        },
    );
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Needs);
    assert!(
        failure.message.contains("--offline")
            && failure.message.contains("is not on this computer"),
        "{}",
        failure.message
    );
    assert_eq!(world.files.requests(), 0, "{events:#?}");
    assert!(
        world.github.requests().is_empty(),
        "the list of builds was not asked for"
    );

    // with the program in place (a build that was installed before), the files are what is missing
    let (outcome, _) = world.up(&file, prepare(&world, true));
    assert_eq!(outcome, Outcome::Prepared);
    let model = world.data.join("models/Hinted.gguf");
    std::fs::remove_file(&model).unwrap();
    let asked = (world.files.requests(), world.github.requests().len());
    let (outcome, events) = world.up(
        &file,
        Options {
            offline: true,
            ..prepare(&world, true)
        },
    );
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Needs);
    assert!(
        failure.message.contains("nothing is fetched")
            && failure.message.contains("Hinted.gguf")
            && failure.message.contains(&model.display().to_string())
            && failure
                .message
                .contains(&format!("/{REPO}/resolve/main/Hinted.gguf")),
        "{}",
        failure.message
    );
    // the plan was still said, so that the person sees what it is
    assert!(!fetch_files(&events).is_empty());
    assert_eq!(
        (world.files.requests(), world.github.requests().len()),
        asked
    );
    assert!(!model.exists());

    // the file put there by any means is what it needs
    std::fs::write(&model, bytes_of("Hinted.gguf")).unwrap();
    let (outcome, events) = world.up(
        &file,
        Options {
            offline: true,
            ..prepare(&world, true)
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(
        (world.files.requests(), world.github.requests().len()),
        asked
    );

    // and so is a folder that has it
    std::fs::remove_file(&model).unwrap();
    let held = world.scratch.join("held");
    std::fs::create_dir_all(&held).unwrap();
    std::fs::write(held.join("Hinted.gguf"), bytes_of("Hinted.gguf")).unwrap();
    let (outcome, events) = world.up(
        &file,
        Options {
            offline: true,
            models_from: vec![held],
            ..prepare(&world, true)
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(
        (world.files.requests(), world.github.requests().len()),
        asked
    );
}

// ---------------------------------------------------------------------------------------------- Y37-Y38

/// Y37: a login is sent to the hub it was given for and to no other: the file behind a login answers 401
/// without it (and the sentence says how to give one), 403 with the wrong one, and is fetched with the right
/// one; the host a hub sends a file on to never sees it; a mirror takes the place of the hub and gets the
/// login instead.
#[test]
fn p3_y37_a_login_goes_to_the_hub_it_was_given_for_and_no_other() {
    let world = World::new("y37");
    let hub = Hub::start();
    let model = bytes_of("Gated.gguf");
    hub.add("/private/Repo/resolve/main/Gated.gguf", model.clone());
    hub.require_login("/private/", "hf_right");
    site_of_models(&world, &hub.base());
    let file = llama(
        &world,
        "gated",
        "[[model]]\nfile = \"Gated.gguf\"\nfrom = \"huggingface:private/Repo\"\n",
    );

    // none
    let (outcome, _) = world.up(&file, prepare(&world, true));
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Fetch);
    assert!(
        failure.message.contains("answered 401")
            && failure.message.contains("--hf-token")
            && failure.message.contains("HF_TOKEN"),
        "{}",
        failure.message
    );
    // the wrong one
    let (outcome, _) = world.up(
        &file,
        Options {
            fetch: say_login("hf_wrong"),
            ..prepare(&world, true)
        },
    );
    let failure = failure_of(outcome);
    assert!(
        failure.message.contains("answered 403") && failure.message.contains("token it was given"),
        "{}",
        failure.message
    );
    assert!(
        !failure.message.contains("hf_wrong"),
        "a login is never in a sentence: {}",
        failure.message
    );
    // the right one
    let (outcome, events) = world.up(
        &file,
        Options {
            fetch: say_login("hf_right"),
            ..prepare(&world, true)
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(
        std::fs::read(world.data.join("models/Gated.gguf")).unwrap(),
        model
    );
    let asked = hub.asked();
    assert_eq!(
        asked.first().and_then(|a| a.authorization.clone()),
        None,
        "the first try had none"
    );
    assert_eq!(
        asked.last().and_then(|a| a.authorization.clone()),
        Some("Bearer hf_right".to_string())
    );
    // the list of builds, which is another host, never saw it
    assert!(
        world.github.requests().iter().all(|(_, h)| h
            .iter()
            .all(|(k, _)| !k.eq_ignore_ascii_case("authorization"))),
        "{:?}",
        world.github.requests()
    );
}

/// Y37 (continued): the host a hub sends a file on to, and a mirror.
#[test]
fn p3_y37b_a_login_does_not_follow_a_file_to_the_host_the_hub_sends_it_to_and_a_mirror_gets_it() {
    let world = World::new("y37b");
    let (hub, download_host, mirror) = (Hub::start(), Hub::start(), Hub::start());
    let model = bytes_of("Moved.gguf");
    // the hub sends the file on to a download server (which answers to another name, so it is another host)
    download_host.add("/cdn/Moved.gguf", model.clone());
    hub.add("/acme/Repo/resolve/main/Moved.gguf", Vec::new());
    hub.redirect(
        "/acme/Repo/resolve/main/Moved.gguf",
        &format!("{}/cdn/Moved.gguf", download_host.base_as_localhost()),
    );
    site_of_models(&world, &hub.base());
    let file = llama(
        &world,
        "moved",
        "[[model]]\nfile = \"Moved.gguf\"\nfrom = \"huggingface:acme/Repo\"\n",
    );
    let (outcome, events) = world.up(
        &file,
        Options {
            fetch: say_login("hf_secret"),
            ..prepare(&world, true)
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(
        std::fs::read(world.data.join("models/Moved.gguf")).unwrap(),
        model
    );
    assert_eq!(
        hub.asked()[0].authorization.as_deref(),
        Some("Bearer hf_secret"),
        "the hub was given the login"
    );
    let after = download_host.asked();
    assert_eq!(after.len(), 1, "{after:?}");
    assert_eq!(after[0].authorization, None, "the download server was not");

    // a mirror takes the hub's place, and gets the login in its place
    std::fs::remove_file(world.data.join("models/Moved.gguf")).unwrap();
    let before = hub.asked().len();
    mirror.add("/acme/Repo/resolve/main/Moved.gguf", model.clone());
    let (outcome, events) = world.up(
        &file,
        Options {
            fetch: vec![
                FetchWord {
                    key: "huggingface_endpoint".into(),
                    value: Secret::new(mirror.base()),
                    origin: "--hf-endpoint".into(),
                },
                FetchWord {
                    key: "huggingface_token".into(),
                    value: Secret::new("hf_secret"),
                    origin: "--hf-token".into(),
                },
            ],
            ..prepare(&world, true)
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(hub.asked().len(), before, "the hub was not asked again");
    let asked = mirror.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].authorization.as_deref(), Some("Bearer hf_secret"));
    assert_eq!(asked[0].path, "/acme/Repo/resolve/main/Moved.gguf");
    // and the plan said where it would come from: the mirror
    let from = fetch_files(&events);
    assert!(
        from.is_empty() || from[0].from.starts_with(&mirror.base()),
        "{from:?}"
    );
}

/// Y37 (continued): the login the person gave for the hub does not go with a request for an address they named on
/// another host.
#[test]
fn p3_y37c_a_login_does_not_go_to_an_address_the_person_names_on_another_host() {
    let world = World::new("y37c");
    let (hub, other) = (Hub::start(), Hub::start());
    let model = bytes_of("Gated.gguf");
    hub.add("/private/Repo/resolve/main/Gated.gguf", model.clone());
    hub.require_login("/private/", "hf_for_the_hub");
    other.add("/files/Gated.gguf", model.clone());
    site_of_models(&world, &hub.base());
    let file = llama(
        &world,
        "gated",
        "[[model]]\nfile = \"Gated.gguf\"\nfrom = \"huggingface:private/Repo\"\n",
    );
    let (outcome, events) = world.up(
        &file,
        Options {
            fetch: say_login("hf_for_the_hub"),
            model_sources: vec![format!(
                "Gated.gguf={}/files/Gated.gguf",
                other.base_as_localhost()
            )],
            ..prepare(&world, true)
        },
    );
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(
        std::fs::read(world.data.join("models/Gated.gguf")).unwrap(),
        model
    );
    let asked = other.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(
        asked[0].authorization, None,
        "the other host was not given the hub's login"
    );
    assert!(hub.asked().is_empty(), "and the hub was not asked");
}

/// Y38: a GitHub login makes the requests for the list of builds signed in, and nothing else is; without
/// one, a used-up allowance says how to give one.
#[test]
fn p3_y38_a_github_login_signs_in_the_list_of_builds_and_nothing_else() {
    let world = World::new("y38");
    let file = llama(&world, "plain", "");
    let signed = |world: &World| -> Vec<Option<String>> {
        world
            .github
            .requests()
            .iter()
            .map(|(_, headers)| {
                headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                    .map(|(_, v)| v.clone())
            })
            .collect()
    };
    let (outcome, _) = world.up(&file, check(&world, true));
    assert_eq!(outcome, Outcome::Planned);
    assert!(signed(&world).iter().all(Option::is_none) && !signed(&world).is_empty());

    let before = world.github.requests().len();
    let (outcome, _) = world.up(
        &file,
        Options {
            fetch: vec![FetchWord {
                key: "github_token".into(),
                value: Secret::new("ghp_login"),
                origin: "the variable GITHUB_TOKEN".into(),
            }],
            ..check(&world, true)
        },
    );
    assert_eq!(outcome, Outcome::Planned);
    let after = signed(&world);
    assert!(after.len() > before);
    assert!(
        after[before..]
            .iter()
            .all(|a| a.as_deref() == Some("Bearer ghp_login")),
        "{after:?}"
    );
    assert!(
        world.files.paths_asked().is_empty(),
        "and no file server was given it, whatever it was asked"
    );

    // a used-up allowance says how to raise it, unless it was raised already
    world.github.mode(Mode::RateLimited {
        reset_epoch: 4_000_000_000,
    });
    let (outcome, _) = world.up(&file, check(&world, true));
    let failure = failure_of(outcome);
    assert!(
        failure.message.contains("--github-token") && failure.message.contains("GITHUB_TOKEN"),
        "{}",
        failure.message
    );
    let (outcome, _) = world.up(
        &file,
        Options {
            fetch: vec![FetchWord {
                key: "github_token".into(),
                value: Secret::new("ghp_login"),
                origin: "--github-token".into(),
            }],
            ..check(&world, true)
        },
    );
    let failure = failure_of(outcome);
    assert!(
        !failure.message.contains("--github-token"),
        "{}",
        failure.message
    );
    assert!(
        !failure.message.contains("ghp_login"),
        "{}",
        failure.message
    );
}

// ---------------------------------------------------------------------------------------------- Y40-Y43

/// A program whose document has a field of lines of environment, and the stand-in that says what it got.
fn env_engine(world: &World, port: u16) -> PathBuf {
    let dir = world.scratch.join("envprog");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("standin.py"),
        crate::testkit::headless_world::STANDIN,
    )
    .unwrap();
    let run = dir.join("run");
    std::fs::write(
        &run,
        "#!/bin/sh\nexec python3 \"$(dirname \"$0\")/standin.py\" \"$@\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        world.config.join("engines/envprog.toml"),
        format!(
            r#"schema = 1
kind = "engine"
id = "envprog"
requires = {{ primitives = ["locate.known_paths@1", "spawn@1", "probe.http@1"] }}

[engine]
name = "envprog"
server_name = "envprog"

[provides]
apis = ["openai.chat"]
default_port = {port}
needs_model = false

[locate]
strategies = [ {{ use = "known_paths", linux = ["{run}"], macos = ["{run}"] }} ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${{runtime.binary}}"
args = ["--port", "${{listen_port}}"]
group = false
stop = {{ policy = "kill" }}

[launch.ready]
probe = {{ use = "http", path = "/health", expect = 200 }}
timeout = "20s"
interval = "100ms"

[[field]]
id = "port"
type = "port"
default = {port}
bind.port = "public"

[[field]]
id = "extra_env"
label = "Extra environment"
type = "kv_lines"
default = ""
bind.env_lines = {{ protect = ["PROTECTED"] }}

[[field]]
id = "scratch"
label = "Scratch folder"
type = "dir"
share = "never"
default = ""
"#,
            run = run.display()
        ),
    )
    .unwrap();
    dir
}

fn env_configuration(world: &World, body: &str) -> PathBuf {
    let path = world.scratch.join("envprog.toml");
    std::fs::write(
        &path,
        format!("schema = 1\nkind = \"profile\"\nid = \"envprog\"\nname = \"envprog\"\nengine = \"envprog\"\n{body}"),
    )
    .unwrap();
    path
}

fn run_options(world: &World, registry_id: &str) -> Options {
    let registry = crate::config::build_registry(&world.dirs());
    let hash =
        quickai_core::guard::document_hash(registry.engine(registry_id).expect("the engine"));
    Options {
        confirmed: vec![hash],
        accept_review: true,
        ..world.options()
    }
}

/// Y40: what belongs to the computer is said in a file, with `--set` and with `--env`, and reaches the program:
/// a setting of the file replaces the configuration's, the lines of environment follow the configuration's own
/// and reach the program's environment, the names the document keeps for itself cannot be set from here, and a
/// value that does not fit its setting is a mistake that names where it was said.
#[test]
fn p3_y40_what_belongs_to_the_computer_reaches_the_program() {
    let world = World::new("y40");
    let port = free_port();
    env_engine(&world, free_port());
    let configuration = env_configuration(
        &world,
        &format!(
            "\n[values.envprog]\nport = {}\nextra_env = \"FROM_CONFIGURATION=1\"\n",
            free_port()
        ),
    );
    let environment = world.scratch.join("server.toml");
    std::fs::write(
        &environment,
        format!(
            "schema = 1\n[values.envprog]\nport = {port}\nscratch = \"/mnt/big/scratch\"\n\n[env.envprog]\nFROM_FILE = \"two\"\nSECRET_KEY = \"s3cr3t\"\n"
        ),
    )
    .unwrap();
    let options = Options {
        environment_files: vec![environment.clone()],
        sets: vec![],
        env_lines: vec!["FROM_FLAG=three".into(), "PROTECTED=nope".into()],
        ..run_options(&world, "envprog")
    };
    let run = super::headless::Run::start(Request {
        dirs: world.dirs(),
        target: configuration.display().to_string(),
        options,
    });
    run.ready();
    let argv_dir = world.scratch.join("envprog");
    let env = std::fs::read_to_string(argv_dir.join("env.txt")).unwrap();
    let argv = std::fs::read_to_string(argv_dir.join("argv.txt")).unwrap();
    assert!(
        argv.contains(&port.to_string()),
        "the file's port was used: {argv}"
    );
    for line in [
        "FROM_CONFIGURATION=1",
        "FROM_FILE=two",
        "SECRET_KEY=s3cr3t",
        "FROM_FLAG=three",
    ] {
        assert!(env.lines().any(|l| l == line), "{line} in\n{env}");
    }
    assert!(
        !env.lines().any(|l| l == "PROTECTED=nope"),
        "a name the document keeps for itself cannot be set from here:\n{env}"
    );
    let (outcome, events) = run.stop();
    assert_eq!(outcome, Outcome::Stopped);
    // what was said is told, where it was said, and a secret is not shown
    let told = events
        .iter()
        .find_map(|e| match e {
            Event::Environment { items } => Some(items.clone()),
            _ => None,
        })
        .expect("what was said is told");
    let line = |key: &str| {
        told.iter()
            .find(|i| i.key == key)
            .unwrap_or_else(|| panic!("{key} in {told:#?}"))
    };
    assert_eq!(
        line("values.envprog.port").value.as_deref(),
        Some(port.to_string().as_str())
    );
    assert!(line("values.envprog.port")
        .from
        .starts_with("environment file "));
    assert_eq!(line("env.envprog.SECRET_KEY").value, None);
    assert_eq!(line("env.envprog.FROM_FLAG").from, "--env");
    assert!(!format!("{events:?}").contains("s3cr3t"));

    // a value that does not fit is a mistake, and says where it was said
    let bad_file = world.scratch.join("bad.toml");
    std::fs::write(
        &bad_file,
        "[values.envprog]\nport = 99999999\n[env.nothing]\nA = \"1\"\n",
    )
    .unwrap();
    let (outcome, _) = world.up(
        &configuration,
        Options {
            environment_files: vec![bad_file.clone()],
            ..check(&world, true)
        },
    );
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Mistake);
    assert!(
        failure.message.contains(&bad_file.display().to_string())
            && failure.message.contains("values.envprog.port")
            && failure.message.contains("env.nothing"),
        "{}",
        failure.message
    );
    for (sets, env_lines, why) in [
        (
            vec!["envprog.port=eighty"],
            vec![],
            "--set envprog.port=eighty",
        ),
        (
            vec!["envprog.nothing=1"],
            vec![],
            "has no setting called `nothing`",
        ),
        (vec!["nothing"], vec![], "it is `<engine>.<field>=<value>`"),
        (vec![], vec!["1BAD=x"], "is not the name of a variable"),
    ] {
        let (outcome, _) = world.up(
            &configuration,
            Options {
                sets: sets.iter().map(ToString::to_string).collect(),
                env_lines: env_lines.iter().map(ToString::to_string).collect(),
                ..check(&world, true)
            },
        );
        let failure = failure_of(outcome);
        assert_eq!(failure.kind, FailureKind::Mistake);
        assert!(failure.message.contains(why), "{why}: {}", failure.message);
    }
    // and a file that is not there
    let (outcome, _) = world.up(
        &configuration,
        Options {
            environment_files: vec![world.scratch.join("missing.toml")],
            ..check(&world, true)
        },
    );
    assert!(failure_of(outcome).message.contains("could not be read"));
}

/// Y41: a configuration that needs something nobody gave stops before anything is fetched or installed, says
/// each thing with why and how to give it, and goes on when each is given by any way: the file, `--set`,
/// `--env`, a login; a plan (`check`) says it too.
#[test]
fn p3_y41_a_configuration_that_needs_something_stops_before_anything_is_fetched() {
    let world = World::new("y41");
    env_engine(&world, free_port());
    let configuration = env_configuration(
        &world,
        &format!(
            r#"
[[model]]
file = "Gated.gguf"
from = "huggingface:{REPO}"

[[needs]]
token = "huggingface"
why = "The repository asks you to accept its licence."

[[needs]]
setting = "envprog.scratch"
why = "A disk with 30 GB free."

[[needs]]
env = "API_SECRET"
why = "Signs the logins."

[values.envprog]
port = {}
"#,
            free_port()
        ),
    );
    let (outcome, events) = world.up(&configuration, run_options(&world, "envprog"));
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Values);
    assert_eq!(provision::Outcome::Failed(failure.clone()).exit_code(), 5);
    for text in [
        "a Hugging Face token",
        "The repository asks you to accept its licence.",
        "--hf-token, the variable HF_TOKEN",
        "envprog.scratch",
        "A disk with 30 GB free.",
        "--set envprog.scratch=VALUE",
        "API_SECRET (for envprog)",
        "--env envprog:API_SECRET=VALUE",
        "quickai-run init",
    ] {
        assert!(
            failure.message.contains(text),
            "{text} in\n{}",
            failure.message
        );
    }
    assert!(plan_steps(&events).is_empty(), "no plan was made");
    assert_eq!(world.files.requests(), 0);
    assert!(world.github.requests().is_empty());
    assert!(!world.data.exists());
    // a plan says it too
    let (outcome, _) = world.up(
        &configuration,
        Options {
            dry_run: true,
            ..run_options(&world, "envprog")
        },
    );
    assert_eq!(failure_of(outcome).kind, FailureKind::Values);

    // each is given in a way of its own
    let file = world.scratch.join("given.toml");
    std::fs::write(&file, "[fetch]\nhuggingface_token = \"hf_x\"\n").unwrap();
    let (outcome, events) = world.up(
        &configuration,
        Options {
            environment_files: vec![file.clone()],
            sets: vec!["envprog.scratch=/mnt/x".into()],
            env_lines: vec!["API_SECRET=abc".into()],
            ..Options {
                dry_run: true,
                ..run_options(&world, "envprog")
            }
        },
    );
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
    // the words of their own count as given: the folder of the models, the port and the Python
    let own = world.file(
        "own-words",
        "[[needs]]\nsetting = \"llamacpp.models_dir\"\n\n[[needs]]\nsetting = \"llamacpp.port\"\n",
    );
    let (outcome, _) = world.up(&own, check(&world, true));
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Values);
    assert!(
        failure.message.contains("llamacpp.models_dir"),
        "{}",
        failure.message
    );
    let (outcome, events) = world.up(
        &own,
        Options {
            models_dir: Some(world.scratch.join("big")),
            port: Some(free_port()),
            ..check(&world, true)
        },
    );
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");

    // one short of all of them: the one that is left is the only one said
    let (outcome, _) = world.up(
        &configuration,
        Options {
            environment_files: vec![file],
            sets: vec!["envprog.scratch=/mnt/x".into()],
            ..Options {
                dry_run: true,
                ..run_options(&world, "envprog")
            }
        },
    );
    let failure = failure_of(outcome);
    assert!(failure.message.contains("API_SECRET") && !failure.message.contains("envprog.scratch"));
    assert!(!failure.message.contains("a Hugging Face token"));
}

/// Y42: a login or a secret is in no sentence, no event, no JSON line the program says, whatever it is given
/// by; what was said is told, with where, and the secret as hidden.
#[test]
fn p3_y42_a_secret_is_in_nothing_the_program_says() {
    let world = World::new("y42");
    env_engine(&world, free_port());
    let configuration = env_configuration(
        &world,
        &format!("\n[values.envprog]\nport = {}\n", free_port()),
    );
    let file = world.scratch.join("secrets.toml");
    std::fs::write(
        &file,
        "[fetch]\nhuggingface_token = \"hf_FILE_TOKEN\"\ngithub_token = \"ghp_FILE_TOKEN\"\n[env.envprog]\nAPI_SECRET = \"file-secret-value\"\nPLAIN = \"visible\"\n",
    )
    .unwrap();
    let (outcome, events) = world.up(
        &configuration,
        Options {
            environment_files: vec![file],
            env_lines: vec!["OTHER_PASSWORD=flag-secret-value".into()],
            fetch: say_login("hf_FLAG_TOKEN"),
            dry_run: true,
            ..run_options(&world, "envprog")
        },
    );
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
    let said = serde_json::to_string(&events).unwrap() + &format!("{events:?}");
    for secret in [
        "hf_FILE_TOKEN",
        "ghp_FILE_TOKEN",
        "file-secret-value",
        "flag-secret-value",
        "hf_FLAG_TOKEN",
    ] {
        assert!(!said.contains(secret), "{secret} in {said}");
    }
    let Some(Event::Environment { items }) = events
        .iter()
        .find(|e| matches!(e, Event::Environment { .. }))
    else {
        panic!("what was said is told: {events:#?}")
    };
    let by_key: BTreeMap<&str, Option<&str>> = items
        .iter()
        .map(|i| (i.key.as_str(), i.value.as_deref()))
        .collect();
    assert_eq!(by_key["fetch.huggingface_token"], None);
    assert_eq!(by_key["fetch.github_token"], None);
    assert_eq!(by_key["env.envprog.API_SECRET"], None);
    assert_eq!(by_key["env.envprog.OTHER_PASSWORD"], None);
    assert_eq!(by_key["env.envprog.PLAIN"], Some("visible"));
    let origin = |key: &str| items.iter().find(|i| i.key == key).unwrap().from.clone();
    assert_eq!(
        origin("fetch.huggingface_token"),
        "--hf-token",
        "a flag wins over a file"
    );
    assert_eq!(
        origin("fetch.github_token").split(' ').next(),
        Some("environment")
    );
    // the options print without it as well
    let options = Options {
        fetch: say_login("hf_DEBUG_TOKEN"),
        ..Options::default()
    };
    assert!(!format!("{options:?}").contains("hf_DEBUG_TOKEN"));
}

/// Y43: the file to fill in is made for a configuration without a network and without a disk of models: what it
/// needs, what belongs to a computer, and each file it fetches with where from and how big; a configuration
/// that is not there is a sentence.
#[test]
fn p3_y43_the_file_to_fill_in_is_made_from_the_configuration_and_the_documents() {
    let world = World::new("y43");
    let file = llama(
        &world,
        "fill",
        &format!(
            "[[model]]\ncatalog = \"alpha-4b\"\nfile = \"{FILE}\"\n\n[[model]]\nfile = \"Hinted.gguf\"\nfrom = \"huggingface:{REPO}\"\npath = \"deep/Hinted.gguf\"\nsize = 1500000000\n\n[[needs]]\ntoken = \"huggingface\"\nwhy = \"Accept the licence first.\"\n"
        ),
    );
    let text = provision::init(&Request {
        dirs: world.dirs(),
        target: file.display().to_string(),
        options: Options::default(),
    })
    .expect("a template");
    for line in [
        "huggingface_token = \"\"",
        "NEEDED by this configuration: Accept the licence first.",
        "[values.llamacpp]",
        "# models_dir = \"\"",
        "# bin_override = \"\"",
        "# extra_args = \"\"",
        &format!("# \"{FILE}\" = \"\""),
        "# \"mmproj-Alpha-4B-F16.gguf\" = \"\"",
        "# \"Hinted.gguf\" = \"\"      # from huggingface:acme/Alpha-4B-GGUF (deep/Hinted.gguf) (1.4 GB)",
    ] {
        // the projector of a vision model is kept under a longer name; the line above names the weights
        if line.contains("mmproj-") {
            continue;
        }
        assert!(text.contains(line), "{line} in\n{text}");
    }
    assert!(
        text.contains("acme--Alpha-4B-GGUF--mmproj-Alpha-4B-F16.gguf"),
        "{text}"
    );
    assert_eq!(world.files.requests(), 0);
    assert!(world.github.requests().is_empty());
    assert!(!world.data.exists(), "nothing was written");
    // it is a file that can be read back as it stands, and says nothing yet
    let registry = crate::config::build_registry(&world.dirs());
    let (read_back, unknown) = quickai_core::profile::environment::Environment::read(
        &text,
        "the template",
        registry.engine_map(),
        "llamacpp",
    )
    .expect("a file that can be read");
    assert!(unknown.is_empty(), "{unknown:?}");
    assert!(read_back.fetch.huggingface_token.is_none() && read_back.values.is_empty());

    let failure = provision::init(&Request {
        dirs: world.dirs(),
        target: "no-such-configuration".into(),
        options: Options::default(),
    })
    .expect_err("there is none");
    assert_eq!(failure.kind, FailureKind::Unreadable);

    // a configuration that is its own way of making a mistake: a place the engine cannot keep a file in
    let flat = llama(
        &world,
        "flat",
        "[[model]]\nfile = \"x.gguf\"\nurl = \"http://127.0.0.1:1/x\"\ninto = \"vae\"\n",
    );
    let failure = provision::init(&Request {
        dirs: world.dirs(),
        target: flat.display().to_string(),
        options: Options::default(),
    })
    .expect_err("it cannot be filled in");
    assert!(
        failure.message.contains("one folder"),
        "{}",
        failure.message
    );
}

// ---------------------------------------------------------------------------------------------- Y50

/// Y50: the build of a program that goes with the engine is the configuration's to pin (`[runtime.<program>]`)
/// and the person's to override (`--build program=label`); a pin that is not installed and cannot be installed
/// is said with the label, and a configuration with no pin uses the program it finds.
#[test]
fn p3_y50_the_build_of_a_program_that_goes_with_the_engine_can_be_pinned() {
    let root = crate::testkit::scratch("y50");
    let config = root.join("config");
    std::fs::create_dir_all(config.join("engines")).unwrap();
    let (main_port, side_port) = (free_port(), free_port());
    let main_run = super::headless::program(&root, "main");
    let side_run = super::headless::program(&root, "side");
    std::fs::write(
        config.join("engines/pairmain.toml"),
        super::headless::pair_document("pairmain", &main_run, main_port, None),
    )
    .unwrap();
    std::fs::write(
        config.join("engines/pairside.toml"),
        super::headless::pair_document("pairside", &side_run, side_port, Some("pairmain")),
    )
    .unwrap();
    let dirs = ConfigDirs {
        user: Some(config),
        data: Some(root.join("data")),
        ..ConfigDirs::default()
    };
    let registry = crate::config::build_registry(&dirs);
    let confirmed: Vec<String> = ["pairmain", "pairside"]
        .iter()
        .map(|id| quickai_core::guard::document_hash(registry.engine(id).expect("the engine")))
        .collect();
    let file = |name: &str, pin: &str| {
        let path = root.join(name);
        std::fs::write(
            &path,
            format!(
                "schema = 1\nkind = \"profile\"\nid = \"{name}\"\nname = \"{name}\"\nengine = \"pairmain\"\n\n{pin}[values.pairmain]\nport = {main_port}\n\n[values.pairside]\nenabled = true\nport = {side_port}\n"
            ),
        )
        .unwrap();
        path
    };
    let run = |target: &Path, companion_builds: &[(&str, &str)]| {
        let mut events = Vec::new();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let outcome = provision::up(
            &Request {
                dirs: dirs.clone(),
                target: target.display().to_string(),
                options: Options {
                    dry_run: true,
                    gpus: Some(Vec::new()),
                    confirmed: confirmed.clone(),
                    companion_builds: companion_builds
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                    ..Options::default()
                },
            },
            &mut |e| events.push(e),
            &stop,
        );
        (outcome, events)
    };
    // no pin: the program it finds
    let (outcome, events) = run(&file("unpinned", ""), &[]);
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
    assert!(plan_steps(&events).iter().any(|s| s.kind == "companion"));
    // a pin that cannot be met: the label is in the sentence
    let pinned = file("pinned", "[runtime.pairside]\nbuild = \"9.9.9\"\n\n");
    let (outcome, _) = run(&pinned, &[]);
    let failure = failure_of(outcome);
    assert_eq!(failure.kind, FailureKind::Needs);
    assert!(
        failure.message.contains("pairside") && failure.message.contains("9.9.9"),
        "{}",
        failure.message
    );
    // what the person says wins over the pin
    let (outcome, _) = run(&pinned, &[("pairside", "8.8.8")]);
    let failure = failure_of(outcome);
    assert!(
        failure.message.contains("8.8.8") && !failure.message.contains("9.9.9"),
        "{}",
        failure.message
    );
    // and a pin for a program that is not switched on is no matter
    let off = root.join("off");
    std::fs::write(
        &off,
        format!("schema = 1\nkind = \"profile\"\nid = \"off\"\nname = \"off\"\nengine = \"pairmain\"\n\n[runtime.pairside]\nbuild = \"9.9.9\"\n\n[values.pairmain]\nport = {main_port}\n"),
    )
    .unwrap();
    let (outcome, events) = run(&off, &[]);
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
}

/// Y51: a program the person names themselves (`--set <engine>.bin_override=`) is the one that is used, with
/// nothing installed; one that is not a file is told.
#[test]
fn p3_y51_a_program_the_person_names_is_the_one_that_is_used() {
    let world = World::new("y51");
    let own = launcher(&world);
    let file = llama(&world, "mine", "");
    let (outcome, events) = world.up(
        &file,
        Options {
            sets: vec![format!("llamacpp.bin_override={}", own.display())],
            ..check(&world, true)
        },
    );
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
    let kinds: Vec<String> = plan_steps(&events).iter().map(|s| s.kind.clone()).collect();
    assert_eq!(kinds, ["start"], "no install: {events:#?}");
    assert!(
        world.github.requests().is_empty(),
        "and no list of builds was asked for"
    );
    let (outcome, _) = world.up(
        &file,
        Options {
            sets: vec!["llamacpp.bin_override=/nowhere/llama-server".into()],
            ..check(&world, true)
        },
    );
    let failure = failure_of(outcome);
    assert!(
        failure.message.contains("is not a file"),
        "{}",
        failure.message
    );
}

/// Y69: when no Python is found the installer's sentence says what to install and where to name the one there is:
/// a box in the window's Settings. The program with no window says the flag instead, and every other sentence of an
/// installer is passed on as it was. Found by the run for real on a computer whose Python 3.12 is behind a shim
/// (docs/headless.md, section 11).
#[test]
fn p3_y69_a_missing_python_is_told_with_the_flag_of_the_command_line_not_the_box_of_the_window() {
    use crate::prims::install::{find_python, Where};
    let nowhere = Where {
        path_var: Some(std::ffi::OsString::new()),
        home: None,
    };
    let versions: Vec<String> = vec!["3.94".into(), "3.93".into()];
    let window = find_python(&versions, None, &nowhere).unwrap_err();
    assert!(window.contains("Python box of Settings"), "{window}");
    let said = provision::in_the_words_of_the_command_line(&window);
    assert!(said.contains("no Python 3.94 or 3.93 was found"), "{said}");
    assert!(
        said.contains("python3.12-venv"),
        "what to install is still said: {said}"
    );
    assert!(
        said.ends_with("or name the Python you have with --python PATH"),
        "{said}"
    );
    assert!(!said.contains("Settings"), "{said}");
    // the other things an install says are not touched
    let pip = "ERROR: No matching distribution found for torch";
    assert_eq!(provision::in_the_words_of_the_command_line(pip), pip);
}
