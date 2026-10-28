//! `quickai-run`: QuickAI with no window (docs/headless.md).
//!
//! It reads a configuration, gets what it needs on this computer, starts it and stays until it is told to
//! stop, in a terminal or in the background (`background`). Everything that is done is the runtime's
//! (`quickai_runtime::provision`), the same code the window uses for the same things; this is the command line
//! around it. It runs on Linux, macOS and Windows.

mod args;
mod background;
mod free;
mod render;
mod signals;

use std::io::Write;
use std::path::Path;

use quickai_runtime::config::ConfigDirs;
use quickai_runtime::provision::{self, Event, FetchWord, Options, Outcome, Request, Secret};
use quickai_runtime::run_file::{self, NotHeld};

use args::{InitInvocation, Parsed, USAGE};

/// Writes where everything is, whole or not at all: a script that waits for the file never reads half.
fn write_ready_file(path: &Path, event: &Event) {
    let Event::Ready { endpoints, model } = event else {
        return;
    };
    let body =
        serde_json::json!({ "endpoints": endpoints, "model": model, "pid": std::process::id() });
    let temporary = path.with_extension("tmp");
    if std::fs::write(&temporary, format!("{body}\n")).is_ok() {
        let _ = std::fs::rename(&temporary, path);
    }
}

/// The logins and the mirror that the variables every tool of the kind reads give; a flag wins over them.
fn from_variables() -> Vec<FetchWord> {
    let first = |names: &[&'static str]| {
        names.iter().find_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .map(|v| (*name, v))
        })
    };
    let mut words = Vec::new();
    for (key, names) in [
        (
            "huggingface_token",
            &["HF_TOKEN", "HUGGING_FACE_HUB_TOKEN"][..],
        ),
        ("huggingface_endpoint", &["HF_ENDPOINT"][..]),
        ("github_token", &["GITHUB_TOKEN", "GH_TOKEN"][..]),
    ] {
        if let Some((name, value)) = first(names) {
            words.push(FetchWord {
                key: key.to_string(),
                value: Secret::new(value),
                origin: format!("the variable {name}"),
            });
        }
    }
    words
}

/// The logins the run was given are for the fetching: what the run starts (an engine, a page beside it, a custom
/// node a document installs) does not inherit them. The engines are isolated from each other and from what they do
/// not need, and a program can read its environment.
fn keep_the_logins_to_the_fetching() {
    for name in [
        "HF_TOKEN",
        "HUGGING_FACE_HUB_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
    ] {
        std::env::remove_var(name);
    }
}

/// `init`: the file to fill in, written where it was asked for and readable by the person only.
fn run_init(init: InitInvocation) -> i32 {
    let request = Request {
        dirs: ConfigDirs::from_environment(),
        target: init.target.clone(),
        options: Options::default(),
    };
    let text = match provision::init(&request) {
        Ok(text) => text,
        Err(failure) => {
            eprintln!("quickai-run: {}", failure.message);
            return provision::Outcome::Failed(failure).exit_code();
        }
    };
    if init.output.as_deref() == Some("-") {
        print!("{text}");
        return 0;
    }
    let path = init
        .output
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| provision::default_environment_path(&init.target));
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if init.force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(text.as_bytes()) {
                eprintln!("quickai-run: could not write {}: {e}", path.display());
                return 1;
            }
            #[cfg(unix)]
            {
                // a file that was there keeps the mode it had: it is made private now
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            println!("Wrote {}", path.display());
            println!("Fill in what applies, then:");
            println!(
                "  quickai-run check {} --environment {}",
                init.target,
                path.display()
            );
            0
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            eprintln!(
                "quickai-run: {} is there already; say --force to write over it, or -o to write another file",
                path.display()
            );
            2
        }
        Err(e) => {
            eprintln!("quickai-run: could not write {}: {e}", path.display());
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // QuickAI's other headless commands are the same code in both programs
    if matches!(
        args.first().map(String::as_str),
        Some("config" | "configuration" | "plan")
    ) {
        if let Some(code) = quickai_runtime::cli::run(&args) {
            std::process::exit(code);
        }
    }
    let invocation = match args::parse(&args) {
        Ok(Parsed::Help) => {
            print!("{USAGE}");
            return;
        }
        Ok(Parsed::Version) => {
            println!("quickai-run {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Ok(Parsed::Init(init)) => std::process::exit(run_init(init)),
        Ok(Parsed::Status { json }) => {
            let dir = run_file::run_dir(&ConfigDirs::from_environment());
            std::process::exit(background::status(&dir, json))
        }
        Ok(Parsed::Free { ports, json }) => std::process::exit(free::run(&ports, json)),
        Ok(Parsed::Down { wait }) => {
            let dir = run_file::run_dir(&ConfigDirs::from_environment());
            std::process::exit(background::down(&dir, wait))
        }
        Ok(Parsed::Run(invocation)) => {
            let mut invocation = *invocation;
            // the variables first, so that a flag, which comes later in the list, wins
            let mut words = from_variables();
            keep_the_logins_to_the_fetching();
            words.append(&mut invocation.options.fetch);
            invocation.options.fetch = words;
            invocation
        }
        Err(message) => {
            eprintln!("quickai-run: {message}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let dirs = ConfigDirs::from_environment();
    let dir = run_file::run_dir(&dirs);
    if invocation.detach {
        std::process::exit(background::start(&args, &dir));
    }
    signals::install(invocation.background);
    // one run at a time: what changes this computer holds its run (`check` changes nothing)
    let mut held = if invocation.options.dry_run {
        None
    } else {
        match run_file::hold(&dir, &invocation.target, invocation.background) {
            Ok(held) => Some(held),
            Err(NotHeld::Busy(run)) => {
                eprintln!("quickai-run: {}", background::busy(run.as_deref()));
                std::process::exit(7);
            }
            Err(NotHeld::Io(e)) => {
                eprintln!("quickai-run: could not write in {}: {e}", dir.display());
                std::process::exit(1);
            }
        }
    };
    if held.is_some() {
        signals::watch_stop_file(dir.clone());
    }
    let request = Request {
        dirs,
        target: invocation.target.clone(),
        options: invocation.options.clone(),
    };
    let mut human = render::Human::new(invocation.quiet);
    let (json, ready_file) = (invocation.json, invocation.ready_file.clone());
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let outcome = provision::up(
        &request,
        &mut |event| {
            if let Some(held) = held.as_mut() {
                held.note(&event);
            }
            if let Some(path) = &ready_file {
                write_ready_file(path, &event);
            }
            if json {
                let _ = writeln!(stdout.lock(), "{}", render::json_line(&event));
            } else {
                human.show(&event, &mut stdout.lock(), &mut stderr.lock());
            }
        },
        &signals::STOP,
    );
    if let Some(path) = &invocation.ready_file {
        let _ = std::fs::remove_file(path);
    }
    if let Some(held) = held.take() {
        let message = match &outcome {
            Outcome::Failed(failure) => Some(failure.message.clone()),
            _ => None,
        };
        held.record_end(outcome.word(), message, outcome.exit_code());
        // the run's file goes before the exit, which runs no destructor
        drop(held);
    }
    std::process::exit(outcome.exit_code());
}
