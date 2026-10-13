//! Group O of docs/phase3-test-plan.md, the program itself (work package 4.18, docs/headless.md): the real
//! `quickai-run` binary, run as a person or a service manager runs it, in a home of its own, against the
//! world of the runtime's tests (a fake GitHub, a file server, a stand-in `llama-server`). What the library
//! proves of the steps, this proves of the command line around them: the words, the files, the signals and
//! the codes it exits with.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use quickai_runtime::testkit::headless_world::{World, FILE};
use quickai_runtime::testkit::http::try_get;
use quickai_runtime::testkit::wait_until;

/// The program, with a home of its own and the documents of the world, and nothing of this machine's.
fn quickai_run(world: &World, args: &[&str]) -> Command {
    let home = world.scratch.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_quickai-run"));
    command
        .args(args)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("QUICKAI_CONFIG_DIR", &world.config)
        .stdin(Stdio::null());
    command
}

fn home(world: &World) -> PathBuf {
    world.scratch.join("home")
}

struct Finished {
    code: i32,
    out: String,
    err: String,
}

fn finish(mut command: Command) -> Finished {
    let output = command.output().expect("the program starts");
    Finished {
        code: output.status.code().expect("it ended by itself"),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// O34: it says what it is, refuses what it does not know with the usage and a code of 2, and a word it
/// shares with QuickAI's other headless commands is theirs.
#[test]
fn p3_o34_it_says_what_it_does_and_refuses_what_it_does_not_know() {
    let world = World::new("o34");
    let help = finish(quickai_run(&world, &["--help"]));
    assert_eq!(help.code, 0);
    for word in [
        "up <configuration>",
        "prepare",
        "check",
        "--accept-review",
        "--gpu",
        "--ready-file",
        "127.0.0.1",
    ] {
        assert!(help.out.contains(word), "{word} in {}", help.out);
    }
    let version = finish(quickai_run(&world, &["--version"]));
    assert_eq!(version.code, 0);
    assert!(version.out.starts_with("quickai-run 0."), "{}", version.out);

    for (args, why) in [
        (vec![], "say what to do"),
        (vec!["frobnicate"], "`frobnicate`"),
        (vec!["up"], "needs a configuration"),
        (vec!["up", "x", "--port", "nope"], "1 to 65535"),
        (vec!["up", "x", "--gpu", "voodoo"], "voodoo"),
    ] {
        let run = finish(quickai_run(&world, &args));
        assert_eq!(run.code, 2, "{args:?}: {}", run.err);
        assert!(run.err.contains(why), "{args:?}: {}", run.err);
        assert!(
            run.err.contains("quickai-run up <configuration>"),
            "the usage follows"
        );
        assert!(run.out.is_empty());
    }
    // the words QuickAI's other headless commands have are theirs
    let check = finish(quickai_run(
        &world,
        &["config", "check", "--strict", "--embedded"],
    ));
    assert_eq!(check.code, 0, "{}{}", check.out, check.err);
    assert_eq!(
        check.out,
        "config: 0 errors, 0 warnings; engines: llamacpp, ollama, comfyui; companions: openwebui\n"
    );
    let list = finish(quickai_run(&world, &["configuration", "list"]));
    assert_eq!(list.code, 0, "{}", list.err);
    assert!(list.out.contains("default"), "{}", list.out);
    let plan = finish(quickai_run(
        &world,
        &["plan", "--engine", "comfyui", "--explain"],
    ));
    assert_eq!(plan.code, 0, "{}", plan.err);
    assert!(
        plan.out.contains("--listen") && plan.out.contains("127.0.0.1"),
        "{}",
        plan.out
    );
}

/// O35: `check` says what would be done, in sentences or as JSON lines, and leaves the home it ran in as
/// it found it.
#[test]
fn p3_o35_check_says_what_would_be_done_and_leaves_the_home_as_it_was() {
    let world = World::new("o35");
    let file = world.usual();
    let file = file.to_str().unwrap();
    let run = finish(quickai_run(&world, &["check", file, "--gpu", "none"]));
    assert_eq!(run.code, 0, "{}{}", run.out, run.err);
    assert!(run.out.contains("This computer:"), "{}", run.out);
    assert!(
        run.out.contains("no graphics card found") || run.out.contains("as you said"),
        "{}",
        run.out
    );
    for line in [
        "To do:",
        "  1. install llama.cpp b100-",
        "  2. fetch Alpha-4B-Q4_K_M.gguf and 1 more file",
        &format!("  3. start llama.cpp on 127.0.0.1:{}", world.port),
        "Nothing was changed.",
    ] {
        assert!(run.out.contains(line), "{line:?} in\n{}", run.out);
    }
    assert!(
        std::fs::read_dir(home(&world)).unwrap().next().is_none(),
        "the home was left as it was found: {:?}",
        std::fs::read_dir(home(&world)).unwrap().collect::<Vec<_>>()
    );

    // the same as lines a script can read
    let json = finish(quickai_run(
        &world,
        &["check", file, "--gpu", "none", "--json"],
    ));
    assert_eq!(json.code, 0);
    let events: Vec<serde_json::Value> = json
        .out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect();
    let names: Vec<&str> = events
        .iter()
        .map(|e| e["event"].as_str().unwrap())
        .collect();
    assert_eq!(names.first(), Some(&"computer"));
    assert!(names.contains(&"plan"));
    assert_eq!(names.last(), Some(&"finished"));
    assert_eq!(events.last().unwrap()["outcome"], "planned");
    let plan = events.iter().find(|e| e["event"] == "plan").unwrap();
    assert_eq!(plan["steps"].as_array().unwrap().len(), 3);

    // --quiet says the plan and the end
    let quiet = finish(quickai_run(&world, &["check", file, "--gpu", "none", "-q"]));
    assert!(
        quiet.out.contains("To do:") && !quiet.out.contains("This computer:"),
        "{}",
        quiet.out
    );
}

/// O36: `up` as a service manager runs it: the plan, the installs and the fetches, the engine started on its
/// model, the address written to the ready file, the engine answering; and a SIGTERM ends it with a code of 0,
/// the ready file gone and no program left.
#[test]
fn p3_o36_up_runs_until_it_is_told_to_stop_and_leaves_nothing() {
    let world = World::new("o36");
    let ready_file = world.scratch.join("ready.json");
    let log = world.scratch.join("out.txt");
    let file = world.usual();
    let mut child: Child = quickai_run(
        &world,
        &[
            "up",
            file.to_str().unwrap(),
            "--gpu",
            "none",
            "--ready-file",
            ready_file.to_str().unwrap(),
        ],
    )
    .stdout(std::fs::File::create(&log).unwrap())
    .stderr(Stdio::inherit())
    .spawn()
    .expect("the program starts");

    wait_until(Duration::from_secs(90), "the ready file", || {
        ready_file.is_file()
    });
    let ready: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&ready_file).unwrap()).unwrap();
    assert_eq!(ready["model"], FILE);
    assert_eq!(ready["pid"], child.id());
    assert_eq!(
        ready["endpoints"][0]["url"],
        format!("http://127.0.0.1:{}", world.port)
    );
    assert_eq!(try_get(&world.url("/health")).unwrap().status, 200);
    assert!(
        home(&world).join(".quickai/models").join(FILE).is_file(),
        "the model was fetched into the home the program ran in"
    );
    assert!(home(&world).join(".quickai/llama-bin").is_dir());

    // a service manager's stop
    assert!(Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap()
        .success());
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "it did not stop");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(0));
    assert!(!ready_file.exists(), "a ready file means ready now");
    wait_until(Duration::from_secs(10), "the engine to be gone", || {
        try_get(&world.url("/health")).is_err()
    });
    let said = std::fs::read_to_string(&log).unwrap();
    for part in [
        "To do:",
        "[1/2] install llama.cpp",
        "[2/2] fetch",
        "Ready.",
        "Stopping...",
        "Stopped.",
    ] {
        assert!(said.contains(part), "{part:?} in\n{said}");
    }
}

/// O37: what went wrong is said on the error stream in a sentence, and the code says which kind it was.
#[test]
fn p3_o37_what_went_wrong_is_a_sentence_and_a_code() {
    let world = World::new("o37");
    // a file that is not there
    let missing = finish(quickai_run(&world, &["up", "/no/such/file.toml"]));
    assert_eq!(missing.code, 1);
    assert!(
        missing.err.contains("quickai-run: ") && missing.err.contains("no such file"),
        "{}",
        missing.err
    );

    // a file that asks, and was not told yes
    let asking = world.file(
        "asking",
        &format!(
            "[[model]]\ncatalog = \"alpha-4b\"\nfile = \"{FILE}\"\n\n[values.llamacpp]\nextra_args = \"--flash-attn\"\n"
        ),
    );
    let run = finish(quickai_run(
        &world,
        &["check", asking.to_str().unwrap(), "--gpu", "none"],
    ));
    assert_eq!(run.code, 3, "{}{}", run.out, run.err);
    assert!(run.err.contains("--accept-review"), "{}", run.err);
    let accepted = finish(quickai_run(
        &world,
        &[
            "check",
            asking.to_str().unwrap(),
            "--gpu",
            "none",
            "--accept-review",
        ],
    ));
    assert_eq!(accepted.code, 0, "{}{}", accepted.out, accepted.err);

    // nothing in either run touched the network or the disk
    assert_eq!(world.files.requests(), 0);
    assert!(std::fs::read_dir(home(&world)).unwrap().next().is_none());
}

/// O38: it needs no window: the binary links no library of any toolkit, and so starts on a server that has
/// no display, no GL and no fonts.
#[test]
fn p3_o38_the_binary_links_no_toolkit() {
    let output = Command::new("ldd")
        .arg(env!("CARGO_BIN_EXE_quickai-run"))
        .output();
    let Ok(output) = output else {
        eprintln!("no ldd on this machine: not checked");
        return;
    };
    let linked = String::from_utf8_lossy(&output.stdout).to_string();
    for toolkit in [
        "libX11",
        "libxcb",
        "libwayland",
        "libGL",
        "libEGL",
        "libxkbcommon",
        "libgtk",
        "libvulkan",
    ] {
        assert!(!linked.contains(toolkit), "{toolkit} is linked:\n{linked}");
    }
    assert!(Path::new(env!("CARGO_BIN_EXE_quickai-run")).is_file());
}

// ---------------------------------------------------------------------------------------------------- Y63-Y66

use quickai_runtime::testkit::headless_world::Hub;

/// A configuration file that needs a login, a setting and a variable of an environment, for the program.
fn needy(world: &World) -> PathBuf {
    world.file(
        "needy",
        &format!(
            "[[model]]\ncatalog = \"alpha-4b\"\nfile = \"{FILE}\"\n\n[[needs]]\ntoken = \"huggingface\"\nwhy = \"The repository asks you to accept its licence.\"\n\n[[needs]]\nsetting = \"llamacpp.models_dir\"\nwhy = \"A disk with room.\"\n\n[values.llamacpp]\nport = {}\n",
            world.port
        ),
    )
}

/// Y63: `init` writes the file to fill in next to the configuration, readable by the person only; it does
/// not write over a file that is there unless it is told to, can print instead of write, and says what it did
/// not understand as a sentence.
#[test]
fn p3_y63_init_writes_the_file_to_fill_in_and_keeps_it_private() {
    use std::os::unix::fs::PermissionsExt;
    let world = World::new("y63");
    let file = needy(&world);
    let written = file.with_file_name("needy.environment.toml");
    let first = finish(quickai_run(&world, &["init", file.to_str().unwrap()]));
    assert_eq!(first.code, 0, "{}{}", first.out, first.err);
    assert!(
        first.out.contains(&format!("Wrote {}", written.display())),
        "{}",
        first.out
    );
    assert!(
        first.out.contains(&format!(
            "quickai-run check {} --environment {}",
            file.display(),
            written.display()
        )),
        "{}",
        first.out
    );
    let text = std::fs::read_to_string(&written).unwrap();
    for part in [
        "huggingface_token = \"\"",
        "NEEDED by this configuration: The repository asks you to accept its licence.",
        "\nmodels_dir = \"\"\n",
        "NEEDED by this configuration: A disk with room.",
        "[values.llamacpp]",
        "[models]",
    ] {
        assert!(text.contains(part), "{part} in\n{text}");
    }
    assert_eq!(
        std::fs::metadata(&written).unwrap().permissions().mode() & 0o777,
        0o600,
        "a file that may hold a login is the person's alone"
    );
    // it is not written over
    std::fs::write(&written, "# mine\n").unwrap();
    let again = finish(quickai_run(&world, &["init", file.to_str().unwrap()]));
    assert_eq!(again.code, 2);
    assert!(again.err.contains("--force"), "{}", again.err);
    assert_eq!(std::fs::read_to_string(&written).unwrap(), "# mine\n");
    // (the file that was there is open to everyone, as one written by hand is)
    std::fs::set_permissions(&written, std::fs::Permissions::from_mode(0o644)).unwrap();
    let forced = finish(quickai_run(
        &world,
        &["init", file.to_str().unwrap(), "--force"],
    ));
    assert_eq!(forced.code, 0, "{}", forced.err);
    assert!(std::fs::read_to_string(&written)
        .unwrap()
        .contains("NEEDED"));
    assert_eq!(
        std::fs::metadata(&written).unwrap().permissions().mode() & 0o777,
        0o600,
        "a file that is written over is made private too"
    );
    // a file somewhere else, and the standard output
    let elsewhere = world.scratch.join("elsewhere.toml");
    let to_file = finish(quickai_run(
        &world,
        &[
            "init",
            file.to_str().unwrap(),
            "-o",
            elsewhere.to_str().unwrap(),
        ],
    ));
    assert_eq!(to_file.code, 0, "{}", to_file.err);
    assert!(elsewhere.is_file());
    std::fs::remove_file(&elsewhere).unwrap();
    let printed = finish(quickai_run(
        &world,
        &["init", file.to_str().unwrap(), "-o", "-"],
    ));
    assert_eq!(printed.code, 0);
    assert!(printed.out.contains("[fetch]") && !printed.out.contains("Wrote"));
    assert!(!elsewhere.exists());
    // a configuration of the person's, by id, writes in the folder it is run in
    let missing = finish(quickai_run(&world, &["init", "no-such-configuration"]));
    assert_eq!(missing.code, 1);
    assert!(missing.err.starts_with("quickai-run: "), "{}", missing.err);
    // and nothing was fetched or installed
    assert_eq!(world.files.requests(), 0);
    assert!(std::fs::read_dir(home(&world)).unwrap().next().is_none());
}

/// Y64: a configuration that needs something stops with a code of its own, 5, and says each thing and how to
/// give it, before anything is fetched; the file `init` wrote, filled in and given back, lets it go on; a word
/// that makes no sense is 2.
#[test]
fn p3_y64_what_the_configuration_needs_is_a_code_of_its_own_and_the_file_to_fill_in_answers_it() {
    let world = World::new("y64");
    let file = needy(&world);
    let run = finish(quickai_run(
        &world,
        &["check", file.to_str().unwrap(), "--gpu", "none"],
    ));
    assert_eq!(run.code, 5, "{}{}", run.out, run.err);
    for part in [
        "this configuration needs something from you",
        "a Hugging Face token",
        "The repository asks you to accept its licence.",
        "--hf-token, the variable HF_TOKEN",
        "llamacpp.models_dir",
        "--set llamacpp.models_dir=VALUE",
        "quickai-run init",
    ] {
        assert!(run.err.contains(part), "{part} in\n{}", run.err);
    }
    assert!(
        run.out.is_empty() || !run.out.contains("To do:"),
        "no plan: {}",
        run.out
    );
    assert_eq!(world.files.requests(), 0);

    // the file to fill in
    let env_file = world.scratch.join("env.toml");
    let made = finish(quickai_run(
        &world,
        &[
            "init",
            file.to_str().unwrap(),
            "-o",
            env_file.to_str().unwrap(),
        ],
    ));
    assert_eq!(made.code, 0, "{}", made.err);
    let blank = finish(quickai_run(
        &world,
        &[
            "check",
            file.to_str().unwrap(),
            "--gpu",
            "none",
            "--environment",
            env_file.to_str().unwrap(),
        ],
    ));
    assert_eq!(
        blank.code, 5,
        "left blank it is not given: {}{}",
        blank.out, blank.err
    );
    let filled = std::fs::read_to_string(&env_file)
        .unwrap()
        .replace(
            "\nhuggingface_token = \"\"\n",
            "\nhuggingface_token = \"hf_filled\"\n",
        )
        .replace(
            "\nmodels_dir = \"\"\n",
            &format!(
                "\nmodels_dir = \"{}\"\n",
                world.scratch.join("big").display()
            ),
        );
    std::fs::write(&env_file, filled).unwrap();
    let ok = finish(quickai_run(
        &world,
        &[
            "check",
            file.to_str().unwrap(),
            "--gpu",
            "none",
            "--accept-review",
            "--environment",
            env_file.to_str().unwrap(),
        ],
    ));
    assert_eq!(ok.code, 0, "{}{}", ok.out, ok.err);
    assert!(
        ok.out.contains("What you said about this computer:"),
        "{}",
        ok.out
    );
    assert!(
        ok.out.contains("fetch.huggingface_token = (hidden)")
            && !ok.out.contains("hf_filled")
            && !ok.err.contains("hf_filled"),
        "{}",
        ok.out
    );
    // the model goes where the file said
    assert!(
        ok.out.contains(&format!(
            "{}",
            world.scratch.join("big").join(FILE).display()
        )),
        "{}",
        ok.out
    );
    // the same, said on the command line
    let flags = finish(quickai_run(
        &world,
        &[
            "check",
            file.to_str().unwrap(),
            "--gpu",
            "none",
            "--accept-review",
            "--hf-token",
            "hf_flag",
            "--set",
            &format!(
                "llamacpp.models_dir={}",
                world.scratch.join("big").display()
            ),
        ],
    ));
    assert_eq!(flags.code, 0, "{}{}", flags.out, flags.err);
    assert!(!flags.out.contains("hf_flag"));

    // a word that makes no sense
    let wrong = finish(quickai_run(
        &world,
        &[
            "check",
            file.to_str().unwrap(),
            "--gpu",
            "none",
            "--set",
            "llamacpp.port=eighty",
        ],
    ));
    assert_eq!(wrong.code, 2, "{}{}", wrong.out, wrong.err);
    assert!(
        wrong.err.contains("--set llamacpp.port=eighty"),
        "{}",
        wrong.err
    );
    assert!(std::fs::read_dir(home(&world)).unwrap().next().is_none());
}

/// Y65: the logins that every tool of the kind reads are read: `HF_TOKEN` reaches the hub as a bearer login
/// and is not on the command line, a flag wins over it, and neither the output, the JSON, the error stream nor
/// the ready file ever has it.
#[test]
fn p3_y65_the_usual_variables_are_read_and_a_login_is_in_nothing_that_is_said() {
    let world = World::new("y65");
    let hub = Hub::start();
    let model = quickai_runtime::testkit::headless_world::model_bytes("Gated.gguf");
    hub.add("/private/Repo/resolve/main/Gated.gguf", model);
    hub.require_login("/private/", "hf_flag_wins");
    // the models come from the hub, whose address the document is patched to
    let path = world.config.join("engines/llamacpp.toml");
    let patched = std::fs::read_to_string(&path)
        .unwrap()
        .replace(world.files.base(), &hub.base());
    std::fs::write(&path, patched).unwrap();
    let file = world.file(
        "gated",
        &format!(
            "[[model]]\nfile = \"Gated.gguf\"\nfrom = \"huggingface:private/Repo\"\n\n[values.llamacpp]\nport = {}\n",
            world.port
        ),
    );
    let stage = |args: &[&str], variable: Option<&str>| {
        let mut command = quickai_run(&world, args);
        if let Some(token) = variable {
            command.env("HF_TOKEN", token);
        }
        finish(command)
    };
    // the variable alone, which the hub does not accept
    let run = stage(
        &[
            "prepare",
            file.to_str().unwrap(),
            "--gpu",
            "none",
            "--accept-review",
            "--json",
        ],
        Some("hf_from_the_variable"),
    );
    assert_ne!(run.code, 0);
    assert!(
        run.err.contains("answered 403") || run.out.contains("answered 403"),
        "{}{}",
        run.out,
        run.err
    );
    for stream in [&run.out, &run.err] {
        assert!(!stream.contains("hf_from_the_variable"), "{stream}");
    }
    // and with the flag, which wins and which the hub accepts
    let run = stage(
        &[
            "prepare",
            file.to_str().unwrap(),
            "--gpu",
            "none",
            "--accept-review",
            "--json",
            "--hf-token",
            "hf_flag_wins",
        ],
        Some("hf_from_the_variable"),
    );
    assert_eq!(run.code, 0, "{}{}", run.out, run.err);
    for stream in [&run.out, &run.err] {
        assert!(
            !stream.contains("hf_flag_wins") && !stream.contains("hf_from_the_variable"),
            "{stream}"
        );
    }
    let events: Vec<serde_json::Value> = run
        .out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let said = events
        .iter()
        .find(|e| e["event"] == "environment")
        .expect("what was said is told");
    let token = said["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["key"] == "fetch.huggingface_token")
        .unwrap();
    assert!(
        token["value"].is_null() && token["from"] == "--hf-token",
        "{token}"
    );
    let asked = hub.asked();
    assert!(
        asked
            .iter()
            .any(|a| a.authorization.as_deref() == Some("Bearer hf_from_the_variable")),
        "the variable was sent first: {asked:?}"
    );
    assert_eq!(
        asked.last().unwrap().authorization.as_deref(),
        Some("Bearer hf_flag_wins"),
        "and the flag won over it the second time"
    );
    assert!(home(&world).join(".quickai/models/Gated.gguf").is_file());
}

/// Y66: offline, the program fetches nothing and lists what to bring, one file at a time, with the place it goes
/// and the address it would have come from, and a code of 1; what is brought, by any means, is used; a name the
/// configuration has no file by is a mistake, 2.
#[test]
fn p3_y66_offline_the_program_lists_what_to_bring_and_uses_what_is_brought() {
    let world = World::new("y66");
    let file = world.file(
        "bring",
        &format!(
            "[[model]]\nfile = \"Hinted.gguf\"\nfrom = \"huggingface:{}\"\n\n[values.llamacpp]\nport = {}\n",
            quickai_runtime::testkit::headless_world::REPO,
            world.port
        ),
    );
    let program = world.scratch.join("llama-server");
    std::fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let common = [
        "check",
        file.to_str().unwrap(),
        "--gpu",
        "none",
        "--accept-review",
        "--offline",
        "--set",
    ];
    let bin = format!("llamacpp.bin_override={}", program.display());
    let mut args: Vec<&str> = common.to_vec();
    args.push(&bin);
    let run = finish(quickai_run(&world, &args));
    assert_eq!(run.code, 1, "{}{}", run.out, run.err);
    let models = home(&world).join(".quickai/models/Hinted.gguf");
    for part in [
        "nothing is fetched",
        "Hinted.gguf",
        &models.display().to_string(),
        "/resolve/main/Hinted.gguf",
    ] {
        assert!(run.err.contains(part), "{part} in\n{}", run.err);
    }
    assert!(
        run.out.contains("To do:"),
        "the plan is said as well:\n{}",
        run.out
    );
    assert_eq!(world.files.requests(), 0);
    assert!(world.github.requests().is_empty());

    // brought by any means
    let brought = world.scratch.join("brought");
    std::fs::create_dir_all(&brought).unwrap();
    std::fs::write(
        brought.join("Hinted.gguf"),
        quickai_runtime::testkit::headless_world::model_bytes("Hinted.gguf"),
    )
    .unwrap();
    let from = brought.to_str().unwrap();
    let mut args: Vec<&str> = common.to_vec();
    args.push(&bin);
    args.extend(["--models-from", from]);
    let run = finish(quickai_run(&world, &args));
    assert_eq!(run.code, 0, "{}{}", run.out, run.err);
    assert!(
        run.out.contains("use Hinted.gguf") && run.out.contains("that you have"),
        "{}",
        run.out
    );
    assert_eq!(world.files.requests(), 0);

    // a name the configuration has no file by
    let mut args: Vec<&str> = common.to_vec();
    args.push(&bin);
    args.extend(["--model-source", "Typo.gguf=/x"]);
    let run = finish(quickai_run(&world, &args));
    assert_eq!(run.code, 2, "{}{}", run.out, run.err);
    assert!(
        run.err.contains("`Typo.gguf`") && run.err.contains("Hinted.gguf"),
        "{}",
        run.err
    );
}

/// Y67: the logins the run was given are for the fetching: the engine it starts is not given them, though it is
/// given the rest of the environment.
#[test]
fn p3_y67_what_the_run_starts_does_not_inherit_the_logins_it_was_given() {
    let world = World::new("y67");
    let file = world.usual();
    let ready_file = world.scratch.join("ready.json");
    let mut command = quickai_run(
        &world,
        &[
            "up",
            file.to_str().unwrap(),
            "--gpu",
            "none",
            "--ready-file",
            ready_file.to_str().unwrap(),
        ],
    );
    command
        .env("HF_TOKEN", "hf_in_the_environment_of_the_run")
        .env("HUGGING_FACE_HUB_TOKEN", "hf_the_older_name")
        .env("GITHUB_TOKEN", "ghp_in_the_environment_of_the_run")
        .env("GH_TOKEN", "ghp_the_other_name")
        .env("A_VARIABLE_OF_NOBODYS_SECRETS", "kept");
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the program starts");
    wait_until(Duration::from_secs(90), "the ready file", || {
        ready_file.is_file()
    });
    // the stand-in writes its environment beside itself
    let mut seen = String::new();
    fn walk(dir: &Path, seen: &mut String) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, seen);
                } else if path.file_name().is_some_and(|n| n == "env.txt") {
                    seen.push_str(&std::fs::read_to_string(&path).unwrap_or_default());
                }
            }
        }
    }
    walk(&home(&world).join(".quickai/llama-bin"), &mut seen);
    assert!(
        seen.contains("A_VARIABLE_OF_NOBODYS_SECRETS=kept"),
        "the rest is passed on:\n{seen}"
    );
    for login in [
        "HF_TOKEN",
        "HUGGING_FACE_HUB_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "hf_in_the_environment_of_the_run",
        "ghp_in_the_environment_of_the_run",
    ] {
        assert!(!seen.contains(login), "{login} reached the engine:\n{seen}");
    }
    assert!(Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap()
        .success());
    let _ = child.wait();
}

// ---- what the computer has to have (WP4.20) -------------------------------------------------------------

/// A program of a computer that is described: a shell script.
fn stand_in(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The folder that is the whole `PATH` of a run on a computer that has no compiler and has `apt-get` (which
/// installs one, and says what it was asked in `apt.log`): the programs the stand-ins use are linked in.
fn computer_without_a_compiler(world: &World) -> (PathBuf, PathBuf) {
    let bin = world.scratch.join("computer/bin");
    std::fs::create_dir_all(&bin).unwrap();
    for tool in ["sh", "env", "chmod", "printf", "true"] {
        for dir in ["/usr/bin", "/bin"] {
            let from = Path::new(dir).join(tool);
            if from.exists() {
                let _ = std::os::unix::fs::symlink(&from, bin.join(tool));
                break;
            }
        }
    }
    let log = world.scratch.join("computer/apt.log");
    stand_in(
        &bin,
        "apt-get",
        &format!(
            "echo \"$*\" >> '{log}'\nif [ \"$1\" = install ]; then printf '#!/bin/sh\\nexit 0\\n' > '{gcc}'; chmod +x '{gcc}'; fi",
            log = log.display(),
            gcc = bin.join("gcc").display(),
        ),
    );
    (bin, log)
}

/// A configuration of the world's engine that needs a C compiler, and the document that tells how to look for
/// one (the shipped documents name it only for ComfyUI; a patch of the world's can name it for the stand-in).
fn needing_a_compiler(world: &World) -> PathBuf {
    let patch = world.config.join("engines/llamacpp.toml");
    let mut text = std::fs::read_to_string(&patch).unwrap();
    text.push_str(
        r#"
[[prerequisite]]
id = "c_compiler"
label = "a C compiler"
check = { use = "program", any_of = ["gcc", "clang"], env = "CC" }
fix = { apt = ["build-essential"], dnf = ["gcc"] }
hint = "Install gcc or clang."
"#,
    );
    std::fs::write(&patch, text).unwrap();
    world.file(
        "needing",
        &format!(
            "[[model]]\ncatalog = \"alpha-4b\"\nfile = \"{FILE}\"\n\n[values.llamacpp]\nport = {}\n\n[[needs]]\nsystem = \"c_compiler\"\nwhy = \"Something the model runs through builds a helper in C.\"\n",
            world.port
        ),
    )
}

/// What a person running the tests types before a command that needs the administrator: nothing for the
/// administrator, `sudo ` for anyone else.
fn who_types_sudo() -> &'static str {
    use std::os::unix::fs::MetadataExt;
    let user = std::fs::metadata("/proc/self")
        .map(|m| m.uid())
        .unwrap_or(1000);
    if user == 0 {
        ""
    } else {
        "sudo "
    }
}

/// Y97: on a computer that lacks what the configuration needs, `check` says so in sentences and as JSON, with the
/// command that brings it, still says the plan, exits with 6, and leaves the home as it found it.
#[test]
fn p3_y97_check_exits_with_6_and_says_what_the_computer_lacks_and_the_command() {
    let world = World::new("y97");
    let file = needing_a_compiler(&world);
    let file = file.to_str().unwrap();
    let (bin, log) = computer_without_a_compiler(&world);
    let sudo = who_types_sudo();
    let mut command = quickai_run(&world, &["check", file, "--gpu", "none"]);
    command.env("PATH", &bin);
    let run = finish(command);
    assert_eq!(run.code, 6, "{}{}", run.out, run.err);
    for line in [
        "What this computer has to have:",
        "  MISSING a C compiler: none of gcc, clang is on the PATH",
        "          Something the model runs through builds a helper in C.",
        "          Install gcc or clang.",
        &format!("          bring it with: {sudo}apt-get update && {sudo}apt-get install -y build-essential"),
        "To do:",
        "  1. install llama.cpp b100-",
    ] {
        assert!(run.out.contains(line), "{line:?} in\n{}", run.out);
    }
    assert!(
        run.out.find("What this computer has to have:") < run.out.find("To do:"),
        "what the computer lacks is said before the plan:\n{}",
        run.out
    );
    assert!(
        run.err
            .contains("quickai-run: this computer lacks something the configuration needs:"),
        "{}",
        run.err
    );
    assert!(run.err.contains("`check` changes nothing"), "{}", run.err);
    assert!(!log.exists(), "nothing was installed");
    assert!(
        std::fs::read_dir(home(&world)).unwrap().next().is_none(),
        "the home was left as it was found"
    );

    // the same as lines a script can read: the look comes before the plan, the end is the failure
    let mut command = quickai_run(&world, &["check", file, "--gpu", "none", "--json"]);
    command.env("PATH", &bin);
    let json = finish(command);
    assert_eq!(json.code, 6, "{}{}", json.out, json.err);
    let events: Vec<serde_json::Value> = json
        .out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect();
    let names: Vec<&str> = events
        .iter()
        .map(|e| e["event"].as_str().unwrap())
        .collect();
    let look = names
        .iter()
        .position(|n| *n == "prerequisites")
        .expect("the look");
    let plan = names.iter().position(|n| *n == "plan").expect("the plan");
    assert!(look < plan, "{names:?}");
    let item = &events[look]["items"][0];
    assert_eq!(item["id"], "c_compiler");
    assert_eq!(item["state"], "missing");
    assert_eq!(
        item["fix"],
        format!("{sudo}apt-get update && {sudo}apt-get install -y build-essential")
    );
    let end = events.last().unwrap();
    assert_eq!(end["event"], "finished");
    assert_eq!(end["outcome"], "failed");

    // a quiet run says what is wrong and the plan
    let mut command = quickai_run(&world, &["check", file, "--gpu", "none", "-q"]);
    command.env("PATH", &bin);
    let quiet = finish(command);
    assert_eq!(quiet.code, 6);
    assert!(quiet.out.contains("MISSING a C compiler"), "{}", quiet.out);

    // and the variable the tool reads decides: a compiler it names that is there is a compiler
    let own = world.scratch.join("computer/my-cc");
    stand_in(own.parent().unwrap(), "my-cc", "exit 0");
    let mut command = quickai_run(&world, &["check", file, "--gpu", "none"]);
    command.env("PATH", &bin).env("CC", &own);
    let run = finish(command);
    assert_eq!(run.code, 0, "{}{}", run.out, run.err);
    assert!(
        run.out.contains("  ok      a C compiler: CC is "),
        "{}",
        run.out
    );
}

/// Y98: told it may (`--install-system-packages`), `prepare` installs the package with the computer's package
/// manager, through `sudo` when it is not the administrator, looks again, and goes on to do what it was asked; a
/// second run finds the compiler and installs nothing; and without the flag nothing is installed.
#[test]
fn p3_y98_prepare_installs_the_packages_when_told_to_and_only_then() {
    let world = World::new("y98");
    let file = needing_a_compiler(&world);
    let file = file.to_str().unwrap();
    let (bin, log) = computer_without_a_compiler(&world);
    // a sudo that needs no password: it runs what it is given
    stand_in(&bin, "sudo", "shift\nexec \"$@\"");

    // without the flag: told, and nothing installed
    let mut command = quickai_run(&world, &["prepare", file, "--gpu", "none"]);
    command.env("PATH", &bin);
    let run = finish(command);
    assert_eq!(run.code, 6, "{}{}", run.out, run.err);
    assert!(
        run.err
            .contains("quickai-run can install the packages itself with --install-system-packages"),
        "{}",
        run.err
    );
    assert!(!log.exists());
    assert_eq!(world.files.requests(), 0, "nothing was fetched either");

    // with it: installed, looked at again, and then what was asked
    let mut command = quickai_run(
        &world,
        &[
            "prepare",
            file,
            "--gpu",
            "none",
            "--install-system-packages",
        ],
    );
    command.env("PATH", &bin);
    let run = finish(command);
    assert_eq!(run.code, 0, "{}{}", run.out, run.err);
    assert_eq!(
        std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        [
            "update",
            "install -y -o DPkg::Lock::Timeout=120 build-essential"
        ]
    );
    for line in [
        "  MISSING a C compiler",
        "Installing build-essential with apt-get.",
        "  ok      a C compiler: gcc at ",
        "Everything it needs is in place; nothing was started.",
    ] {
        assert!(run.out.contains(line), "{line:?} in\n{}", run.out);
    }
    assert!(
        run.out.find("Installing build-essential") < run.out.find("Everything it needs"),
        "{}",
        run.out
    );
    assert!(
        home(&world).join(".quickai/llama-bin").is_dir(),
        "the run went on to install the engine"
    );

    // a second run finds the compiler, and installs nothing
    std::fs::remove_file(&log).unwrap();
    let mut command = quickai_run(
        &world,
        &[
            "prepare",
            file,
            "--gpu",
            "none",
            "--install-system-packages",
        ],
    );
    command.env("PATH", &bin);
    let run = finish(command);
    assert_eq!(run.code, 0, "{}{}", run.out, run.err);
    assert!(!log.exists(), "nothing to install");
    assert!(
        run.out.contains("  ok      a C compiler: gcc at "),
        "{}",
        run.out
    );

    // told not to look: the look is left out, and so is the install
    std::fs::remove_file(bin.join("gcc")).unwrap();
    let mut command = quickai_run(
        &world,
        &["prepare", file, "--gpu", "none", "--skip-prerequisites"],
    );
    command.env("PATH", &bin);
    let run = finish(command);
    assert_eq!(run.code, 0, "{}{}", run.out, run.err);
    assert!(
        run.out
            .contains("Not looking at what this computer has to have (a C compiler), as you said."),
        "{}",
        run.out
    );
    assert!(!log.exists());
}
