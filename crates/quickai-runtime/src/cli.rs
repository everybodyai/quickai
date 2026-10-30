//! Headless commands for development and CI (docs/architecture.md, section 2.2), run before the
//! window opens:
//!
//! ```text
//! quickai config check [--strict] [--embedded]
//! quickai plan [--explain | --json] [--model FILE] [--binary FILE] [--port N] [--ctx-size N]
//!              [--gpu-layers N] [--extra-args TEXT] [--proxy]
//! ```
//!
//! `config check` loads every configuration layer and prints what is wrong with it, with file and
//! line; it exits 1 on errors (and, with `--strict`, on warnings too). `plan` prints the run plan for
//! a selection, with where every argument came from. Neither is a product feature: they exist so the
//! documents can be checked and the planner looked at without starting the interface. A Windows
//! release build has no console, so use a debug build there.

use std::io::Write;
use std::path::PathBuf;

use quickai_core::diag::Severity;
use quickai_core::plan::explain::render_explain;
use quickai_core::value::Value;

use crate::config::{build_registry, ConfigDirs};
use crate::launch::MachineFacts;
use crate::resolve::{self, ResolveError, Wanted};
use crate::settings::DEFAULT_ENGINE;

const USAGE: &str = "\
QuickAI headless commands

  quickai config check [--strict] [--embedded]
      Load the configuration layers and report problems with file and line.
      --strict      warnings (for example keys this version does not know) fail too
      --embedded    check only the documents compiled into the application

  quickai config docs [vocab | catalog | profile | telemetry]
      Print the reference page for engine documents (docs/reference/engine-fields.md), or with
      `vocab` the one for vocabulary documents (docs/reference/vocab-fields.md), with `catalog`
      the one for catalog documents (docs/reference/catalog-fields.md), or with `profile` the one
      for configuration documents (docs/reference/profile-fields.md), generated from the types the
      loader reads; with `telemetry`, every event QuickAI can send
      (docs/reference/telemetry-events.md).

  quickai --telemetry
      Print, as JSON, every telemetry event this build can send: its level, why it is collected,
      and each field with its shape and the kind of data it is.

  quickai plan [--explain | --json] [--engine ID] [--model FILE|NAME] [--binary FILE] [--port N]
               [--ctx-size N] [--gpu-layers N] [--extra-args TEXT] [--proxy]
      Print the run plan for a selection. Values not given come from the engine's field defaults.
      --engine ID   the engine (default llamacpp); for one that lists its models by name, such as
                    ollama, --model is a name and may be left out; one that runs on its own, such
                    as comfyui, is given no model at all
      --explain     (default) the plan as text, with the origin of every argument
      --json        the plan as JSON

  quickai plan ... [--configuration ID|FILE]
      Start from a configuration: its engine, model and settings; what is given on the line wins.

  quickai configuration list | show | check | export | import
      Configurations, a runtime as a file that can be shared (docs/configurations.md). Each is shown by
      `quickai configuration` with no arguments.

  quickai --help
";

/// Runs a headless command if `args` is one. `None` means "not a headless command": start the
/// interface as usual. Any other arguments are ignored by the interface, as they always were.
pub fn run(args: &[String]) -> Option<i32> {
    let first = args.first().map(String::as_str)?;
    if !matches!(
        first,
        "config" | "configuration" | "plan" | "--telemetry" | "--help" | "-h"
    ) {
        return None;
    }
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    Some(run_with(
        args,
        &ConfigDirs::from_environment(),
        &mut stdout.lock(),
        &mut stderr.lock(),
    ))
}

pub fn run_with(
    args: &[String],
    dirs: &ConfigDirs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            let _ = write!(out, "{USAGE}");
            0
        }
        Some("--telemetry") if args.len() == 1 => {
            let list = quickai_core::telemetry::schema::list_json();
            let _ = writeln!(
                out,
                "{}",
                serde_json::to_string_pretty(&list).unwrap_or_default()
            );
            0
        }
        Some("config") => match args.get(1).map(String::as_str) {
            Some("check") => config_check(&args[2..], dirs, out, err),
            Some("docs") if args.len() == 2 => {
                let _ = write!(out, "{}", quickai_core::doc::reference::render());
                0
            }
            Some("docs") if args.len() == 3 && args[2] == "vocab" => {
                let _ = write!(out, "{}", quickai_core::doc::reference::render_vocab());
                0
            }
            Some("docs") if args.len() == 3 && args[2] == "catalog" => {
                let _ = write!(out, "{}", quickai_core::doc::reference::render_catalog());
                0
            }
            Some("docs") if args.len() == 3 && args[2] == "telemetry" => {
                let _ = write!(out, "{}", quickai_core::telemetry::schema::reference_page());
                0
            }
            Some("docs") if args.len() == 3 && args[2] == "profile" => {
                let _ = write!(out, "{}", quickai_core::doc::reference::render_profile());
                0
            }
            _ => usage_error(
                err,
                "expected `config check` or `config docs [vocab | catalog | profile]`",
            ),
        },
        Some("configuration") => crate::configuration_cli::run(&args[1..], dirs, out, err),
        Some("plan") => plan_command(&args[1..], dirs, out, err),
        _ => usage_error(err, "unknown command"),
    }
}

fn usage_error(err: &mut dyn Write, message: &str) -> i32 {
    let _ = writeln!(err, "quickai: {message}\n\n{USAGE}");
    2
}

fn config_check(
    args: &[String],
    dirs: &ConfigDirs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let mut strict = false;
    let mut embedded_only = false;
    for arg in args {
        match arg.as_str() {
            "--strict" => strict = true,
            "--embedded" => embedded_only = true,
            other => return usage_error(err, &format!("unknown option `{other}`")),
        }
    }
    let registry = if embedded_only {
        build_registry(&ConfigDirs::default())
    } else {
        build_registry(dirs)
    };
    let (mut errors, mut warnings) = (0, 0);
    for d in &registry.diagnostics {
        match d.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
            Severity::Info => {}
        }
        let _ = writeln!(out, "{d}");
    }
    // the engines to choose from, and the programs that go with one (they are documents of an engine, and not to choose)
    let engines: Vec<&str> = registry.selectable().map(|e| e.id.as_str()).collect();
    let companions: Vec<&str> = registry
        .engines()
        .filter(|e| e.doc.engine.companion_of.is_some())
        .map(|e| e.id.as_str())
        .collect();
    let _ = writeln!(
        out,
        "config: {errors} error{}, {warnings} warning{}; engines: {}{}",
        if errors == 1 { "" } else { "s" },
        if warnings == 1 { "" } else { "s" },
        if engines.is_empty() {
            "none".to_string()
        } else {
            engines.join(", ")
        },
        if companions.is_empty() {
            String::new()
        } else {
            format!("; companions: {}", companions.join(", "))
        }
    );
    i32::from(errors > 0 || (strict && warnings > 0))
}

struct PlanArgs {
    json: bool,
    model: Option<PathBuf>,
    binary: Option<String>,
    port: Option<u16>,
    ctx_size: Option<u32>,
    gpu_layers: Option<u32>,
    extra_args: Option<String>,
    proxy: bool,
    engine: Option<String>,
    /// A configuration (an id, or a file) to start from.
    configuration: Option<String>,
}

fn parse_plan_args(args: &[String]) -> Result<PlanArgs, String> {
    let mut parsed = PlanArgs {
        json: false,
        model: None,
        binary: None,
        port: None,
        ctx_size: None,
        gpu_layers: None,
        extra_args: None,
        proxy: false,
        engine: None,
        configuration: None,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| -> Result<String, String> {
            it.next()
                .cloned()
                .ok_or_else(|| format!("`{name}` needs a value"))
        };
        match arg.as_str() {
            "--explain" => parsed.json = false,
            "--json" => parsed.json = true,
            "--proxy" => parsed.proxy = true,
            "--engine" => parsed.engine = Some(value("--engine")?),
            "--configuration" => parsed.configuration = Some(value("--configuration")?),
            "--model" => parsed.model = Some(PathBuf::from(value("--model")?)),
            "--binary" => parsed.binary = Some(value("--binary")?),
            "--extra-args" => parsed.extra_args = Some(value("--extra-args")?),
            "--port" => {
                parsed.port = Some(
                    value("--port")?
                        .parse()
                        .map_err(|_| "`--port` needs a number from 1 to 65535".to_string())?,
                )
            }
            "--ctx-size" => {
                parsed.ctx_size = Some(
                    value("--ctx-size")?
                        .parse()
                        .map_err(|_| "`--ctx-size` needs a whole number".to_string())?,
                )
            }
            "--gpu-layers" => {
                parsed.gpu_layers = Some(
                    value("--gpu-layers")?
                        .parse()
                        .map_err(|_| "`--gpu-layers` needs a whole number".to_string())?,
                )
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(parsed)
}

fn plan_command(
    args: &[String],
    dirs: &ConfigDirs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let parsed = match parse_plan_args(args) {
        Ok(p) => p,
        Err(message) => return usage_error(err, &message),
    };
    let registry = build_registry(dirs);
    for d in registry
        .diagnostics
        .iter()
        .filter(|d| d.severity != Severity::Info)
    {
        let _ = writeln!(err, "{d}");
    }

    // a configuration gives the engine, the model and the settings; what is said on the line wins
    let base = match &parsed.configuration {
        Some(target) => match crate::configuration_cli::plan_base(&registry, target) {
            Ok(base) => Some(base),
            Err(message) => {
                let _ = writeln!(err, "quickai: {message}");
                return 1;
            }
        },
        None => None,
    };
    let engine = match (&parsed.engine, &base) {
        (Some(wanted), Some(base)) if wanted != &base.engine => {
            let _ = writeln!(
                err,
                "quickai: `--engine {wanted}` and the configuration, which is for {}, disagree",
                base.engine
            );
            return 2;
        }
        (Some(wanted), _) => wanted.clone(),
        (None, Some(base)) => base.engine.clone(),
        (None, None) => DEFAULT_ENGINE.to_string(),
    };
    let Some(engine_doc) = registry.engine(&engine) else {
        let _ = writeln!(err, "quickai: there is no engine called `{engine}`");
        return 1;
    };
    // one that runs on its own (ComfyUI) is given no model: the work a client sends it names the files it uses
    let needs_model = engine_doc.doc.provides.needs_model;
    if !needs_model && parsed.model.is_some() {
        let _ = writeln!(
            err,
            "quickai: {engine} runs with no model, so --model is ignored"
        );
    }

    // what the configuration gives, and then what is said on the line
    let mut wanted = Wanted {
        engine: engine.clone(),
        model_path: needs_model
            .then(|| {
                parsed
                    .model
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
            })
            .flatten(),
        model_name: base.as_ref().and_then(|b| b.model.clone()),
        binary: parsed.binary.clone(),
        proxy: parsed.proxy || base.as_ref().is_some_and(|b| b.proxy),
        ..Wanted::default()
    };
    if let Some(base) = &base {
        wanted.values.extend(base.values.clone());
    }
    let key = |field: &str| format!("{engine}.{field}");
    if let Some(port) = parsed.port {
        wanted.values.insert(key("port"), Value::from(port));
    }
    if let Some(ctx) = parsed.ctx_size {
        wanted.values.insert(key("ctx_size"), Value::from(ctx));
    }
    if let Some(layers) = parsed.gpu_layers {
        wanted
            .values
            .insert(key("n_gpu_layers"), Value::from(layers));
    }
    if let Some(extra) = &parsed.extra_args {
        wanted
            .values
            .insert(key("extra_args"), Value::from(extra.as_str()));
    }
    if let Some(binary) = &parsed.binary {
        wanted
            .values
            .insert(key("bin_override"), Value::from(binary.as_str()));
    }

    let machine = MachineFacts::detect(crate::hardware::total_ram_bytes(), &crate::gpu::detect());
    let mut notes = Vec::new();
    let resolved = resolve::resolve(&registry, dirs, machine, &wanted, &mut notes);
    for note in notes {
        let _ = writeln!(err, "{note}");
    }
    match resolved {
        Ok(resolved) => {
            if parsed.json {
                let _ = writeln!(
                    out,
                    "{}",
                    serde_json::to_string_pretty(&resolved.plan).unwrap_or_default()
                );
            } else {
                let _ = write!(out, "{}", render_explain(&resolved.plan));
            }
            0
        }
        Err(ResolveError::UnknownEngine(engine)) => {
            let _ = writeln!(err, "quickai: there is no engine called `{engine}`");
            1
        }
        Err(ResolveError::ModelNotHere { name }) => {
            let _ = writeln!(
                err,
                "quickai: the configuration's model, {name}, is not in the models folder; pass --model FILE"
            );
            1
        }
        Err(ResolveError::NoModel) => {
            let _ = writeln!(err, "quickai: no model found; pass --model FILE");
            1
        }
        Err(ResolveError::NoPort(e)) => {
            let _ = writeln!(err, "quickai: could not find a free internal port: {e}");
            1
        }
        Err(ResolveError::Planner(problems)) => {
            for d in problems {
                let _ = writeln!(err, "{d}");
            }
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn run_capture(list: &[&str], dirs: &ConfigDirs) -> (i32, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(&args(list), dirs, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("quickai-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn user_dir_with(name: &str, text: &str) -> (PathBuf, ConfigDirs) {
        let dir = scratch(name);
        std::fs::create_dir_all(dir.join("engines")).unwrap();
        std::fs::write(dir.join("engines/llamacpp.toml"), text).unwrap();
        let dirs = ConfigDirs {
            user: Some(dir.clone()),
            ..ConfigDirs::default()
        };
        (dir, dirs)
    }

    #[test]
    fn only_the_known_subcommands_are_taken_by_the_cli() {
        assert_eq!(run(&[]), None);
        assert_eq!(run(&args(&["somefile.gguf"])), None);
        assert_eq!(run(&args(&["--unknown"])), None);
    }

    #[test]
    fn p3_e16_config_check_strict_on_the_embedded_documents_is_clean_with_ollama_in_the_set() {
        let (code, out, err) = run_capture(
            &["config", "check", "--strict", "--embedded"],
            &ConfigDirs::default(),
        );
        assert_eq!(code, 0, "{out}{err}");
        assert_eq!(
            out,
            "config: 0 errors, 0 warnings; engines: llamacpp, ollama, comfyui; companions: openwebui\n"
        );
    }

    #[test]
    fn config_check_reports_a_broken_document_with_file_and_line() {
        let (dir, dirs) = user_dir_with(
            "broken",
            "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[launch.spawn]\nargs = [\"${modl.main.path}\"]\n",
        );
        let (code, out, _) = run_capture(&["config", "check"], &dirs);
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("llamacpp.toml:5"), "{out}");
        assert!(out.contains("unknown name `modl`"), "{out}");
        assert!(out.contains("did you mean `model`?"), "{out}");
        assert!(out.contains("config: 1 error, 0 warnings"), "{out}");
        // the embedded document stays in force
        assert!(out.contains("engines: llamacpp"), "{out}");

        // --embedded ignores the user's documents
        let (code, out, _) = run_capture(&["config", "check", "--embedded"], &dirs);
        assert_eq!(code, 0, "{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_w05c_config_check_holds_documents_to_the_widgets_the_interface_draws() {
        // a field of the person's own, naming a widget that nothing draws
        let document = |widget: &str| {
            format!("schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n\n[[field]]\nid = \"extra\"\ntype = \"text\"\nui = {{ widget = \"{widget}\" }}\n")
        };
        let (dir, dirs) = user_dir_with("widget", &document("runtime_managr"));
        let (code, out, _) = run_capture(&["config", "check"], &dirs);
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("llamacpp.toml:8"), "{out}");
        assert!(
            out.contains("`runtime_managr` is not a widget this version of QuickAI has"),
            "{out}"
        );
        assert!(out.contains("did you mean `runtime_manager`?"), "{out}");
        let _ = std::fs::remove_dir_all(&dir);

        // one that is drawn loads
        let (dir, dirs) = user_dir_with("widget-ok", &document("runtime_manager"));
        let (code, out, _) = run_capture(&["config", "check", "--strict"], &dirs);
        assert_eq!(code, 0, "{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_docs_prints_the_checked_in_reference_page() {
        let (code, out, _) = run_capture(&["config", "docs"], &ConfigDirs::default());
        assert_eq!(code, 0);
        let page = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/reference/engine-fields.md"),
        )
        .expect("the reference page is checked in");
        assert_eq!(
            out,
            page.replace("\r\n", "\n"),
            "regenerate it with `quickai config docs`"
        );
        // it takes no arguments but the name of the other page
        let (code, _, err) = run_capture(&["config", "docs", "extra"], &ConfigDirs::default());
        assert_eq!(code, 2, "{err}");
        let (code, _, err) = run_capture(&["config", "docs", "vocab", "x"], &ConfigDirs::default());
        assert_eq!(code, 2, "{err}");
    }

    #[test]
    fn config_docs_vocab_prints_the_checked_in_page_for_vocabulary_documents() {
        let (code, out, _) = run_capture(&["config", "docs", "vocab"], &ConfigDirs::default());
        assert_eq!(code, 0);
        let page = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/reference/vocab-fields.md"),
        )
        .expect("the reference page is checked in");
        assert_eq!(
            out,
            page.replace("\r\n", "\n"),
            "regenerate it with `quickai config docs vocab`"
        );
    }

    #[test]
    fn telemetry_lists_every_event_as_json_and_as_a_page() {
        let (code, out, _) = run_capture(&["--telemetry"], &ConfigDirs::default());
        assert_eq!(code, 0);
        let list: serde_json::Value = serde_json::from_str(&out).expect("JSON");
        assert_eq!(
            list["events"].as_array().map(Vec::len),
            Some(quickai_core::telemetry::schema::EVENTS.len())
        );
        let (code, page, _) = run_capture(&["config", "docs", "telemetry"], &ConfigDirs::default());
        assert_eq!(code, 0);
        assert_eq!(page, quickai_core::telemetry::schema::reference_page());
        // only alone
        let (code, _, _) = run_capture(&["--telemetry", "x"], &ConfigDirs::default());
        assert_ne!(code, 0);
    }

    #[test]
    fn config_docs_catalog_prints_the_checked_in_page_for_catalog_documents() {
        let (code, out, _) = run_capture(&["config", "docs", "catalog"], &ConfigDirs::default());
        assert_eq!(code, 0);
        let page = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/reference/catalog-fields.md"),
        )
        .expect("the reference page is checked in");
        assert_eq!(
            out,
            page.replace("\r\n", "\n"),
            "regenerate it with `quickai config docs catalog`"
        );
    }

    #[test]
    fn config_docs_profile_prints_the_page_of_configuration_documents() {
        let (code, out, _) = run_capture(&["config", "docs", "profile"], &ConfigDirs::default());
        assert_eq!(code, 0);
        assert!(out.contains("# Configuration documents"), "{out}");
        let page = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/profile-fields.md"),
        )
        .expect("the reference page is checked in");
        assert_eq!(
            out,
            page.replace("\r\n", "\n"),
            "regenerate it with `quickai config docs profile`"
        );
    }

    #[test]
    fn warnings_fail_only_under_strict() {
        let (dir, dirs) = user_dir_with(
            "warn",
            "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[future]\nthing = 1\n",
        );
        let (code, out, _) = run_capture(&["config", "check"], &dirs);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("1 warning"), "{out}");
        let (code, _, _) = run_capture(&["config", "check", "--strict"], &dirs);
        assert_eq!(code, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_e27_plan_explains_an_ollama_run_by_name_and_runs_empty_without_one() {
        let (code, out, err) = run_capture(
            &[
                "plan",
                "--explain",
                "--engine",
                "ollama",
                "--model",
                "qwen3:4b",
                "--binary",
                "/opt/ollama/bin/ollama",
                "--port",
                "9999",
            ],
            &ConfigDirs::default(),
        );
        assert_eq!(code, 0, "{out}{err}");
        assert!(out.contains("plan: qwen3:4b on ollama"), "{out}");
        assert!(out.contains("env OLLAMA_HOST=127.0.0.1:9999"), "{out}");
        assert!(
            out.contains(r#"POST /api/generate {"keep_alive":-1,"model":"qwen3:4b"}"#),
            "{out}"
        );
        let (code, out, _) = run_capture(
            &[
                "plan",
                "--engine",
                "ollama",
                "--binary",
                "/opt/ollama/bin/ollama",
            ],
            &ConfigDirs::default(),
        );
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("plan: (no model) on ollama") && !out.contains("warm"),
            "{out}"
        );
        let (code, _, err) = run_capture(&["plan", "--engine", "nosuch"], &ConfigDirs::default());
        assert_eq!(code, 1, "{err}");
        assert!(err.contains("there is no engine called `nosuch`"), "{err}");
    }

    #[test]
    fn plan_explains_a_run_with_defaults_filled_in() {
        let dir = scratch("plan");
        let model = dir.join("Test-Model-Q4.gguf");
        std::fs::write(&model, vec![0u8; 1024]).unwrap();
        let (code, out, err) = run_capture(
            &[
                "plan",
                "--explain",
                "--model",
                model.to_str().unwrap(),
                "--binary",
                "/opt/quickai/llama-server",
                "--port",
                "9001",
            ],
            &ConfigDirs::default(),
        );
        assert_eq!(code, 0, "{out}{err}");
        assert!(
            out.starts_with("plan: Test-Model-Q4.gguf on llamacpp\n"),
            "{out}"
        );
        assert!(out.contains("/opt/quickai/llama-server"), "{out}");
        assert!(out.contains("9001"), "{out}");
        assert!(
            out.contains("field llamacpp.n_gpu_layers (default)"),
            "{out}"
        );
        assert!(out.contains("(default: recommend_ctx_size)"), "{out}");
        assert!(out.contains("GET /health == 200"), "{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plan_can_print_json() {
        let dir = scratch("planjson");
        let model = dir.join("m.gguf");
        std::fs::write(&model, b"x").unwrap();
        let (code, out, _) = run_capture(
            &[
                "plan",
                "--json",
                "--model",
                model.to_str().unwrap(),
                "--binary",
                "/x/llama-server",
                "--proxy",
            ],
            &ConfigDirs::default(),
        );
        assert_eq!(code, 0, "{out}");
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(json["engine"], "llamacpp");
        assert!(json["gateway"]["upstream_port"].as_u64().is_some());
        assert_eq!(json["units"][0]["start"][0]["how"], "spawn");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mistakes_get_usage_and_exit_two() {
        for list in [
            &["config"][..],
            &["config", "bogus"],
            &["config", "check", "--nope"],
            &["plan", "--port"],
            &["plan", "--port", "abc"],
            &["plan", "--wat"],
        ] {
            let (code, _, err) = run_capture(list, &ConfigDirs::default());
            assert_eq!(code, 2, "{list:?}: {err}");
            assert!(err.contains("QuickAI headless commands"), "{list:?}: {err}");
        }
        let (code, out, _) = run_capture(&["--help"], &ConfigDirs::default());
        assert_eq!(code, 0);
        assert!(out.contains("config check"));
    }

    #[test]
    fn plan_without_any_model_says_so() {
        // an explicit missing file still plans (the engine will say it cannot open it); with no
        // --model at all and no models in the dev tree this machine may or may not have, only the
        // argument handling is checked here
        let parsed = parse_plan_args(&args(&["--proxy", "--ctx-size", "512"])).unwrap();
        assert!(parsed.proxy && parsed.model.is_none());
        assert_eq!(parsed.ctx_size, Some(512));
    }

    #[test]
    fn the_plan_of_an_engine_that_runs_on_its_own_has_no_model() {
        // ComfyUI is started with none, whatever models the folders hold: the header says so, the JSON has
        // none, and a --model that is given is said to be ignored
        let dirs = ConfigDirs::default();
        let (code, text, err) = run_capture(&["plan", "--engine", "comfyui"], &dirs);
        assert_eq!(code, 0, "{text}{err}");
        assert!(text.starts_with("plan: (no model) on comfyui\n"), "{text}");
        assert_eq!(err, "");
        let (code, json, _) = run_capture(&["plan", "--engine", "comfyui", "--json"], &dirs);
        assert_eq!(code, 0);
        let plan: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(plan["model"], serde_json::Value::Null, "{json}");
        let (code, _, err) =
            run_capture(&["plan", "--engine", "comfyui", "--model", "x.gguf"], &dirs);
        assert_eq!(code, 0);
        assert!(
            err.contains("comfyui runs with no model, so --model is ignored"),
            "{err}"
        );
    }

    #[test]
    fn the_proxy_is_planned_only_for_an_engine_whose_traffic_is_read() {
        let dirs = ConfigDirs::default();
        let (code, json, err) =
            run_capture(&["plan", "--engine", "comfyui", "--json", "--proxy"], &dirs);
        assert_eq!(code, 0, "{json}{err}");
        let plan: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(plan["gateway"], serde_json::Value::Null, "{json}");
        assert_eq!(plan["public_port"], 8188);
        // the engine listens on the public port itself
        let args: Vec<&str> = plan["units"][0]["start"][1]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["value"].as_str().unwrap())
            .collect();
        let at = args.iter().position(|a| *a == "--port").unwrap();
        assert_eq!(args[at + 1], "8188");

        // llama.cpp's conversation is read through it
        let dir = scratch("proxy-llama");
        let model = dir.join("m.gguf");
        std::fs::write(&model, b"x").unwrap();
        let (code, json, err) = run_capture(
            &[
                "plan",
                "--json",
                "--proxy",
                "--model",
                &model.display().to_string(),
            ],
            &dirs,
        );
        assert_eq!(code, 0, "{json}{err}");
        let plan: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(plan["gateway"].is_object(), "{json}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- configurations (WP4.17.4) --------------------------------------------------------------

    const EXAMPLE: &str = include_str!("../../../docs/examples/profiles/vision-chat.toml");

    /// A configuration directory of someone's, with a data directory of its own so that what is installed
    /// and what is downloaded is what the test puts there.
    fn person(name: &str) -> (PathBuf, ConfigDirs) {
        let dir = scratch(name);
        let dirs = ConfigDirs {
            user: Some(dir.clone()),
            data: Some(dir.join("data")),
            ..ConfigDirs::default()
        };
        (dir, dirs)
    }

    fn put(dir: &Path, rel: &str, text: &str) -> PathBuf {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    fn tree(dir: &Path) -> Vec<String> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, base, out);
                } else {
                    // one separator on every system, so the lists read the same everywhere
                    let rel = path.strip_prefix(base).unwrap();
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    #[test]
    fn p3_a47_configuration_list_and_show_print_what_the_store_has() {
        let (dir, dirs) = person("a47");
        put(&dir, "profiles/vision-chat.toml", EXAMPLE);
        put(
            &dir,
            "profiles/broken.toml",
            "schema = 1\nkind = \"profile\"\nid = \"broken\"\n",
        );
        put(&dir, "profiles/later.toml", "schema = 1\nkind = \"profile\"\nid = \"later\"\nrequires = { app = \">=9.0\" }\nengine = \"llamacpp\"\n");
        let mut settings = crate::settings::Settings::default();
        settings.set_active_configuration("vision-chat");
        settings.save_to(&dir.join("settings.json"));

        let (code, out, _) = run_capture(&["configuration", "list"], &dirs);
        assert_eq!(code, 0);
        let lines: Vec<&str> = out.lines().collect();
        assert!(
            lines[0].starts_with("  default")
                && lines[0].contains("Default")
                && lines[0].contains("llamacpp")
                && lines[0].contains("built in"),
            "{out}"
        );
        assert!(
            lines[1].starts_with("* vision-chat")
                && lines[1].contains("Vision chat")
                && lines[1].contains("ollama")
                && lines[1].contains("yours")
                && lines[1].contains("qwen3-vl:8b"),
            "{out}"
        );
        // the files that could not be used, with why
        assert!(
            out.contains("! ")
                && out.contains("broken.toml")
                && out.contains("missing field `engine`"),
            "{out}"
        );
        assert!(
            out.contains("later.toml") && out.contains("update QuickAI"),
            "{out}"
        );
        // with nothing chosen the Default is the one in use
        std::fs::remove_file(dir.join("settings.json")).unwrap();
        let (_, out, _) = run_capture(&["configuration", "list"], &dirs);
        assert!(
            out.lines().next().unwrap().starts_with("* default"),
            "{out}"
        );

        // show: what it holds, and what a person should be told about it on this computer
        let (code, out, err) = run_capture(&["configuration", "show", "vision-chat"], &dirs);
        assert_eq!(code, 0, "{err}");
        assert!(
            out.starts_with("Vision chat (vision-chat), yours\n"),
            "{out}"
        );
        for want in [
            "Engine:  ollama",
            "Program: v0.40.0-ollama-linux-amd64",
            "Model:   qwen3-vl:8b (main, 5.7 GB)",
            "Context size: 16384",
            "Extra environment (KEY=VALUE per line): OLLAMA_FLASH_ATTENTION=1",
            "Start Open WebUI with Ollama: true",
            "On this computer:",
            "Made with v0.40.0-ollama-linux-amd64, which is not installed here, and no build of this program is.",
            "Ollama is not running, so it cannot be told whether qwen3-vl:8b is here.",
        ] {
            assert!(out.contains(want), "{want} in\n{out}");
        }
        let (code, _, err) = run_capture(&["configuration", "show", "nope"], &dirs);
        assert_eq!(code, 1);
        assert!(err.contains("no configuration called `nope`"), "{err}");
        let (code, _, _) = run_capture(&["configuration", "show"], &dirs);
        assert_eq!(code, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a48_configuration_check_prints_the_notes_for_this_computer_and_reads_nothing_but_the_file(
    ) {
        let (dir, dirs) = person("a48");
        let file = put(&dir, "given/vision-chat.toml", EXAMPLE);
        let before = tree(&dir);
        let (code, out, err) =
            run_capture(&["configuration", "check", file.to_str().unwrap()], &dirs);
        assert_eq!(code, 0, "{out}{err}");
        assert!(
            out.starts_with("Vision chat (vision-chat), for ollama\n"),
            "{out}"
        );
        // what is asked about is shown in full
        assert!(
            out.contains("  Asks   Lines of environment, set for Ollama as they are written."),
            "{out}"
        );
        assert!(
            out.contains("ollama.extra_env = OLLAMA_FLASH_ATTENTION=1"),
            "{out}"
        );
        assert!(out.contains("Warns"), "{out}");
        assert!(
            out.trim_end().ends_with(" warns") && out.contains("0 stops, 1 asks"),
            "{out}"
        );
        // --strict fails on what is asked or warned about; a file that cannot be used fails always
        let (code, _, _) = run_capture(
            &["configuration", "check", "--strict", file.to_str().unwrap()],
            &dirs,
        );
        assert_eq!(code, 1);
        let alien = put(
            &dir,
            "given/alien.toml",
            "schema = 1\nkind = \"profile\"\nid = \"alien\"\nengine = \"whispercpp\"\n",
        );
        let (code, out, _) =
            run_capture(&["configuration", "check", alien.to_str().unwrap()], &dirs);
        assert_eq!(code, 1);
        assert!(
            out.contains("Stops  It is for whispercpp, which this QuickAI does not have."),
            "{out}"
        );
        let newer = put(&dir, "given/newer.toml", "schema = 1\nkind = \"profile\"\nid = \"n\"\nrequires = { app = \">=9.0\" }\nengine = \"llamacpp\"\n");
        let (code, out, _) =
            run_capture(&["configuration", "check", newer.to_str().unwrap()], &dirs);
        assert_eq!(code, 1);
        assert!(out.contains("Update QuickAI to use it."), "{out}");
        let junk = put(&dir, "given/junk.toml", "this is = not toml = at all");
        let (code, out, _) =
            run_capture(&["configuration", "check", junk.to_str().unwrap()], &dirs);
        assert_eq!(
            (code, out.contains("not a QuickAI configuration")),
            (1, true),
            "{out}"
        );
        let (code, _, err) = run_capture(
            &[
                "configuration",
                "check",
                dir.join("nope.toml").to_str().unwrap(),
            ],
            &dirs,
        );
        assert_eq!(code, 1);
        assert!(err.contains("could not read"), "{err}");
        // and nothing was written anywhere
        let mut expected = before.clone();
        expected
            .extend(["given/alien.toml", "given/newer.toml", "given/junk.toml"].map(String::from));
        expected.sort();
        assert_eq!(tree(&dir), expected);
        let (code, _, _) = run_capture(&["configuration", "check"], &dirs);
        assert_eq!(code, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a49_export_import_and_plan_configuration() {
        let (dir, dirs) = person("a49");
        let mut with_secret = EXAMPLE.replace(
            "OLLAMA_FLASH_ATTENTION=1",
            "OLLAMA_FLASH_ATTENTION=1\\nHF_TOKEN=hunter2",
        );
        with_secret.push('\n');
        put(&dir, "profiles/vision-chat.toml", &with_secret);

        // export: to the screen, no secret, and what was left out is said
        let (code, out, err) = run_capture(&["configuration", "export", "vision-chat"], &dirs);
        assert_eq!(code, 0, "{err}");
        assert!(
            out.contains("OLLAMA_FLASH_ATTENTION=1")
                && !out.contains("hunter2")
                && !out.contains("HF_TOKEN"),
            "{out}"
        );
        assert!(
            out.contains("kind = \"profile\"") && out.contains("made_with = \"QuickAI "),
            "{out}"
        );
        assert!(
            err.contains("Lines left out because they look like secrets"),
            "{err}"
        );
        let given = dir.join("shared.toml");
        let (code, _, _) = run_capture(
            &[
                "configuration",
                "export",
                "vision-chat",
                "-o",
                given.to_str().unwrap(),
                "--note",
                "For you.",
            ],
            &dirs,
        );
        assert_eq!(code, 0);
        let text = std::fs::read_to_string(&given).unwrap();
        assert!(text.contains("note = \"For you.\""), "{text}");
        let (code, _, err) = run_capture(&["configuration", "export", "nope"], &dirs);
        assert_eq!(
            (code, err.contains("no configuration called nope")),
            (1, true),
            "{err}"
        );

        // import on another computer: what is asked about is not taken without a yes
        let (other, other_dirs) = person("a49b");
        let (code, out, err) = run_capture(
            &["configuration", "import", given.to_str().unwrap()],
            &other_dirs,
        );
        assert_eq!(code, 0, "{out}{err}");
        assert!(
            out.contains("imported as Vision chat (vision-chat)"),
            "{out}"
        );
        assert!(out.contains("not taken: ollama.extra_env"), "{out}");
        let stored = std::fs::read_to_string(other.join("profiles/vision-chat.toml")).unwrap();
        assert!(
            stored.contains("ctx_size = 16384") && !stored.contains("extra_env"),
            "{stored}"
        );
        // the same name again is a question, and the answers are three
        let (code, _, err) = run_capture(
            &["configuration", "import", given.to_str().unwrap()],
            &other_dirs,
        );
        assert_eq!(code, 1);
        assert!(
            err.contains("--as NAME, --keep-both or --replace")
                && err.contains("nothing was stored"),
            "{err}"
        );
        let (code, out, _) = run_capture(
            &[
                "configuration",
                "import",
                given.to_str().unwrap(),
                "--keep-both",
                "--accept-review",
            ],
            &other_dirs,
        );
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("imported as Vision chat 2 (vision-chat-2)"),
            "{out}"
        );
        let kept = std::fs::read_to_string(other.join("profiles/vision-chat-2.toml")).unwrap();
        assert!(
            kept.contains("OLLAMA_FLASH_ATTENTION=1"),
            "taken with a yes: {kept}"
        );
        let (code, _, _) = run_capture(
            &[
                "configuration",
                "import",
                given.to_str().unwrap(),
                "--replace",
                "--accept-review",
            ],
            &other_dirs,
        );
        assert_eq!(code, 0);
        assert!(
            std::fs::read_to_string(other.join("profiles/vision-chat.toml"))
                .unwrap()
                .contains("extra_env")
        );
        let (code, out, _) = run_capture(
            &[
                "configuration",
                "import",
                given.to_str().unwrap(),
                "--as",
                "Other name",
            ],
            &other_dirs,
        );
        assert_eq!(code, 0, "{out}");
        assert!(other.join("profiles/other-name.toml").exists());
        // a file that cannot be used stores nothing
        let alien = put(
            &dir,
            "alien.toml",
            "schema = 1\nkind = \"profile\"\nid = \"alien\"\nengine = \"whispercpp\"\n",
        );
        let before = tree(&other);
        let (code, _, err) = run_capture(
            &["configuration", "import", alien.to_str().unwrap()],
            &other_dirs,
        );
        assert_eq!(code, 1);
        assert!(err.contains("nothing was stored"), "{err}");
        assert_eq!(tree(&other), before);
        // the settings of the person are not touched by an import
        assert!(!other.join("settings.json").exists());

        // plan: the run plan the configuration makes here
        let (code, out, err) = run_capture(&["plan", "--configuration", "vision-chat"], &dirs);
        assert_eq!(code, 0, "{err}");
        assert!(
            out.contains("OLLAMA_CONTEXT_LENGTH") && out.contains("16384"),
            "{out}"
        );
        let (code, json, _) =
            run_capture(&["plan", "--configuration", "vision-chat", "--json"], &dirs);
        assert_eq!(code, 0);
        let plan: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(plan["engine"], "ollama");
        assert_eq!(plan["model"], "qwen3-vl:8b");
        // what is said on the line wins, and an engine that disagrees is refused
        let (code, out, _) = run_capture(
            &[
                "plan",
                "--configuration",
                "vision-chat",
                "--ctx-size",
                "4096",
            ],
            &dirs,
        );
        assert_eq!(code, 0);
        assert!(out.contains("4096") && !out.contains("16384"), "{out}");
        let (code, _, err) = run_capture(
            &[
                "plan",
                "--configuration",
                "vision-chat",
                "--engine",
                "llamacpp",
            ],
            &dirs,
        );
        assert_eq!(code, 2);
        assert!(err.contains("disagree"), "{err}");
        // a file too
        let (code, out, _) =
            run_capture(&["plan", "--configuration", given.to_str().unwrap()], &dirs);
        assert_eq!(code, 0);
        assert!(out.contains("OLLAMA_CONTEXT_LENGTH"), "{out}");
        let (code, _, err) = run_capture(&["plan", "--configuration", "nope"], &dirs);
        assert_eq!(
            (code, err.contains("no configuration called `nope`")),
            (1, true),
            "{err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }
}
