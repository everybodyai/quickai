//! What the person's own lines of environment may and may not do (plan decision C31). The planner
//! puts them into the spawn's variables and the form shows the lines that were refused; both ask
//! here, so that what the form says is wrong is exactly what the plan leaves out.

use crate::kvlines::{self, KvLines};
use crate::registry::Engine;

/// The names the lines of a field may not set: the field's own `protect` list, every variable the
/// document locks (`locked = true` in `[[launch.spawn.env]]`) and every one it adds to rather than
/// sets (`prepend`: a library path the program needs would be lost to a line that sets it).
pub fn protected_names(engine: &Engine, protect: &[String]) -> Vec<String> {
    let mut names: Vec<String> = protect.to_vec();
    if let Some(spawn) = &engine.spawn {
        for entry in spawn.env.iter().filter(|e| e.locked || e.prepend.is_some()) {
            if !names.iter().any(|n| n == &entry.name) {
                names.push(entry.name.clone());
            }
        }
    }
    names
}

/// The lines `text` of a field with `bind.env_lines.protect = protect`, read for `engine`. A
/// protected name is refused in any spelling on every system; two names are one variable only on
/// Windows (`windows`).
pub fn user_lines(engine: &Engine, protect: &[String], text: &str, windows: bool) -> KvLines {
    let names = protected_names(engine, protect);
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    kvlines::parse_with(text, &names, true, windows)
}
