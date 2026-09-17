//! Persists the last-used model/port/context-size/GPU-layers across runs, as
//! a small JSON file in the OS config dir (so it survives even though a
//! packaged build's own directory may be read-only, e.g. inside an AppImage).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
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
        assert_eq!(settings.last_model_filename, Some("Bonsai-8B-Q1_0.gguf".to_string()));
        assert!(settings.dark_mode);
        assert_eq!(settings.models_dir_override, None);
        assert_eq!(settings.llama_bin_override, None);
        assert_eq!(settings.extra_args, "");
    }
}
