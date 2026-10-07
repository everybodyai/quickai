//! What a real Ollama answered (`tests/fixtures/ollama/recorded`, made by `record.sh` against the
//! release in `index.json`), and a way to compare an answer with it by *shape*: the same keys
//! holding the same kinds of value. Values themselves (timestamps, digests, token counts) differ
//! from one run to the next and are not compared.

use std::collections::BTreeSet;

use serde_json::Value;

pub struct Rec {
    pub name: String,
    pub method: String,
    pub path: String,
    pub request: Option<Value>,
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl Rec {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The body as one JSON document.
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("recording {} is not JSON: {e}", self.name))
    }

    /// The body as lines of JSON.
    pub fn ndjson(&self) -> Vec<Value> {
        self.text()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                serde_json::from_str(l)
                    .unwrap_or_else(|e| panic!("recording {}: {l:?} is not JSON: {e}", self.name))
            })
            .collect()
    }

    /// The `data:` payloads of a server-sent stream, in order.
    pub fn sse_data(&self) -> Vec<String> {
        self.text()
            .split("\n\n")
            .filter_map(|event| event.strip_prefix("data: ").map(str::to_string))
            .collect()
    }

    /// The media type, without parameters.
    pub fn media_type(&self) -> String {
        self.content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    }
}

pub struct Recorded {
    /// The Ollama release the recordings were made against.
    pub ollama_version: String,
    pub responses: Vec<Rec>,
    /// The hosts a real pull contacted (`egress.txt`), as `host:port`.
    pub egress: Vec<String>,
    /// The hosts a real pull of an `hf.co/` reference contacted (`egress_hf.txt`).
    pub egress_hf: Vec<String>,
}

fn dir() -> std::path::PathBuf {
    super::fixtures_dir().join("ollama").join("recorded")
}

pub fn load() -> Recorded {
    let index: Value = serde_json::from_slice(
        &std::fs::read(dir().join("index.json")).expect("the recordings are in the repository"),
    )
    .expect("index.json is JSON");
    let responses = index["responses"]
        .as_array()
        .expect("a list of responses")
        .iter()
        .map(|r| Rec {
            name: r["name"].as_str().unwrap().to_string(),
            method: r["method"].as_str().unwrap().to_string(),
            path: r["path"].as_str().unwrap().to_string(),
            request: (!r["request"].is_null()).then(|| r["request"].clone()),
            status: r["status"].as_u64().unwrap() as u16,
            content_type: r["content_type"].as_str().unwrap_or("").to_string(),
            body: std::fs::read(dir().join(r["file"].as_str().unwrap()))
                .unwrap_or_else(|e| panic!("body of {}: {e}", r["name"])),
        })
        .collect();
    let hosts = |file: &str| -> Vec<String> {
        std::fs::read_to_string(dir().join(file))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    };
    Recorded {
        ollama_version: index["ollama_version"].as_str().unwrap().to_string(),
        responses,
        egress: hosts("egress.txt"),
        egress_hf: hosts("egress_hf.txt"),
    }
}

impl Recorded {
    pub fn get(&self, name: &str) -> &Rec {
        self.responses
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no recording called {name}"))
    }
}

/// The shape of a JSON value as text: `{"a":string,"b":[number]}`. Object keys are sorted; an
/// array is the set of its elements' shapes; every number is `number`. A subtree whose JSON
/// pointer is in `opaque` is just `{...}` (a map whose keys are the engine's business, such as
/// `model_info`).
pub fn shape(value: &Value, opaque: &[&str]) -> String {
    fn walk(v: &Value, here: &str, opaque: &[&str]) -> String {
        match v {
            Value::Null => "null".into(),
            Value::Bool(_) => "bool".into(),
            Value::Number(_) => "number".into(),
            Value::String(_) => "string".into(),
            Value::Array(items) => {
                let set: BTreeSet<String> = items
                    .iter()
                    .map(|i| walk(i, &format!("{here}/*"), opaque))
                    .collect();
                format!("[{}]", set.into_iter().collect::<Vec<_>>().join("|"))
            }
            Value::Object(map) => {
                if opaque.contains(&here) {
                    return "{...}".into();
                }
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let inner: Vec<String> = keys
                    .into_iter()
                    .map(|k| format!("{k:?}:{}", walk(&map[k], &format!("{here}/{k}"), opaque)))
                    .collect();
                format!("{{{}}}", inner.join(","))
            }
        }
    }
    walk(value, "", opaque)
}

/// The distinct shapes among `values`.
pub fn shapes(values: &[Value], opaque: &[&str]) -> BTreeSet<String> {
    values.iter().map(|v| shape(v, opaque)).collect()
}

/// Panics unless `got` is exactly the shape of `want`.
pub fn assert_same_shape(what: &str, want: &Value, got: &Value, opaque: &[&str]) {
    let (w, g) = (shape(want, opaque), shape(got, opaque));
    assert_eq!(
        g, w,
        "{what}: the fake's answer has another shape than the real one\n  real: {w}\n  fake: {g}"
    );
}

/// Panics when `got` holds a shape the real stream never did, or lacks the real stream's last one
/// (the line that says it is done).
pub fn assert_stream_shapes(what: &str, want: &[Value], got: &[Value], opaque: &[&str]) {
    let real = shapes(want, opaque);
    let fake = shapes(got, opaque);
    for shape in &fake {
        assert!(
            real.contains(shape),
            "{what}: the fake sends a line shaped {shape}, which the real stream never did\n  real shapes: {real:#?}"
        );
    }
    let last = shape(want.last().expect("a recorded stream has lines"), opaque);
    assert_eq!(
        shape(got.last().expect("the fake sent lines"), opaque),
        last,
        "{what}: the last line of the fake's stream has another shape than the real one's"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_recordings_load_and_are_what_the_index_says() {
        let rec = load();
        assert_eq!(rec.ollama_version, "0.35.1");
        assert!(rec.responses.len() >= 30);
        assert_eq!(rec.get("version").json(), json!({"version": "0.35.1"}));
        assert_eq!(rec.get("root").text(), "Ollama is running");
        assert_eq!(rec.get("route_unknown").status, 404);
        assert_eq!(rec.get("pull_real").media_type(), "application/x-ndjson");
        assert_eq!(
            rec.get("v1_chat_stream").sse_data().last().unwrap(),
            "[DONE]"
        );
        assert_eq!(rec.egress, ["registry.ollama.ai:443"]);
        // every request body was recorded as JSON where there was one
        assert!(rec.get("generate_load").request.is_some());
        assert!(rec.get("version").request.is_none());
    }

    #[test]
    fn shapes_ignore_values_sort_keys_and_merge_array_elements() {
        let a = json!({"b": 1, "a": ["x", "y"], "c": null});
        let b = json!({"c": null, "a": ["z"], "b": 2.5});
        assert_eq!(shape(&a, &[]), shape(&b, &[]));
        assert_eq!(shape(&a, &[]), r#"{"a":[string],"b":number,"c":null}"#);
        assert_ne!(shape(&json!({"a": 1}), &[]), shape(&json!({"a": "1"}), &[]));
        assert_ne!(
            shape(&json!({"a": 1}), &[]),
            shape(&json!({"a": 1, "b": 1}), &[])
        );
        assert_eq!(shape(&json!([]), &[]), "[]");
        assert_eq!(
            shape(&json!({"m": {"x": 1}}), &["/m"]),
            shape(&json!({"m": {"y": "z", "w": [1]}}), &["/m"])
        );
    }

    #[test]
    fn a_stream_comparison_catches_an_invented_line_and_a_missing_ending() {
        let want = vec![
            json!({"s": "a"}),
            json!({"s": "b", "n": 1}),
            json!({"s": "done", "d": true}),
        ];
        let ok = vec![json!({"s": "x"}), json!({"s": "done", "d": false})];
        assert_stream_shapes("ok", &want, &ok, &[]);
        let invented = std::panic::catch_unwind(|| {
            assert_stream_shapes(
                "x",
                &want,
                &[json!({"other": 1}), json!({"s": "done", "d": true})],
                &[],
            )
        });
        assert!(invented.is_err());
        let no_end = std::panic::catch_unwind(|| {
            assert_stream_shapes("x", &want, &[json!({"s": "a"})], &[])
        });
        assert!(no_end.is_err());
    }
}
