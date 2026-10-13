//! Persists the last-used model/port/context-size/GPU-layers across runs, as
//! a small JSON file in the OS config dir (so it survives even though a
//! packaged build's own directory may be read-only, e.g. inside an AppImage).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quickai_core::registry::Engine;
use quickai_core::value::Value;
use serde::{Deserialize, Serialize};

/// Where the chosen engine is kept: `values."app.engine"`.
pub const ENGINE_KEY: &str = "app.engine";

/// The engine a file with no choice in it means.
pub const DEFAULT_ENGINE: &str = "llamacpp";

/// Where the id of the active configuration is kept: `values."app.configuration"`, absent while it is
/// the Default (docs/configurations.md, section 6), so that the file of a person who does not use
/// configurations is what it always was.
pub const ACTIVE_CONFIGURATION_KEY: &str = "app.configuration";

/// Where the hashes of the documents the person said may run a program are kept:
/// `values."app.confirmed_engines"`, a list of texts.
pub const CONFIRMED_ENGINES_KEY: &str = "app.confirmed_engines";

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub last_model_filename: Option<String>,
    pub port: u16,
    pub ctx_size: u32,
    pub n_gpu_layers: u32,
    /// Defaults to `true` (egui's own default) so settings files saved
    /// before this field existed keep the same look after an upgrade.
    #[serde(default = "default_dark_mode")]
    pub dark_mode: bool,
    /// `None` unless the user explicitly picked a custom downloaded-models
    /// folder in ⚙ Settings — otherwise `paths::user_models_dir()` is
    /// recomputed fresh on every launch, which matters if this file is ever
    /// copied to a different machine/username.
    #[serde(default)]
    pub models_dir_override: Option<String>,
    /// Same idea as `models_dir_override`, for a custom llama-server binary.
    #[serde(default)]
    pub llama_bin_override: Option<String>,
    /// Extra command-line arguments appended verbatim to the llama-server
    /// invocation, e.g. `--threads 8 --flash-attn on`. Empty by default.
    #[serde(default)]
    pub extra_args: String,
    /// Off by default: clients connect straight to llama-server with no
    /// extra hop. When on, QuickAI puts a small local proxy in front of
    /// llama-server (see `proxy.rs`) so the 📡 Live window can show
    /// chat/completion traffic as it happens.
    #[serde(default)]
    pub enable_traffic_proxy: bool,
    /// Everything else in the file: `values` (the settings of fields that have no `legacy_key`,
    /// keyed `<engine>.<field>`) and any key this version does not know. Kept so that saving never
    /// loses what a newer version, or an engine whose document is gone, stored.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

fn default_dark_mode() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            last_model_filename: None,
            port: 8080,
            ctx_size: 4096,
            n_gpu_layers: 999,
            dark_mode: true,
            models_dir_override: None,
            llama_bin_override: None,
            extra_args: String::new(),
            enable_traffic_proxy: false,
            other: serde_json::Map::new(),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("dev", "quickai", "quickai")?;
    Some(dirs.config_dir().join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        config_path()
            .map(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        self.save_to(&path);
    }

    /// The settings stored at `path`; the defaults when the file is missing, unreadable, not JSON
    /// or of the wrong shape (a wrongly typed value discards the whole file, as it always has).
    pub(crate) fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| Self::from_json(&s))
            .unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Some(json) = self.to_json() {
            let _ = std::fs::write(path, json);
        }
    }

    /// The values this file holds for the fields of `engine`, keyed `<engine>.<field>`: a field with a
    /// `legacy_key` is read from that key at the top level, any other from `values`. A key that is
    /// null or absent is a value that is not set, and is left out.
    pub fn engine_values(&self, engine: &Engine) -> BTreeMap<String, Value> {
        let object = self.as_object();
        let stored = object.get("values").and_then(serde_json::Value::as_object);
        let mut found = BTreeMap::new();
        for field in &engine.doc.fields {
            let key = format!("{}.{}", engine.id, field.id);
            let value = match &field.legacy_key {
                Some(legacy) => object.get(legacy),
                None => stored.and_then(|values| values.get(&key)),
            };
            if let Some(value) = value.filter(|v| !v.is_null()) {
                found.insert(key, Value::from_json(value));
            }
        }
        found
    }

    /// Stores `values` for the fields of `engine` into the shape of the file. A value that is not
    /// set is cleared: a field with a `legacy_key` returns to what a first run holds for that key,
    /// any other is removed from `values`. Values of other engines stay where they are. A value that
    /// does not fit its key (a port that is text) is refused, and `self` is left as it was.
    pub fn set_engine_values(
        &mut self,
        engine: &Engine,
        values: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        let mut object = self.as_object();
        let defaults = Settings::default().as_object();
        let mut stored = match object.get("values") {
            Some(serde_json::Value::Object(existing)) => existing.clone(),
            _ => serde_json::Map::new(),
        };
        for field in &engine.doc.fields {
            let key = format!("{}.{}", engine.id, field.id);
            let value = values
                .get(&key)
                .filter(|v| !v.is_null())
                .map(Value::to_json);
            match &field.legacy_key {
                Some(legacy) => {
                    let value = value.unwrap_or_else(|| {
                        defaults
                            .get(legacy)
                            .cloned()
                            .unwrap_or(serde_json::Value::Null)
                    });
                    object.insert(legacy.clone(), value);
                }
                None => match value {
                    Some(value) => {
                        stored.insert(key, value);
                    }
                    None => {
                        stored.remove(&key);
                    }
                },
            }
        }
        // A `values` entry that is not an object is not ours to rewrite; it stays unless there is
        // something to store in its place.
        let foreign = matches!(object.get("values"), Some(v) if !v.is_object());
        if !stored.is_empty() {
            object.insert("values".to_string(), serde_json::Value::Object(stored));
        } else if !foreign {
            object.remove("values");
        }
        *self = serde_json::from_value(serde_json::Value::Object(object))
            .map_err(|e| format!("a stored value does not fit its setting: {e}"))?;
        Ok(())
    }

    /// The engine the file says was chosen, `None` when it says nothing, which is llama.cpp (a
    /// llama.cpp user's file has no such key).
    pub fn chosen_engine(&self) -> Option<String> {
        self.other
            .get("values")
            .and_then(serde_json::Value::as_object)
            .and_then(|values| values.get(ENGINE_KEY))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    }

    /// Records the choice of an engine. llama.cpp is what a file with no choice means, so choosing
    /// it takes the key away again: a llama.cpp user's file keeps exactly the keys it has always
    /// had. The settings of every engine, the one chosen or not, are left as they are.
    pub fn set_chosen_engine(&mut self, id: &str) {
        let mut values = match self.other.get("values") {
            Some(serde_json::Value::Object(existing)) => existing.clone(),
            _ => serde_json::Map::new(),
        };
        if id == DEFAULT_ENGINE {
            values.remove(ENGINE_KEY);
        } else {
            values.insert(ENGINE_KEY.to_string(), serde_json::Value::from(id));
        }
        if values.is_empty() {
            self.other.remove("values");
        } else {
            self.other
                .insert("values".to_string(), serde_json::Value::Object(values));
        }
    }

    /// The id of the configuration the person chose, `None` for the Default (which a file with no choice
    /// in it means).
    pub fn active_configuration(&self) -> Option<String> {
        self.other
            .get("values")
            .and_then(serde_json::Value::as_object)
            .and_then(|values| values.get(ACTIVE_CONFIGURATION_KEY))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    }

    /// Records the configuration chosen. The Default is what a file with no choice means, so choosing
    /// it takes the key away again; everything else in the file is left as it is.
    #[allow(dead_code)] // the windows of WP4.17.5 are its user; the tests use it now
    pub fn set_active_configuration(&mut self, id: &str) {
        let mut values = match self.other.get("values") {
            Some(serde_json::Value::Object(existing)) => existing.clone(),
            _ => serde_json::Map::new(),
        };
        if id == crate::configurations::DEFAULT_ID {
            values.remove(ACTIVE_CONFIGURATION_KEY);
        } else {
            values.insert(
                ACTIVE_CONFIGURATION_KEY.to_string(),
                serde_json::Value::from(id),
            );
        }
        if values.is_empty() {
            self.other.remove("values");
        } else {
            self.other
                .insert("values".to_string(), serde_json::Value::Object(values));
        }
    }

    /// The hashes of the engine documents the person has said may start the program they name (see
    /// `quickai_core::guard`).
    pub fn confirmed_engines(&self) -> Vec<String> {
        self.other
            .get("values")
            .and_then(serde_json::Value::as_object)
            .and_then(|values| values.get(CONFIRMED_ENGINES_KEY))
            .and_then(serde_json::Value::as_array)
            .map(|hashes| {
                hashes
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Remembers that the person said yes to the document with this hash.
    pub fn confirm_engine(&mut self, hash: &str) {
        let mut hashes = self.confirmed_engines();
        if hashes.iter().any(|h| h == hash) {
            return;
        }
        hashes.push(hash.to_string());
        let mut values = match self.other.get("values") {
            Some(serde_json::Value::Object(existing)) => existing.clone(),
            _ => serde_json::Map::new(),
        };
        values.insert(
            CONFIRMED_ENGINES_KEY.to_string(),
            serde_json::Value::from(hashes),
        );
        self.other
            .insert("values".to_string(), serde_json::Value::Object(values));
    }

    /// The file as a JSON object: the nine keys and everything in `other`, flattened.
    fn as_object(&self) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(object)) => object,
            _ => serde_json::Map::new(),
        }
    }

    fn from_json(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }

    fn to_json(&self) -> Option<String> {
        serde_json::to_string_pretty(self).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A settings.json written before models_dir_override/llama_bin_override/
    /// extra_args existed must still load — those fields fall back to their
    /// defaults via `#[serde(default)]` rather than failing to parse.
    #[test]
    fn loads_settings_json_missing_newer_fields() {
        let old_json = r#"{
            "last_model_filename": "Bonsai-8B-Q1_0.gguf",
            "port": 8080,
            "ctx_size": 4096,
            "n_gpu_layers": 999
        }"#;
        let settings: Settings = serde_json::from_str(old_json).unwrap();
        assert_eq!(
            settings.last_model_filename,
            Some("Bonsai-8B-Q1_0.gguf".to_string())
        );
        assert!(settings.dark_mode);
        assert_eq!(settings.models_dir_override, None);
        assert_eq!(settings.llama_bin_override, None);
        assert_eq!(settings.extra_args, "");
        assert!(!settings.enable_traffic_proxy);
    }

    // ---- compatibility contract (docs/implementation-plan.md, WP0.5) ---------------------------
    //
    // `settings.json` keeps these nine keys at its top level for as long as the GUI suite and users'
    // existing files depend on them; the declarative core maps fields onto them with `legacy_key`
    // (architecture.md section 6.5). The goldens pin today's exact output.

    const LEGACY_KEYS: [&str; 9] = [
        "last_model_filename",
        "port",
        "ctx_size",
        "n_gpu_layers",
        "dark_mode",
        "models_dir_override",
        "llama_bin_override",
        "extra_args",
        "enable_traffic_proxy",
    ];

    fn populated() -> Settings {
        Settings {
            last_model_filename: Some("Qwen3-4B-Q4_K_M.gguf".to_string()),
            port: 8099,
            ctx_size: 8192,
            n_gpu_layers: 20,
            dark_mode: false,
            models_dir_override: Some("/data/models".to_string()),
            llama_bin_override: Some("/opt/llama/llama-server".to_string()),
            extra_args: "--threads 8 --flash-attn on".to_string(),
            enable_traffic_proxy: true,
            other: serde_json::Map::new(),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("quickai-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn keys_of(json: &str) -> Vec<String> {
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    }

    /// A18 of docs/phase3-test-plan.md: choosing the Default leaves a llama.cpp user's file with the nine
    /// keys it has always had, a first run's file exactly the golden one, and what a configuration does
    /// not carry (the theme, the folders, a program of the person's own) where it was.
    #[test]
    fn p3_a18_choosing_the_default_leaves_the_nine_keys_and_the_goldens_unchanged() {
        use crate::config::{build_registry, ConfigDirs};
        use crate::configurations::{choose, store_state, Installed};
        use quickai_core::profile::check::Inventory;
        let registry = build_registry(&ConfigDirs::default());
        let default = registry.profile("default").unwrap().configuration.clone();
        let mut want: Vec<String> = LEGACY_KEYS.iter().map(|k| k.to_string()).collect();
        want.sort();

        // a first run
        let mut first = Settings::default();
        let choice = choose(
            &registry,
            &first,
            &default,
            &Inventory::default(),
            &Installed::new(),
        )
        .unwrap();
        store_state(&registry, &mut first, &choice.state).unwrap();
        first.set_active_configuration("default");
        crate::golden::assert_golden("settings_default.json", &first.to_json().unwrap());

        // a person who has set everything
        let mut tuned = populated();
        let choice = choose(
            &registry,
            &tuned,
            &default,
            &Inventory::default(),
            &Installed::new(),
        )
        .unwrap();
        store_state(&registry, &mut tuned, &choice.state).unwrap();
        tuned.set_active_configuration("default");
        let json = tuned.to_json().unwrap();
        assert_eq!(keys_of(&json), want);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["port"], 8080);
        assert_eq!(value["ctx_size"], 4096);
        assert_eq!(value["n_gpu_layers"], 999);
        assert_eq!(value["extra_args"], "");
        assert_eq!(value["last_model_filename"], serde_json::Value::Null);
        assert_eq!(value["enable_traffic_proxy"], false);
        // the theme, the folder and the program are theirs, not a configuration's
        assert_eq!(value["dark_mode"], false);
        assert_eq!(value["models_dir_override"], "/data/models");
        assert_eq!(value["llama_bin_override"], "/opt/llama/llama-server");
    }

    #[test]
    fn default_settings_match_the_golden_file() {
        crate::golden::assert_golden(
            "settings_default.json",
            &Settings::default().to_json().unwrap(),
        );
    }

    #[test]
    fn populated_settings_match_the_golden_file() {
        crate::golden::assert_golden("settings_populated.json", &populated().to_json().unwrap());
    }

    #[test]
    fn a_saved_file_has_exactly_the_nine_legacy_keys() {
        let mut want: Vec<String> = LEGACY_KEYS.iter().map(|k| k.to_string()).collect();
        want.sort();
        assert_eq!(keys_of(&Settings::default().to_json().unwrap()), want);
        assert_eq!(keys_of(&populated().to_json().unwrap()), want);
    }

    #[test]
    fn an_old_file_is_upgraded_to_all_nine_keys_when_saved() {
        let old =
            r#"{"last_model_filename": null, "port": 8081, "ctx_size": 2048, "n_gpu_layers": 0}"#;
        let settings = Settings::from_json(old).unwrap();
        let mut want: Vec<String> = LEGACY_KEYS.iter().map(|k| k.to_string()).collect();
        want.sort();
        assert_eq!(keys_of(&settings.to_json().unwrap()), want);
        assert_eq!(settings.port, 8081);
        assert!(settings.dark_mode);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = scratch("roundtrip");
        let path = dir.join("nested").join("settings.json");
        populated().save_to(&path);
        assert!(path.is_file(), "save_to creates missing parent directories");
        assert_eq!(
            Settings::load_from(&path).to_json().unwrap(),
            populated().to_json().unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_unparseable_file_gives_the_defaults() {
        let dir = scratch("fallbacks");
        std::fs::create_dir_all(&dir).unwrap();
        let defaults = Settings::default().to_json().unwrap();

        assert_eq!(
            Settings::load_from(&dir.join("absent.json"))
                .to_json()
                .unwrap(),
            defaults
        );

        let garbage = dir.join("garbage.json");
        std::fs::write(&garbage, "{ not json").unwrap();
        assert_eq!(Settings::load_from(&garbage).to_json().unwrap(), defaults);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The GUI suite's seeding helper relies on this: a wrongly typed value does not fall back
    /// field by field, it discards the whole file.
    #[test]
    fn a_wrongly_typed_value_discards_the_whole_file() {
        let dir = scratch("wrongtype");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"last_model_filename": null, "port": "8081", "ctx_size": 1024, "n_gpu_layers": 0}"#,
        )
        .unwrap();
        let loaded = Settings::load_from(&path);
        assert_eq!(loaded.port, 8080);
        assert_eq!(
            loaded.ctx_size, 4096,
            "ctx_size from the bad file must not survive"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- unknown keys and per-field persistence (docs/implementation-plan.md, WP2.2) ----------------

    /// A key this version does not know is kept when the file is saved: removing an engine document
    /// must not lose what the user stored for it, and an older build must not erase what a newer one
    /// wrote. (Until Phase 2 such a key was dropped on the next save.)
    #[test]
    fn unknown_keys_survive_a_load_and_a_save() {
        let dir = scratch("unknown");
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            r#"{"last_model_filename": null, "port": 8080, "ctx_size": 4096, "n_gpu_layers": 999,
                "future_key": {"a": [1, 2.5, null, "x"], "b": {"c": true}}, "also": 7}"#,
        )
        .unwrap();
        Settings::load_from(&path).save_to(&path);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved["future_key"],
            serde_json::json!({"a": [1, 2.5, null, "x"], "b": {"c": true}})
        );
        assert_eq!(saved["also"], 7);
        assert_eq!(saved["port"], 8080, "the keys it does know are still there");

        // saving again changes nothing: the file is stable
        let first = std::fs::read_to_string(&path).unwrap();
        Settings::load_from(&path).save_to(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_known_keys_keep_their_order_and_the_extras_follow() {
        let settings = Settings::from_json(
            r#"{"zzz": 1, "last_model_filename": null, "port": 8080, "ctx_size": 4096, "n_gpu_layers": 999, "aaa": 2}"#,
        )
        .unwrap();
        let text = settings.to_json().unwrap();
        let at = |key: &str| text.find(&format!("\"{key}\"")).unwrap();
        assert!(at("last_model_filename") < at("port") && at("enable_traffic_proxy") < at("aaa"));
        assert!(at("aaa") < at("zzz"), "extras are sorted by name: {text}");
    }

    const DEMO: &str = r#"schema = 1
kind = "engine"
id = "demo"
requires = { primitives = ["spawn@1", "locate.setting@1"] }

[engine]
name = "Demo"

[locate]
strategies = [ { use = "setting", field = "bin" } ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${runtime.binary}"
args = []

[[field]]
id = "port"
type = "port"
default = 9000
legacy_key = "port"
bind.port = "public"

[[field]]
id = "bin"
type = "runtime_path"
legacy_key = "llama_bin_override"

[[field]]
id = "mode"
type = "choice"
options = ["auto", "attach"]
default = "auto"

[[field]]
id = "scale"
type = "float"
default = 1.5
"#;

    /// An engine that has fields of both persistence shapes: two with a `legacy_key`, two without.
    fn demo_registry() -> quickai_core::registry::Registry {
        use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
        let source = MemorySource::new(&[("embedded:engines/demo.toml", DEMO)]);
        let registry = quickai_core::registry::Registry::build(
            load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
            &crate::prims::known(),
        );
        assert!(
            registry.diagnostics.is_empty(),
            "{:#?}",
            registry.diagnostics
        );
        registry
    }

    fn llama_registry() -> quickai_core::registry::Registry {
        crate::config::build_registry(&crate::config::ConfigDirs::default())
    }

    fn v(value: impl Into<quickai_core::value::Value>) -> quickai_core::value::Value {
        value.into()
    }

    #[test]
    fn legacy_fields_are_read_from_their_top_level_keys() {
        let registry = llama_registry();
        let engine = registry.engine("llamacpp").unwrap();
        let values = populated().engine_values(engine);
        assert_eq!(values["llamacpp.port"], v(8099u16));
        assert_eq!(values["llamacpp.ctx_size"], v(8192u32));
        assert_eq!(values["llamacpp.n_gpu_layers"], v(20u32));
        assert_eq!(
            values["llamacpp.extra_args"],
            v("--threads 8 --flash-attn on")
        );
        assert_eq!(
            values["llamacpp.bin_override"],
            v("/opt/llama/llama-server")
        );
        // a null key is a value that is not there
        let none = Settings::default().engine_values(engine);
        assert!(!none.contains_key("llamacpp.bin_override"));
        assert_eq!(
            none["llamacpp.ctx_size"],
            v(4096u32),
            "a first run holds today's defaults"
        );
    }

    #[test]
    fn legacy_fields_are_written_to_their_top_level_keys_and_the_file_keeps_its_nine_keys() {
        let registry = llama_registry();
        let engine = registry.engine("llamacpp").unwrap();
        let mut settings = Settings::default();
        let mut values = settings.engine_values(engine);
        values.insert("llamacpp.port".into(), v(7000u16));
        values.insert("llamacpp.ctx_size".into(), v(2048u32));
        values.insert("llamacpp.bin_override".into(), v("/x/llama-server"));
        settings.set_engine_values(engine, &values).unwrap();
        assert_eq!((settings.port, settings.ctx_size), (7000, 2048));
        assert_eq!(
            settings.llama_bin_override.as_deref(),
            Some("/x/llama-server")
        );
        let mut want: Vec<String> = LEGACY_KEYS.iter().map(|k| k.to_string()).collect();
        want.sort();
        assert_eq!(
            keys_of(&settings.to_json().unwrap()),
            want,
            "nothing else is added, no `values`"
        );
    }

    #[test]
    fn a_cleared_value_goes_back_to_null_or_to_the_files_default() {
        let registry = llama_registry();
        let engine = registry.engine("llamacpp").unwrap();
        let mut settings = populated();
        let mut values = settings.engine_values(engine);
        values.remove("llamacpp.bin_override");
        values.remove("llamacpp.port");
        settings.set_engine_values(engine, &values).unwrap();
        assert_eq!(
            settings.llama_bin_override, None,
            "an optional value is cleared"
        );
        assert_eq!(
            settings.port, 8080,
            "a value the file always holds returns to its default"
        );
        assert_eq!(settings.ctx_size, 8192, "the others are untouched");
    }

    #[test]
    fn plain_fields_live_under_values_and_legacy_ones_never_do() {
        let registry = demo_registry();
        let engine = registry.engine("demo").unwrap();
        let mut settings = Settings::default();
        let values = std::collections::BTreeMap::from([
            ("demo.port".to_string(), v(9100u16)),
            ("demo.bin".to_string(), v("/x")),
            ("demo.mode".to_string(), v("attach")),
            ("demo.scale".to_string(), v(2.5)),
        ]);
        settings.set_engine_values(engine, &values).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&settings.to_json().unwrap()).unwrap();
        assert_eq!(saved["port"], 9100, "a field with a legacy_key keeps it");
        assert_eq!(saved["llama_bin_override"], "/x");
        assert_eq!(
            saved["values"],
            serde_json::json!({"demo.mode": "attach", "demo.scale": 2.5}),
            "the others are under `values`, keyed by engine and field"
        );
        assert_eq!(
            settings.engine_values(engine),
            values,
            "and read back the same"
        );
    }

    #[test]
    fn values_survive_a_save_and_a_load() {
        let registry = demo_registry();
        let engine = registry.engine("demo").unwrap();
        let dir = scratch("values");
        let path = dir.join("settings.json");
        let mut settings = Settings::default();
        let mut values = settings.engine_values(engine);
        values.insert("demo.mode".into(), v("attach"));
        settings.set_engine_values(engine, &values).unwrap();
        settings.save_to(&path);
        assert_eq!(
            Settings::load_from(&path).engine_values(engine)["demo.mode"],
            v("attach")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_values_of_an_engine_that_is_no_longer_there_are_kept() {
        let registry = demo_registry();
        let engine = registry.engine("demo").unwrap();
        let mut settings = Settings::from_json(
            r#"{"last_model_filename": null, "port": 8080, "ctx_size": 4096, "n_gpu_layers": 999,
                "values": {"ollama.mode": "attach", "demo.mode": "auto"}}"#,
        )
        .unwrap();
        let mut values = settings.engine_values(engine);
        assert_eq!(values["demo.mode"], v("auto"));
        values.insert("demo.mode".into(), v("attach"));
        settings.set_engine_values(engine, &values).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&settings.to_json().unwrap()).unwrap();
        assert_eq!(
            saved["values"],
            serde_json::json!({"ollama.mode": "attach", "demo.mode": "attach"}),
            "the other engine's value is still there"
        );
    }

    #[test]
    fn clearing_the_last_plain_value_removes_the_values_object() {
        let registry = demo_registry();
        let engine = registry.engine("demo").unwrap();
        let mut settings = Settings::from_json(
            r#"{"last_model_filename": null, "port": 8080, "ctx_size": 4096, "n_gpu_layers": 999,
                "values": {"demo.mode": "attach"}}"#,
        )
        .unwrap();
        settings
            .set_engine_values(engine, &std::collections::BTreeMap::new())
            .unwrap();
        assert!(!settings.to_json().unwrap().contains("values"));
    }

    #[test]
    fn a_values_entry_that_is_not_an_object_is_left_alone() {
        let registry = llama_registry();
        let engine = registry.engine("llamacpp").unwrap();
        let mut settings = Settings::from_json(
            r#"{"last_model_filename": null, "port": 8080, "ctx_size": 4096, "n_gpu_layers": 999,
                "values": "oops"}"#,
        )
        .unwrap();
        let values = settings.engine_values(engine);
        settings.set_engine_values(engine, &values).unwrap();
        assert!(settings.to_json().unwrap().contains(r#""values": "oops""#));
    }

    #[test]
    fn a_value_of_the_wrong_type_is_refused_and_nothing_changes() {
        let registry = llama_registry();
        let engine = registry.engine("llamacpp").unwrap();
        for bad in [v("eighty"), v(70000u32), v(-1i64), v(true)] {
            let mut settings = populated();
            let before = settings.to_json().unwrap();
            let mut values = settings.engine_values(engine);
            values.insert("llamacpp.port".into(), bad.clone());
            assert!(
                settings.set_engine_values(engine, &values).is_err(),
                "{bad:?} cannot be a port"
            );
            assert_eq!(
                settings.to_json().unwrap(),
                before,
                "{bad:?}: nothing changed"
            );
        }
    }

    #[test]
    fn a_legacy_key_the_struct_does_not_have_is_kept_as_an_extra() {
        // a document may name a key of its own; it is stored at the top level like the others
        let registry = {
            use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
            let text = DEMO.replace(
                "id = \"scale\"",
                "id = \"scale\"\nlegacy_key = \"demo_scale\"",
            );
            let source = MemorySource::new(&[("embedded:engines/demo.toml", text.as_str())]);
            quickai_core::registry::Registry::build(
                load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
                &crate::prims::known(),
            )
        };
        let engine = registry.engine("demo").unwrap();
        let mut settings = Settings::default();
        let mut values = settings.engine_values(engine);
        values.insert("demo.scale".into(), v(3.0));
        settings.set_engine_values(engine, &values).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&settings.to_json().unwrap()).unwrap();
        assert_eq!(saved["demo_scale"], 3.0);
        assert!(saved
            .get("values")
            .is_none_or(|v| v.get("demo.scale").is_none()));
        assert_eq!(settings.engine_values(engine)["demo.scale"], v(3.0));
    }
}
