//! `quickai configuration list | show | check | export | import | identity`, and the configuration of `quickai
//! plan --configuration` (docs/configurations.md, section 11). They are for scripts and for support,
//! and they are how the whole of it is tested without a window.
//!
//! There is no `use` on purpose: the window writes `settings.json` when it changes and when it
//! closes, and a command that wrote it under a running window would be undone by it.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use quickai_core::profile::check::{open_text, review, Inventory, Report};
use quickai_core::profile::notes::{Level, Note};
use quickai_core::profile::state::{carried, WorkingState};
use quickai_core::profile::{label_of, shown, Configuration, MAX_FILE_BYTES};
use quickai_core::registry::Registry;

use crate::config::{build_registry, ConfigDirs};
use crate::configurations::{
    self, busy_ports, entries, export_text, import, installed_builds, inventory, listed_by_engines,
    models_here, unusable, working_state, Host, Installed, Now, OnTaken,
};
use crate::launch::{self, MachineFacts};
use crate::settings::Settings;

pub const USAGE: &str = "\
  quickai configuration list
      The configurations there are: the one in use is marked *, and the files that could not be used
      are listed with the reason.

  quickai configuration show <id | file>
      What a configuration holds, and what a person should be told about it on this computer.

  quickai configuration check <file> [--strict]
      The notes for this computer about a file from outside. Exits 1 when one stops it (and with
      --strict when one asks or warns). It reads nothing but the file, and starts and writes nothing.

  quickai configuration export <id> [-o FILE] [--note TEXT]
      The text of a configuration to give away: no folder, no program, no secret. To the screen, or to FILE.

  quickai configuration import <file> [--as NAME] [--replace | --keep-both] [--accept-review]
      Stores a file from outside as a configuration of your own. Free text and settings that widen
      what a program may do are shown and are not taken unless --accept-review says so.

  quickai configuration identity <id | file> [--canonical]
      Who the configuration is, whatever the spelling of its file: two with the same identity run the
      same thing, and quickai.everybodyai.dev keeps one copy of each. --canonical also prints the form
      the hash is taken over.

  quickai plan --configuration <id | file> [--explain | --json]
      The run plan that configuration makes here.
";

/// Everything about this computer that a review needs.
pub struct Computer {
    pub host: Host,
    pub installed: Installed,
    pub models: std::collections::BTreeMap<String, quickai_core::profile::check::ModelsHere>,
}

/// Reads the computer: its memory and cards, the builds installed, the models in the folders and the
/// lists the engines gave last time. Nothing is started and nothing is asked of the network.
pub fn detect(registry: &Registry, dirs: &ConfigDirs) -> Computer {
    let machine = MachineFacts::detect(crate::hardware::total_ram_bytes(), &crate::gpu::detect());
    detect_for(registry, dirs, machine)
}

/// `detect` for a computer whose memory and cards are already known (a person said which cards it has).
pub fn detect_for(registry: &Registry, dirs: &ConfigDirs, machine: MachineFacts) -> Computer {
    let host = Host::from_machine(&machine);
    let mut facts = launch::host_facts(Path::new(""), machine, None);
    if let Some(data) = &dirs.data {
        facts.data_dir = data.display().to_string();
    }
    let installed = installed_builds(registry, &facts);
    let (bundled, user) = match &dirs.data {
        Some(data) => (data.join("bundled-models"), data.join("models")),
        None => (
            crate::paths::bundled_models_dir(),
            crate::paths::user_models_dir(),
        ),
    };
    let listed = listed_by_engines(registry, &facts);
    let models = models_here(registry, &bundled, &|_| user.clone(), &listed);
    Computer {
        host,
        installed,
        models,
    }
}

impl Computer {
    pub fn inventory(
        &self,
        registry: &Registry,
        dirs: &ConfigDirs,
        configuration: Option<&Configuration>,
        settings: &Settings,
    ) -> Inventory {
        let data = dirs.data.as_ref().map(|d| d.display().to_string());
        let busy = configuration
            .map(|c| {
                busy_ports(
                    registry,
                    c,
                    &working_state(registry, settings, &self.installed),
                )
            })
            .unwrap_or_default();
        inventory(
            registry,
            &self.host,
            &self.installed,
            &self.models,
            Now {
                running: &BTreeSet::new(),
                busy_ports: busy,
                first_addresses: configurations::first_addresses(registry, settings),
            },
            &|engine| {
                launch::locate_binary_in(registry, &engine.id, None, data.as_deref()).exists()
            },
        )
    }
}

/// The settings of the person whose configuration directory this is.
pub fn settings_of(dirs: &ConfigDirs) -> Settings {
    match &dirs.user {
        Some(user) => Settings::load_from(&user.join("settings.json")),
        None => Settings::default(),
    }
}

fn word(level: Level) -> &'static str {
    level.label()
}

fn print_notes(registry: &Registry, report: &Report, out: &mut dyn Write) {
    for note in &report.notes {
        let _ = writeln!(
            out,
            "  {:<6} {}",
            word(note.level),
            note.text(registry.vocabulary())
        );
    }
}

pub fn read_file(path: &str) -> Result<String, String> {
    let path = Path::new(path);
    let size = std::fs::metadata(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?
        .len();
    if size as usize > MAX_FILE_BYTES {
        return Err(format!(
            "{} is larger than {} KiB and is not a configuration",
            path.display(),
            MAX_FILE_BYTES / 1024
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not text", path.display()))
}

/// A configuration by its id, or from a file when the argument is one. The second is whether it came from outside.
pub fn resolve(
    registry: &Registry,
    target: &str,
) -> Result<(Configuration, Vec<Note>, bool), String> {
    if let Some(entry) = registry.profile(target) {
        return Ok((entry.configuration.clone(), entry.notes.clone(), false));
    }
    if Path::new(target).is_file() {
        let text = read_file(target)?;
        let opened = open_text(registry, &text, env!("CARGO_PKG_VERSION"))
            .map_err(|stop| stop.text(registry.vocabulary()))?;
        return Ok((opened.configuration, opened.notes, true));
    }
    Err(format!(
        "there is no configuration called `{target}`, and no such file"
    ))
}

pub fn run(args: &[String], dirs: &ConfigDirs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match args.first().map(String::as_str) {
        Some("list") if args.len() == 1 => list(dirs, out),
        Some("show") => show(&args[1..], dirs, out, err),
        Some("check") => check(&args[1..], dirs, out, err),
        Some("export") => export_command(&args[1..], dirs, out, err),
        Some("import") => import_command(&args[1..], dirs, out, err),
        Some("identity") => identity_command(&args[1..], dirs, out, err),
        _ => {
            let _ = writeln!(
                err,
                "quickai: expected `configuration list | show | check | export | import | identity`\n\n{USAGE}"
            );
            2
        }
    }
}

fn usage(err: &mut dyn Write, message: &str) -> i32 {
    let _ = writeln!(err, "quickai: {message}\n\n{USAGE}");
    2
}

fn list(dirs: &ConfigDirs, out: &mut dyn Write) -> i32 {
    let registry = build_registry(dirs);
    let settings = settings_of(dirs);
    let (active, _) = configurations::active_for_start(&registry, &settings);
    let all = entries(&registry);
    let width = |f: &dyn Fn(&configurations::Entry) -> usize| all.iter().map(f).max().unwrap_or(0);
    let (w_id, w_name, w_engine) = (
        width(&|e| e.id.chars().count()),
        width(&|e| e.name.chars().count()),
        width(&|e| e.configuration.engine.chars().count()),
    );
    for entry in &all {
        let model = entry
            .configuration
            .models
            .first()
            .map_or(String::new(), |m| format!("  {}", m.display()));
        let _ = writeln!(
            out,
            "{} {:<w_id$}  {:<w_name$}  {:<w_engine$}  {}{model}",
            if entry.id == active { "*" } else { " " },
            entry.id,
            entry.name,
            entry.configuration.engine,
            entry.origin.label(),
        );
    }
    for bad in unusable(&registry) {
        let _ = writeln!(out, "! {}\n    {}", bad.file, bad.why);
    }
    0
}

fn identity_command(
    args: &[String],
    dirs: &ConfigDirs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let (target, canonical) = match args {
        [target] => (target, false),
        [target, flag] | [flag, target] if flag == "--canonical" => (target, true),
        _ => return usage(err, "`configuration identity` needs an id or a file"),
    };
    let registry = build_registry(dirs);
    let configuration = match resolve(&registry, target) {
        Ok((configuration, _, _)) => configuration,
        Err(message) => {
            let _ = writeln!(err, "quickai: {message}");
            return 1;
        }
    };
    let id = quickai_core::profile::canonical::identity(&registry, &configuration);
    let _ = writeln!(out, "identity {}\nfamily   {}", id.identity, id.family);
    if canonical {
        let _ = writeln!(out, "{}", id.canonical);
    }
    0
}

fn show(args: &[String], dirs: &ConfigDirs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let [target] = args else {
        return usage(err, "`configuration show` needs an id or a file");
    };
    let registry = build_registry(dirs);
    let (configuration, read_notes, outside) = match resolve(&registry, target) {
        Ok(found) => found,
        Err(message) => {
            let _ = writeln!(err, "quickai: {message}");
            return 1;
        }
    };
    let computer = detect(&registry, dirs);
    let settings = settings_of(dirs);
    let inv = computer.inventory(&registry, dirs, Some(&configuration), &settings);
    let report = review(&registry, &configuration, &read_notes, &inv, outside);

    let origin = registry
        .profile(&configuration.id)
        .filter(|_| !outside)
        .map_or("a file", |p| configurations::Origin::of(p.layer).label());
    let _ = writeln!(
        out,
        "{} ({}), {origin}",
        configuration.name, configuration.id
    );
    if let Some(note) = &configuration.meta.note {
        let _ = writeln!(out, "  {note}");
    }
    let _ = writeln!(out, "Engine:  {}", configuration.engine);
    if let Some(build) = &configuration.runtime.build {
        let version = configuration
            .runtime
            .version
            .as_ref()
            .map_or(String::new(), |v| format!(" (version {v})"));
        let _ = writeln!(out, "Program: {build}{version}");
    }
    for pick in &configuration.models {
        let size = pick.size.map_or(String::new(), |s| {
            format!(", {}", quickai_core::profile::check::human_bytes(s))
        });
        let _ = writeln!(out, "Model:   {} ({}{size})", pick.display(), pick.role);
    }
    if configuration.traffic_proxy == Some(true) {
        let _ = writeln!(out, "Live traffic proxy: on");
    }
    let settings_lines: Vec<(String, String)> = carried(&registry, &configuration.engine)
        .into_iter()
        .filter_map(|(key, field)| {
            let value = configuration.values.get(&key)?;
            Some((label_of(field), full(value)))
        })
        .collect();
    if !settings_lines.is_empty() {
        let _ = writeln!(out, "Settings:");
        for (label, value) in settings_lines {
            let mut lines = value.lines();
            let _ = writeln!(out, "  {label}: {}", lines.next().unwrap_or_default());
            for more in lines {
                let _ = writeln!(out, "      {more}");
            }
        }
    }
    if !report.notes.is_empty() {
        let _ = writeln!(out, "On this computer:");
        print_notes(&registry, &report, out);
    }
    0
}

fn full(value: &quickai_core::value::Value) -> String {
    match value {
        quickai_core::value::Value::Str(text) => text.clone(),
        other => shown(other),
    }
}

fn check(args: &[String], dirs: &ConfigDirs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut strict = false;
    let mut file = None;
    for arg in args {
        match arg.as_str() {
            "--strict" => strict = true,
            other if other.starts_with('-') => {
                return usage(err, &format!("unknown option `{other}`"))
            }
            other if file.is_none() => file = Some(other.to_string()),
            _ => return usage(err, "`configuration check` takes one file"),
        }
    }
    let Some(file) = file else {
        return usage(err, "`configuration check` needs a file");
    };
    let registry = build_registry(dirs);
    let text = match read_file(&file) {
        Ok(text) => text,
        Err(message) => {
            let _ = writeln!(err, "quickai: {message}");
            return 1;
        }
    };
    let opened = match open_text(&registry, &text, env!("CARGO_PKG_VERSION")) {
        Ok(opened) => opened,
        Err(stop) => {
            let _ = writeln!(
                out,
                "  {:<6} {}",
                word(stop.level),
                stop.text(registry.vocabulary())
            );
            let _ = writeln!(out, "configuration: cannot be used");
            return 1;
        }
    };
    let computer = detect(&registry, dirs);
    let settings = settings_of(dirs);
    let inv = computer.inventory(&registry, dirs, Some(&opened.configuration), &settings);
    let report = review(&registry, &opened.configuration, &opened.notes, &inv, true);
    let _ = writeln!(
        out,
        "{} ({}), for {}",
        opened.configuration.name, opened.configuration.id, opened.configuration.engine
    );
    print_notes(&registry, &report, out);
    for ask in report.asks() {
        if let Some(value) = ask
            .subject
            .as_ref()
            .and_then(|k| opened.configuration.values.get(k))
        {
            let _ = writeln!(
                out,
                "         {} = {}",
                ask.subject.as_deref().unwrap_or_default(),
                full(value)
            );
        }
    }
    let count = |level: Level| report.notes.iter().filter(|n| n.level == level).count();
    let (stops, asks, warns) = (count(Level::Stops), count(Level::Asks), count(Level::Warns));
    let _ = writeln!(
        out,
        "configuration: {stops} stops, {asks} asks, {warns} warns"
    );
    i32::from(stops > 0 || (strict && (asks > 0 || warns > 0)))
}

fn export_command(
    args: &[String],
    dirs: &ConfigDirs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let (mut id, mut file, mut note) = (None, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-o" | "--output" => match it.next() {
                Some(value) => file = Some(PathBuf::from(value)),
                None => return usage(err, "`-o` needs a file"),
            },
            "--note" => match it.next() {
                Some(value) => note = Some(value.clone()),
                None => return usage(err, "`--note` needs a text"),
            },
            other if other.starts_with('-') => {
                return usage(err, &format!("unknown option `{other}`"))
            }
            other if id.is_none() => id = Some(other.to_string()),
            _ => return usage(err, "`configuration export` takes one id"),
        }
    }
    let Some(id) = id else {
        return usage(err, "`configuration export` needs an id");
    };
    let registry = build_registry(dirs);
    let machine = MachineFacts::detect(crate::hardware::total_ram_bytes(), &crate::gpu::detect());
    let host = Host::from_machine(&machine);
    let (text, left_out) = match export_text(&registry, &id, None, note.as_deref(), &host) {
        Ok(done) => done,
        Err(e) => {
            let _ = writeln!(err, "quickai: {e}");
            return 1;
        }
    };
    for note in left_out {
        let _ = writeln!(err, "quickai: {}", note.text(registry.vocabulary()));
    }
    match file {
        Some(path) => {
            if let Err(e) = std::fs::write(&path, text) {
                let _ = writeln!(err, "quickai: could not write {}: {e}", path.display());
                return 1;
            }
            0
        }
        None => {
            let _ = write!(out, "{text}");
            0
        }
    }
}

fn import_command(
    args: &[String],
    dirs: &ConfigDirs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let (mut file, mut called, mut on_taken, mut accept) = (None, None, OnTaken::Refuse, false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--as" => match it.next() {
                Some(value) => called = Some(value.clone()),
                None => return usage(err, "`--as` needs a name"),
            },
            "--replace" => on_taken = OnTaken::Replace,
            "--keep-both" => on_taken = OnTaken::KeepBoth,
            "--accept-review" => accept = true,
            other if other.starts_with('-') => {
                return usage(err, &format!("unknown option `{other}`"))
            }
            other if file.is_none() => file = Some(other.to_string()),
            _ => return usage(err, "`configuration import` takes one file"),
        }
    }
    let Some(file) = file else {
        return usage(err, "`configuration import` needs a file");
    };
    let registry = build_registry(dirs);
    let text = match read_file(&file) {
        Ok(text) => text,
        Err(message) => {
            let _ = writeln!(err, "quickai: {message}");
            return 1;
        }
    };
    let computer = detect(&registry, dirs);
    let settings = settings_of(dirs);
    // what a file asks about is looked at first, and taken only when the person says so
    let ticked: BTreeSet<String> = if accept {
        match open_text(&registry, &text, env!("CARGO_PKG_VERSION")) {
            Ok(opened) => {
                let inv =
                    computer.inventory(&registry, dirs, Some(&opened.configuration), &settings);
                review(&registry, &opened.configuration, &opened.notes, &inv, true)
                    .asks()
                    .filter_map(|n| n.subject.clone())
                    .collect()
            }
            Err(_) => BTreeSet::new(),
        }
    } else {
        BTreeSet::new()
    };
    let inv = match open_text(&registry, &text, env!("CARGO_PKG_VERSION")) {
        Ok(opened) => computer.inventory(&registry, dirs, Some(&opened.configuration), &settings),
        Err(_) => computer.inventory(&registry, dirs, None, &settings),
    };
    match import(
        dirs,
        &registry,
        &text,
        &inv,
        &ticked,
        called.as_deref(),
        on_taken,
        true,
    ) {
        Ok(done) => {
            print_notes(&registry, &done.report, out);
            let not_taken: Vec<&str> = done
                .report
                .asks()
                .filter_map(|n| n.subject.as_deref())
                .filter(|k| !ticked.contains(*k))
                .collect();
            for key in not_taken {
                let _ = writeln!(
                    out,
                    "  not taken: {key} (use --accept-review to take what is asked about)"
                );
            }
            let _ = writeln!(
                out,
                "imported as {} ({})",
                done.configuration.name, done.configuration.id
            );
            0
        }
        Err(report) => {
            print_notes(&registry, &report, out);
            if report.has("name_taken") {
                let _ = writeln!(
                    err,
                    "quickai: that name is used; use --as NAME, --keep-both or --replace"
                );
            }
            let _ = writeln!(err, "quickai: nothing was stored");
            1
        }
    }
}

/// What `quickai plan --configuration` starts from: the configuration, its model found, its settings.
pub struct PlanBase {
    pub engine: String,
    pub values: std::collections::BTreeMap<String, quickai_core::value::Value>,
    pub model: Option<String>,
    pub proxy: bool,
}

/// The configuration of `quickai plan --configuration`: applied to nothing, so that what it gives is
/// what is planned, and the fields' defaults fill in the rest.
pub fn plan_base(registry: &Registry, target: &str) -> Result<PlanBase, String> {
    let (configuration, _, _) = resolve(registry, target)?;
    let applied = quickai_core::profile::state::apply(registry, &configuration)
        .map_err(|stop| stop.text(registry.vocabulary()))?;
    let state = WorkingState::default().after(registry, &applied, None, None);
    Ok(PlanBase {
        engine: configuration.engine.clone(),
        values: applied.set.clone(),
        model: configuration
            .models
            .first()
            .map(|pick| pick.source.label().to_string()),
        proxy: state.traffic_proxy,
    })
}
