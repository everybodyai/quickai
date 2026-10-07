//! The `[[diagnostic]]` rules of an engine document (work package 3.7, docs/phase3-test-plan.md
//! E14): when each applies, what a rule that is wrongly written does at load, and that the advice is
//! the document's and not the code's.

use quickai_core::advice::{advice, Advice};
use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
use quickai_core::plan::{HostFacts, Selection};
use quickai_core::registry::{KnownPrimitives, Registry};

const BASE: &str = r#"schema = 1
kind = "engine"
id = "acme"
requires = { primitives = ["spawn@1", "probe.http@1", "locate.setting@1"] }

[engine]
name = "Acme"

[provides]
apis = ["acme.talk"]
default_port = 9000

[locate]
strategies = [ { use = "setting", field = "bin_override" } ]

[launch]
strategies = ["spawn"]

[launch.spawn]
program = "${runtime.binary}"
args = ["serve"]

[launch.ready]
probe = { use = "http", path = "/health", expect = 200 }

[[field]]
id = "port"
type = "port"
default = 9000
bind.port = "public"

[[field]]
id = "bin_override"
type = "runtime_path"

RULES
"#;

const RULES: &str = r#"[[diagnostic]]
when = { log_matches = 'address already in use' }
hint = "Port ${field.port} is taken."
action = "focus_field:port"

[[diagnostic]]
when = { exited_during = "starting" }
hint = "It exited before it was ready; the log has the reason."

[[diagnostic]]
when = { log_matches = '(?i)out of memory', exited_during = "running" }
hint = "It ran out of memory."
"#;

fn registry(rules: &str) -> Registry {
    let text = BASE.replace("RULES", rules);
    let source = MemorySource::new(&[("embedded:engines/acme.toml", &text)]);
    Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &KnownPrimitives::new(&["spawn@1", "probe.http@1", "locate.setting@1"]),
    )
}

fn said(registry: &Registry, log: &[&str], ended_in: Option<&str>) -> Vec<Advice> {
    let log: Vec<String> = log.iter().map(|l| l.to_string()).collect();
    advice(
        registry,
        &Selection::new("acme"),
        &HostFacts::default(),
        &log,
        ended_in,
    )
}

#[test]
fn p3_e14b_a_rule_applies_when_every_condition_it_names_holds() {
    let registry = registry(RULES);
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    // the log alone
    let taken = said(&registry, &["listen: address already in use"], None);
    assert_eq!(
        taken,
        [Advice {
            text: "Port 9000 is taken.".into(),
            action: Some("focus_field:port".into())
        }]
    );
    // the state alone: whatever the log says
    let early = said(&registry, &[], Some("starting"));
    assert_eq!(early.len(), 1);
    assert!(early[0].text.starts_with("It exited before it was ready"));
    assert_eq!(said(&registry, &[], Some("running")), []);
    assert_eq!(said(&registry, &[], None), []);
    // both: the log says it and the run ended in that state
    assert_eq!(
        said(&registry, &["OUT OF MEMORY"], Some("running"))[0].text,
        "It ran out of memory."
    );
    assert_eq!(
        said(&registry, &["out of memory"], Some("starting")).len(),
        1,
        "only the state rule: the memory one is for a run that was running"
    );
    assert_eq!(said(&registry, &["out of memory"], None), []);
    // the order is the document's, and a piece of advice is said once
    let both = said(
        &registry,
        &["address already in use", "address already in use"],
        Some("starting"),
    );
    assert_eq!(both.len(), 2, "{both:?}");
    assert!(both[0].text.contains("Port 9000"), "{both:?}");
}

#[test]
fn p3_e14c_a_rule_written_wrongly_is_a_load_diagnostic_at_its_line() {
    let cases: [(&str, &str, &str); 6] = [
        (
            "log_matches = 'address already in use'",
            "log_matches = '(unclosed'",
            "is not a valid pattern",
        ),
        (
            "exited_during = \"starting\"",
            "exited_during = \"stopping\"",
            "`stopping` is not a state a run ends in",
        ),
        (
            "action = \"focus_field:port\"",
            "action = \"Focus Field\"",
            "`Focus Field` is not an action id",
        ),
        (
            "hint = \"Port ${field.port} is taken.\"",
            "hint = \"Port ${field.prot} is taken.\"",
            "`field` has no `prot`",
        ),
        (
            "when = { exited_during = \"starting\" }",
            "when = { }",
            "a diagnostic says when it applies",
        ),
        (
            "hint = \"It ran out of memory.\"",
            "hint = \"It ran out of ${memory\"",
            "never closed",
        ),
    ];
    for (old, new, wanted) in cases {
        assert!(RULES.contains(old), "the rules have no `{old}`");
        let registry = registry(&RULES.replace(old, new));
        assert!(registry.engine("acme").is_none(), "`{new}` loaded");
        let errors: Vec<(Option<u32>, String)> = registry
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| (d.line, d.to_string()))
            .collect();
        assert!(
            errors
                .iter()
                .any(|(line, text)| text.contains(wanted) && line.is_some()),
            "`{new}` should be reported with `{wanted}` and a line: {errors:#?}"
        );
    }
}
