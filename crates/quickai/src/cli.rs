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
use std::path::{Path, PathBuf};

use quickai_core::diag::Severity;
use quickai_core::plan::explain::render_explain;
use quickai_core::plan::Selection;

use crate::config::{build_registry, ConfigDirs};
use crate::launch::{self, MachineFacts, ENGINE_ID};
use crate::models::ModelEntry;

const USAGE: &str = "\
QuickAI headless commands

  quickai config check [--strict] [--embedded]
      Load the configuration layers and report problems with file and line.
      --strict      warnings (for example keys this version does not know) fail too
      --embedded    check only the documents compiled into the application

  quickai config docs
      Print the reference page for engine documents (docs/reference/engine-fields.md), generated
      from the types the loader reads.

  quickai plan [--explain | --json] [--model FILE] [--binary FILE] [--port N]
               [--ctx-size N] [--gpu-layers N] [--extra-args TEXT] [--proxy]
      Print the run plan for a selection. Values not given come from the engine's field defaults.
      --explain     (default) the plan as text, with the origin of every argument
      --json        the plan as JSON

  quickai --help
";

/// Runs a headless command if `args` is one. `None` means "not a headless command": start the
/// interface as usual. Any other arguments are ignored by the interface, as they always were.
pub fn run(args: &[String]) -> Option<i32> {
    let first = args.first().map(String::as_str)?;
    if !matches!(first, "config" | "plan" | "--help" | "-h") {
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
        Some("config") => match args.get(1).map(String::as_str) {
            Some("check") => config_check(&args[2..], dirs, out, err),
            Some("docs") if args.len() == 2 => {
                let _ = write!(out, "{}", quickai_core::doc::reference::render());
                0
            }
            _ => usage_error(err, "expected `config check` or `config docs`"),
        },
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
    let engines: Vec<&str> = registry.engines().map(|e| e.id.as_str()).collect();
    let _ = writeln!(
        out,
        "config: {errors} error{}, {warnings} warning{}; engines: {}",
        if errors == 1 { "" } else { "s" },
        if warnings == 1 { "" } else { "s" },
        if engines.is_empty() {
            "none".to_string()
        } else {
            engines.join(", ")
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

fn model_entry(path: &Path) -> ModelEntry {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    ModelEntry {
        display: filename.clone(),
        filename,
        path: path.to_path_buf(),
        note: String::new(),
        bundled: false,
        size_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
    }
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

    let model = match &parsed.model {
        Some(path) => model_entry(path),
        None => {
            let bundled = crate::paths::bundled_models_dir();
            let user = crate::paths::user_models_dir();
            match crate::models::discover_all(&bundled, &user)
                .into_iter()
                .next()
            {
                Some(m) => m,
                None => {
                    let _ = writeln!(err, "quickai: no model found; pass --model FILE");
                    return 1;
                }
            }
        }
    };

    let mut selection = Selection::new(ENGINE_ID).with_model(quickai_core::plan::ModelInfo {
        id: model.filename.clone(),
        display: model.display.clone(),
        path: model.path.display().to_string(),
        size_bytes: model.size_bytes,
    });
    if let Some(port) = parsed.port {
        selection = selection.set("llamacpp.port", port);
    }
    if let Some(ctx) = parsed.ctx_size {
        selection = selection.set("llamacpp.ctx_size", ctx);
    }
    if let Some(layers) = parsed.gpu_layers {
        selection = selection.set("llamacpp.n_gpu_layers", layers);
    }
    if let Some(extra) = &parsed.extra_args {
        selection = selection.set("llamacpp.extra_args", extra.as_str());
    }
    if let Some(binary) = &parsed.binary {
        selection = selection.set("llamacpp.bin_override", binary.as_str());
    }
    if parsed.proxy {
        selection = selection.set(quickai_core::plan::TRAFFIC_PROXY_KEY, true);
    }

    let binary = launch::locate_binary(&registry, parsed.binary.as_deref());
    let internal_port = if parsed.proxy {
        match crate::runtime::process::ephemeral_port() {
            Ok(port) => Some(port),
            Err(e) => {
                let _ = writeln!(err, "quickai: could not find a free internal port: {e}");
                return 1;
            }
        }
    } else {
        None
    };
    let machine = MachineFacts::detect(crate::hardware::total_ram_bytes(), &crate::gpu::detect());
    match launch::plan_selection(&registry, &selection, &binary, machine, internal_port) {
        Ok(plan) => {
            if parsed.json {
                let _ = writeln!(
                    out,
                    "{}",
                    serde_json::to_string_pretty(&plan).unwrap_or_default()
                );
            } else {
                let _ = write!(out, "{}", render_explain(&plan));
            }
            0
        }
        Err(problems) => {
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
    fn config_check_on_the_embedded_documents_is_clean() {
        let (code, out, err) = run_capture(
            &["config", "check", "--strict", "--embedded"],
            &ConfigDirs::default(),
        );
        assert_eq!(code, 0, "{out}{err}");
        assert_eq!(out, "config: 0 errors, 0 warnings; engines: llamacpp\n");
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
        // it takes no arguments
        let (code, _, err) = run_capture(&["config", "docs", "extra"], &ConfigDirs::default());
        assert_eq!(code, 2, "{err}");
    }

    #[test]
    fn warnings_fail_only_under_strict() {
        let (dir, dirs) = user_dir_with(
            "warn",
            "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[models]\nneeds_server = true\n",
        );
        let (code, out, _) = run_capture(&["config", "check"], &dirs);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("1 warning"), "{out}");
        let (code, _, _) = run_capture(&["config", "check", "--strict"], &dirs);
        assert_eq!(code, 1);
        let _ = std::fs::remove_dir_all(&dir);
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
}
