//! The values of the engine's fields as the interface holds them: `<engine>.<field>` to a value, the
//! same keys a `Selection` and the form model use. They come from `settings.json` through each
//! field's `legacy_key` (docs/implementation-plan.md, WP2.2) and go back the same way.

use std::collections::BTreeMap;

use quickai_core::form::Edit;
use quickai_core::registry::Engine;
use quickai_core::value::Value;

use crate::settings::{Settings, DEFAULT_ENGINE};

#[derive(Clone, Debug)]
pub struct FieldValues {
    /// The engine the values belong to: their keys are `<engine>.<field>`.
    engine: String,
    values: BTreeMap<String, Value>,
}

impl Default for FieldValues {
    fn default() -> Self {
        FieldValues::new(DEFAULT_ENGINE)
    }
}

impl FieldValues {
    pub fn new(engine: &str) -> Self {
        FieldValues {
            engine: engine.to_string(),
            values: BTreeMap::new(),
        }
    }

    /// What the settings hold for the engine's fields. A first run of llama.cpp holds what it always
    /// has: port 8080, context size 4096, GPU layers 999 and no extra arguments. With no engine
    /// document (which a test of the embedded manifests rules out) there are no values.
    pub fn from_settings(settings: &Settings, engine: Option<&Engine>) -> Self {
        FieldValues {
            engine: engine.map_or(DEFAULT_ENGINE, |e| e.id.as_str()).to_string(),
            values: engine
                .map(|engine| settings.engine_values(engine))
                .unwrap_or_default(),
        }
    }

    pub fn key(&self, field: &str) -> String {
        format!("{}.{field}", self.engine)
    }

    pub fn get(&self, field: &str) -> Option<&Value> {
        self.values.get(&self.key(field))
    }

    pub fn int(&self, field: &str) -> Option<i64> {
        self.get(field).and_then(Value::as_i64)
    }

    pub fn text(&self, field: &str) -> Option<&str> {
        self.get(field).and_then(Value::as_str)
    }

    pub fn set(&mut self, field: &str, value: impl Into<Value>) {
        let key = self.key(field);
        self.values.insert(key, value.into());
    }

    pub fn clear(&mut self, field: &str) {
        let key = self.key(field);
        self.values.remove(&key);
    }

    /// Applies what the form asked for: a new value for each key, or the key cleared for a `null`.
    pub fn apply(&mut self, edits: impl IntoIterator<Item = Edit>) {
        for edit in edits {
            if edit.value.is_null() {
                self.values.remove(&edit.key);
            } else {
                self.values.insert(edit.key, edit.value);
            }
        }
    }

    pub fn map(&self) -> &BTreeMap<String, Value> {
        &self.values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{build_registry, ConfigDirs};

    #[test]
    fn a_first_run_holds_todays_defaults_and_no_override() {
        let registry = build_registry(&ConfigDirs::default());
        let values =
            FieldValues::from_settings(&Settings::default(), registry.engine(DEFAULT_ENGINE));
        assert_eq!(values.int("port"), Some(8080));
        assert_eq!(values.int("ctx_size"), Some(4096));
        assert_eq!(values.int("n_gpu_layers"), Some(999));
        assert_eq!(values.text("extra_args"), Some(""));
        assert_eq!(values.get("bin_override"), None);
        assert_eq!(values.get("models_dir"), None);
        assert_eq!(values.get("last_model"), None);
    }

    #[test]
    fn values_are_set_cleared_and_applied_under_the_engines_keys() {
        let mut values = FieldValues::default();
        values.set("port", 9000u16);
        assert_eq!(values.map().keys().collect::<Vec<_>>(), ["llamacpp.port"]);
        values.apply([
            Edit {
                key: "llamacpp.ctx_size".into(),
                value: Value::Int(2048),
            },
            Edit {
                key: "llamacpp.port".into(),
                value: Value::Null,
            },
        ]);
        assert_eq!(values.int("ctx_size"), Some(2048));
        assert_eq!(values.get("port"), None, "a null clears the value");
        values.clear("ctx_size");
        assert!(values.map().is_empty());
    }

    #[test]
    fn no_engine_means_no_values() {
        assert!(FieldValues::from_settings(&Settings::default(), None)
            .map()
            .is_empty());
    }
}
