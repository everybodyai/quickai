//! What was typed: `quickai-run up <configuration> [options]` and the other words (docs/headless.md, section 5).
//!
//! Kept apart from everything that does anything so that what a mistake says can be tested without a
//! computer: a word it does not know is named, a value it needs is asked for, and nothing is guessed.

use std::path::PathBuf;

use quickai_runtime::gpu::vendors_named;
use quickai_runtime::provision::{FetchWord, Options, Secret};

pub const USAGE: &str = "\
quickai-run: run a QuickAI configuration with no window

  quickai-run up <configuration> [options]
      Get everything the configuration needs on this computer (the programs, the models), start it, say
      where it is, and stay until it is stopped (Ctrl-C, or SIGTERM from a service manager). The plan is
      printed first: what will be installed and fetched, from where, to where, and how big it is.

  quickai-run prepare <configuration> [options]
      The same up to starting: install and fetch what is missing, and start nothing.

  quickai-run check <configuration> [options]
      Say what would be done and change nothing (the same as `up --dry-run`).

  quickai-run init <configuration> [-o FILE] [--force]
      Write the environment file to fill in for a configuration: what it needs from you, what belongs to
      a computer (folders, a Python, logins, the variables of a program's environment) and the files it
      fetches, each with where from. It is made readable by you only. `-o -` prints it. Then give it back
      with `--environment FILE`.

  quickai-run plan | config | configuration ...
      QuickAI's other headless commands, unchanged (see `quickai --help`): for example
      `quickai-run configuration import vision.toml`, and `quickai-run configuration list`.

<configuration> is the id of one you keep (`quickai-run configuration list`) or the path of a file.

Options:
  --accept-review      take what a file from outside asks about (free text, wider access, a place it
                       fetches a model from): it is shown and is not used unless you say so
  --build [ID=]LABEL   the build to install when none is installed, instead of the one the configuration
                       names or the engine's document chooses; `openwebui=0.11.4` names the build of a
                       program that goes with the engine
  --port N             the port the engine listens on, instead of the configuration's
  --models-dir DIR     keep the model files in DIR (a bigger disk), instead of the engine's own folder
  --python PATH        the Python an install that builds an environment uses
  --gpu CARDS          what this computer has when it cannot be told (nvidia, amd, intel, apple, or none;
                       several with commas); it decides which build and which PyTorch are installed
  --allow-program HASH let a document of your own start a program QuickAI did not install (the hash is
                       in the question it asks); may be given again
  --dry-run            as `check`
  --json               print one JSON object a line for each thing that happens, instead of sentences
  --quiet              print only the plan, where it is, and what went wrong
  --ready-file PATH    write where everything is to PATH once it runs (and remove it when it stops)

What the computer has to have (a configuration says what it needs of it: a compiler, the headers of Python, a driver):
  --install-system-packages  when the computer lacks something a package brings (a compiler, the headers or the
                       venv module of Python), install the package with the computer's own package manager
                       (apt-get, dnf, zypper, pacman, apk; apt-get's install waits up to two minutes for a
                       lock another process holds, such as the upgrades a new server runs by itself);
                       it has to run as the administrator, or sudo must need no password. Without this flag
                       a missing package is only told, with the exact command that installs it.
  --skip-prerequisites look at none of it, and run no test of the installed environment

What belongs to this computer and not to the configuration:
  --environment FILE   a file of this computer's values (`init` writes one to fill in); may be given
                       again, a later one wins
  --set ENGINE.FIELD=VALUE   one setting of the engine or of a program that goes with it, also the ones
                       a configuration cannot carry (a folder, a Python); may be given again
  --env [PROGRAM:]NAME=VALUE one variable of a program's environment (PROGRAM defaults to the engine);
                       may be given again
  --hf-token TOKEN, --hf-endpoint URL, --github-token TOKEN
                       a login for Hugging Face, another address for it (a mirror), a login for GitHub's
                       list of builds; the variables HF_TOKEN, HF_ENDPOINT and GITHUB_TOKEN do the same
                       (a flag wins). Prefer the variable or the file: a flag shows in `ps`

Where the models come from, when not where the configuration says:
  --model-source FILE=WHERE  get this file from WHERE: a path of the file (or of a folder that has it),
                       an address, or huggingface:owner/repo; may be given again
  --models-from DIR    look in DIR for the files of the models before fetching them (they are linked,
                       not copied, and checked); may be given again
  --offline            fetch nothing: use what is here, and say what is missing and where each file goes
  --version, --help

It listens on 127.0.0.1 only; to reach it from elsewhere use an SSH tunnel. Exit codes: 0 done or stopped,
1 it could not be done, 2 a mistake in the command, 3 the file asks and was not accepted, 4 a program
stopped by itself, 5 the configuration needs something nobody gave, 6 the computer lacks something the
configuration needs (or the installed environment failed its test), 130 interrupted before it ran.
";

/// What `up`, `prepare` and `check` have in common.
#[derive(Debug, PartialEq)]
pub struct Invocation {
    pub target: String,
    pub options: Options,
    pub json: bool,
    pub quiet: bool,
    pub ready_file: Option<PathBuf>,
}

/// `init`: the file to fill in.
#[derive(Debug, PartialEq)]
pub struct InitInvocation {
    pub target: String,
    /// Where to write it, `-` for the standard output; left out, next to the configuration.
    pub output: Option<String>,
    /// Write over a file that is there.
    pub force: bool,
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Help,
    Version,
    Run(Box<Invocation>),
    Init(InitInvocation),
}

/// Reads the command line (without the program's name).
pub fn parse(args: &[String]) -> Result<Parsed, String> {
    let Some(first) = args.first() else {
        return Err("say what to do: `up`, `prepare` or `check` and a configuration".into());
    };
    let (prepare, dry_run) = match first.as_str() {
        "--help" | "-h" | "help" => return Ok(Parsed::Help),
        "--version" | "-V" => return Ok(Parsed::Version),
        "up" => (false, false),
        "prepare" => (true, false),
        "check" => (false, true),
        "init" => return parse_init(&args[1..]),
        other => return Err(format!("`{other}` is not something this does")),
    };
    let mut invocation = Invocation {
        target: String::new(),
        options: Options {
            prepare_only: prepare,
            dry_run,
            ..Options::default()
        },
        json: false,
        quiet: false,
        ready_file: None,
    };
    let mut it = args[1..].iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next()
                .cloned()
                .ok_or_else(|| format!("`{flag}` needs a value"))
        };
        match arg.as_str() {
            "--help" | "-h" => return Ok(Parsed::Help),
            "--accept-review" => invocation.options.accept_review = true,
            "--dry-run" => invocation.options.dry_run = true,
            "--json" => invocation.json = true,
            "--quiet" | "-q" => invocation.quiet = true,
            "--build" => {
                let text = value("--build")?;
                match text.split_once('=') {
                    Some((id, label))
                        if !id.is_empty()
                            && id
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) =>
                    {
                        invocation
                            .options
                            .companion_builds
                            .insert(id.to_string(), label.to_string());
                    }
                    _ => invocation.options.build = Some(text),
                }
            }
            "--port" => {
                let text = value("--port")?;
                invocation.options.port =
                    Some(text.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| {
                        format!("`--port` needs a number from 1 to 65535, not `{text}`")
                    })?);
            }
            "--models-dir" => {
                invocation.options.models_dir = Some(PathBuf::from(value("--models-dir")?))
            }
            "--python" => invocation.options.python = Some(PathBuf::from(value("--python")?)),
            "--gpu" => invocation.options.gpus = Some(vendors_named(&value("--gpu")?)?),
            "--allow-program" => {
                let hash = value("--allow-program")?;
                if hash.len() < 12 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(format!(
                        "`--allow-program` needs the hash the question gave (twelve hexadecimal digits or more), not `{hash}`"
                    ));
                }
                invocation.options.confirmed.push(hash.to_ascii_lowercase());
            }
            "--ready-file" => invocation.ready_file = Some(PathBuf::from(value("--ready-file")?)),
            "--environment" => invocation
                .options
                .environment_files
                .push(PathBuf::from(value("--environment")?)),
            "--set" => invocation.options.sets.push(value("--set")?),
            "--env" => invocation.options.env_lines.push(value("--env")?),
            "--model-source" => invocation
                .options
                .model_sources
                .push(value("--model-source")?),
            "--models-from" => invocation
                .options
                .models_from
                .push(PathBuf::from(value("--models-from")?)),
            "--offline" => invocation.options.offline = true,
            "--install-system-packages" => invocation.options.install_system_packages = true,
            "--skip-prerequisites" => invocation.options.skip_prerequisites = true,
            "--hf-token" => invocation.options.fetch.push(FetchWord {
                key: "huggingface_token".into(),
                value: Secret::new(value("--hf-token")?),
                origin: "--hf-token".into(),
            }),
            "--hf-endpoint" => invocation.options.fetch.push(FetchWord {
                key: "huggingface_endpoint".into(),
                value: Secret::new(value("--hf-endpoint")?),
                origin: "--hf-endpoint".into(),
            }),
            "--github-token" => invocation.options.fetch.push(FetchWord {
                key: "github_token".into(),
                value: Secret::new(value("--github-token")?),
                origin: "--github-token".into(),
            }),
            other if other.starts_with('-') => return Err(format!("`{other}` is not an option")),
            other if invocation.target.is_empty() => invocation.target = other.to_string(),
            other => {
                return Err(format!(
                    "one configuration at a time: `{other}` follows `{}`",
                    invocation.target
                ))
            }
        }
    }
    if invocation.target.is_empty() {
        return Err(format!(
            "`{first}` needs a configuration: an id, or the path of a file"
        ));
    }
    Ok(Parsed::Run(Box::new(invocation)))
}

fn parse_init(args: &[String]) -> Result<Parsed, String> {
    let mut init = InitInvocation {
        target: String::new(),
        output: None,
        force: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(Parsed::Help),
            "--force" => init.force = true,
            "-o" | "--output" => {
                init.output = Some(
                    it.next()
                        .cloned()
                        .ok_or_else(|| format!("`{arg}` needs a value"))?,
                )
            }
            other if other.starts_with('-') => {
                return Err(format!("`{other}` is not an option of `init`"))
            }
            other if init.target.is_empty() => init.target = other.to_string(),
            other => {
                return Err(format!(
                    "one configuration at a time: `{other}` follows `{}`",
                    init.target
                ))
            }
        }
    }
    if init.target.is_empty() {
        return Err("`init` needs a configuration: an id, or the path of a file".to_string());
    }
    Ok(Parsed::Init(init))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickai_runtime::gpu::GpuVendor;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    fn run(line: &str) -> Invocation {
        match parse(&words(line)) {
            Ok(Parsed::Run(invocation)) => *invocation,
            other => panic!("{line}: {other:?}"),
        }
    }

    #[test]
    fn p3_o30_the_words_and_their_options_are_read() {
        let up = run("up vision.toml --port 9000 --gpu nvidia,amd --accept-review --build b1 --json -q --ready-file /run/q.json");
        assert_eq!(up.target, "vision.toml");
        assert_eq!(up.options.port, Some(9000));
        assert_eq!(
            up.options.gpus,
            Some(vec![GpuVendor::Amd, GpuVendor::Nvidia])
        );
        assert!(up.options.accept_review && up.json && up.quiet);
        assert_eq!(up.options.build.as_deref(), Some("b1"));
        assert_eq!(up.ready_file, Some(PathBuf::from("/run/q.json")));
        assert!(!up.options.dry_run && !up.options.prepare_only);

        assert!(run("prepare x").options.prepare_only);
        assert!(run("check x").options.dry_run);
        assert!(run("up x --dry-run").options.dry_run);
        assert_eq!(run("up x --gpu none").options.gpus, Some(Vec::new()));
        assert_eq!(
            run("up x").options.gpus,
            None,
            "not said: the machine is asked"
        );
        assert_eq!(
            run("up x --allow-program ABCDEF012345 --allow-program 0123456789abcdef")
                .options
                .confirmed,
            ["abcdef012345", "0123456789abcdef"]
        );
        assert_eq!(parse(&words("--help")), Ok(Parsed::Help));
        assert_eq!(parse(&words("up --help")), Ok(Parsed::Help));
        assert_eq!(parse(&words("--version")), Ok(Parsed::Version));
    }

    /// Y60: the words that say what belongs to this computer and where the models come from are read, each as
    /// many times as it is given, in the order given; a login is a secret from the moment it is read.
    #[test]
    fn p3_y60_the_words_for_this_computer_and_for_where_models_come_from_are_read() {
        let up = run("up wan.toml --environment a.toml --environment b.toml --set comfyui.models_dir=/mnt/x --set comfyui.port=9000 --env WEBUI_NAME=Studio --env openwebui:ENABLE_SIGNUP=false --model-source v.safetensors=/srv/v.safetensors --model-source u.gguf=https://m.example/u.gguf --models-from /srv/a --models-from /srv/b --offline --hf-token hf_abc --hf-endpoint https://m.example --github-token ghp_abc --build openwebui=0.11.4 --build v0.39.0");
        assert_eq!(
            up.options.environment_files,
            [PathBuf::from("a.toml"), PathBuf::from("b.toml")]
        );
        assert_eq!(
            up.options.sets,
            ["comfyui.models_dir=/mnt/x", "comfyui.port=9000"]
        );
        assert_eq!(
            up.options.env_lines,
            ["WEBUI_NAME=Studio", "openwebui:ENABLE_SIGNUP=false"]
        );
        assert_eq!(
            up.options.model_sources,
            [
                "v.safetensors=/srv/v.safetensors",
                "u.gguf=https://m.example/u.gguf"
            ]
        );
        assert_eq!(
            up.options.models_from,
            [PathBuf::from("/srv/a"), PathBuf::from("/srv/b")]
        );
        assert!(up.options.offline);
        let given: Vec<(&str, &str)> = up
            .options
            .fetch
            .iter()
            .map(|w| (w.key.as_str(), w.origin.as_str()))
            .collect();
        assert_eq!(
            given,
            [
                ("huggingface_token", "--hf-token"),
                ("huggingface_endpoint", "--hf-endpoint"),
                ("github_token", "--github-token")
            ]
        );
        assert!(
            !format!("{:?}", up.options).contains("hf_abc")
                && !format!("{:?}", up).contains("ghp_abc"),
            "a login is not in what prints"
        );
        // the build of a program that goes with the engine, and the engine's own
        assert_eq!(up.options.build.as_deref(), Some("v0.39.0"));
        assert_eq!(
            up.options
                .companion_builds
                .get("openwebui")
                .map(String::as_str),
            Some("0.11.4")
        );
        // and none of it is there when it is not said
        let plain = run("up x");
        assert!(plain.options.environment_files.is_empty() && plain.options.sets.is_empty());
        assert!(plain.options.fetch.is_empty() && !plain.options.offline);

        // `init`
        assert_eq!(
            parse(&words("init wan.toml")),
            Ok(Parsed::Init(InitInvocation {
                target: "wan.toml".into(),
                output: None,
                force: false
            }))
        );
        assert_eq!(
            parse(&words("init wan -o /tmp/env.toml --force")),
            Ok(Parsed::Init(InitInvocation {
                target: "wan".into(),
                output: Some("/tmp/env.toml".into()),
                force: true
            }))
        );
        assert_eq!(
            parse(&words("init wan --output -")),
            Ok(Parsed::Init(InitInvocation {
                target: "wan".into(),
                output: Some("-".into()),
                force: false
            }))
        );
        assert_eq!(parse(&words("init --help")), Ok(Parsed::Help));
    }

    /// Y95: the two words about what the computer has to have are read, off unless said, and the usage says them
    /// and the code of their own.
    #[test]
    fn p3_y95_the_words_for_what_the_computer_has_to_have_are_read_and_in_the_usage() {
        let plain = run("up x");
        assert!(!plain.options.install_system_packages && !plain.options.skip_prerequisites);
        assert!(
            plain.options.machine.is_none(),
            "this computer is the one looked at"
        );
        let both = run("prepare x --install-system-packages --skip-prerequisites");
        assert!(both.options.install_system_packages && both.options.skip_prerequisites);
        assert!(
            run("check x --install-system-packages")
                .options
                .install_system_packages
        );
        for word in [
            "--install-system-packages",
            "--skip-prerequisites",
            "6 the computer lacks something",
            "apt-get, dnf, zypper, pacman, apk",
        ] {
            assert!(USAGE.contains(word), "{word} in the usage");
        }
        // a flag that is not one is still refused, and the new ones take no value
        assert!(parse(&words("up x --install-packages")).is_err());
        assert_eq!(
            parse(&words("up --install-system-packages")),
            Err("`up` needs a configuration: an id, or the path of a file".to_string())
        );
    }

    /// Y61: a mistake in the new words is named, and nothing is guessed.
    #[test]
    fn p3_y61_a_mistake_in_the_new_words_is_named() {
        let wrong = |line: &str| parse(&words(line)).expect_err(line);
        for flag in [
            "--environment",
            "--set",
            "--env",
            "--model-source",
            "--models-from",
            "--hf-token",
            "--hf-endpoint",
            "--github-token",
            "--build",
        ] {
            assert!(
                wrong(&format!("up x {flag}")).contains("needs a value"),
                "{flag}"
            );
        }
        assert!(wrong("init").contains("needs a configuration"));
        assert!(wrong("init a b").contains("one configuration at a time"));
        assert!(wrong("init a --nonsense").contains("`--nonsense`"));
        assert!(wrong("init a -o").contains("needs a value"));
    }

    #[test]
    fn p3_o31_a_mistake_is_named_and_nothing_is_guessed() {
        let wrong = |line: &str| parse(&words(line)).expect_err(line);
        assert!(wrong("").contains("say what to do"));
        assert!(wrong("frobnicate x").contains("`frobnicate`"));
        assert!(wrong("up").contains("needs a configuration"));
        assert!(wrong("up a b").contains("one configuration at a time"));
        assert!(wrong("up a --nonsense").contains("`--nonsense`"));
        assert!(wrong("up a --port").contains("needs a value"));
        assert!(wrong("up a --port 99999").contains("1 to 65535"));
        assert!(wrong("up a --port 0").contains("1 to 65535"));
        assert!(wrong("up a --gpu nvidlia").contains("nvidlia"));
        assert!(wrong("up a --allow-program xyz").contains("hexadecimal"));
        assert!(wrong("up a --allow-program abc").contains("twelve"));
    }
}
