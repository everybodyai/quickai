//! What a string typed into the model box means to an engine whose models are named references
//! (docs/phase3-test-plan.md, group M): the document's `[models] input_rules`, first match wins.

use super::engine::CompiledInputAction;
use super::Engine;
use crate::expr::eval;
use crate::value::{map, Value};

/// What to do with what was typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputAction {
    /// Nothing was typed (or only spaces).
    Nothing,
    /// Ask the engine to pull this reference (`qwen3:4b`, `hf.co/user/repo:Q4_K_M`).
    Pull { reference: String },
    /// Do not; say this to the person.
    Reject { message: String },
}

impl Engine {
    /// The first of the document's rules that matches `input` (surrounding spaces ignored).
    /// `is_file` answers whether a path names a file on this machine: the only thing a rule may
    /// ask of the machine, which is why it is passed in. An input no rule matches is rejected
    /// with a sentence of ours, so a document that ends in a rule with no `when` never reaches it.
    pub fn classify_input(&self, input: &str, is_file: &dyn Fn(&str) -> bool) -> InputAction {
        let input = input.trim();
        if input.is_empty() {
            return InputAction::Nothing;
        }
        let Some(models) = &self.models else {
            return InputAction::Reject {
                message: format!("{} has no models to ask for.", self.doc.engine.name),
            };
        };
        let context = map([
            ("input", Value::Str(input.to_string())),
            ("input_is_file", Value::Bool(is_file(input))),
        ]);
        for rule in &models.input_rules {
            if let Some(when) = &rule.when {
                // a condition that cannot be worked out is not a match: the rules after it are tried
                if !eval(when, &context).is_ok_and(|v| v.truthy()) {
                    continue;
                }
            }
            return match &rule.action {
                CompiledInputAction::Pull(reference) => match reference.render(&context) {
                    Ok(reference) => InputAction::Pull {
                        reference: reference.to_string(),
                    },
                    Err(e) => InputAction::Reject {
                        message: format!("The name to pull could not be worked out: {}", e.message),
                    },
                },
                CompiledInputAction::Reject(message) => InputAction::Reject {
                    message: message.clone(),
                },
            };
        }
        InputAction::Reject {
            message: format!(
                "{} has no rule for that; try a model's name.",
                self.doc.engine.name
            ),
        }
    }
}
