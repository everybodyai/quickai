//! Group C of the test plan: the fakes are held to what a real Ollama did.
//!
//! The recordings in `tests/fixtures/ollama/recorded` were made by `record.sh` against a real
//! release. C01 replays every recorded request against the Rust fake and compares the answer by
//! shape; the same is done for `fake_ollama.py` in the GUI suite's repository. C02 re-records with
//! a real binary and compares with what is stored, which is how the recordings are renewed when
//! Ollama is upgraded. C03 pins the facts about a real pull that the code must be written around.

use serde_json::Value;

use crate::testkit::fake_ollama::FakeOllama;
use crate::testkit::http::{send, Reply};
use crate::testkit::recorded::{assert_same_shape, assert_stream_shapes, load, shape, Rec};

/// The keys under these pointers belong to the engine (a model's metadata): not compared.
const OPAQUE: &[&str] = &["/model_info"];

fn body_of(rec: &Rec) -> Option<String> {
    rec.request.as_ref().map(|r| match r {
        // a body that was not JSON (a deliberately broken request) was recorded as text
        Value::String(raw) => raw.clone(),
        other => other.to_string(),
    })
}

fn replay(base: &str, rec: &Rec) -> Reply {
    send(
        &rec.method,
        &format!("{base}{}", rec.path),
        body_of(rec).as_deref(),
    )
}

fn ndjson_of(reply: &Reply) -> Vec<Value> {
    reply
        .body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l:?}: {e}")))
        .collect()
}

fn sse_of(reply: &Reply) -> Vec<String> {
    reply
        .body
        .split("\n\n")
        .filter_map(|e| e.strip_prefix("data: ").map(str::to_string))
        .collect()
}

fn media_type(reply: &Reply) -> String {
    reply
        .header("content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// Compares one answer with the recording of the same request.
fn assert_like(rec: &Rec, got: &Reply) {
    let what = format!("{} {} ({})", rec.method, rec.path, rec.name);
    assert_eq!(got.status, rec.status, "{what}: status");
    assert_eq!(media_type(got), rec.media_type(), "{what}: content type");
    match rec.media_type().as_str() {
        "application/x-ndjson" => {
            assert_stream_shapes(&what, &rec.ndjson(), &ndjson_of(got), OPAQUE);
        }
        "text/event-stream" => {
            let (want, have) = (rec.sse_data(), sse_of(got));
            assert_eq!(have.last(), want.last(), "{what}: how the stream ends");
            let parse = |items: &[String]| -> Vec<Value> {
                items
                    .iter()
                    .filter(|d| d.as_str() != "[DONE]")
                    .map(|d| serde_json::from_str(d).unwrap())
                    .collect()
            };
            assert_stream_shapes(&what, &parse(&want), &parse(&have), OPAQUE);
        }
        "application/json" if rec.status < 400 => {
            assert_same_shape(&what, &rec.json(), &got.json(), OPAQUE);
        }
        // an error's own sentence is part of what the application shows, so it is compared whole
        _ => assert_eq!(got.body, rec.text(), "{what}: body"),
    }
}

/// Every recorded request, in the order it was made, against the fake set up the way the real
/// server was at that point of the recording.
#[test]
fn p3_c01_the_rust_fake_answers_every_recorded_request_with_the_shape_the_real_one_did() {
    let recorded = load();
    let fake = FakeOllama::start();
    let base = fake.base();
    let mut replayed = 0;
    for rec in &recorded.responses {
        match rec.name.as_str() {
            // the import and the copy were done to put models in the store: the fake is given the
            // model directly (the application never calls these endpoints)
            "create" => {
                fake.add_model("bonsai-q1", 248_302_385);
                continue;
            }
            "copy" | "delete_ok" => continue,
            "pull_unreachable" => {
                let message = rec.ndjson().last().unwrap()["error"]
                    .as_str()
                    .unwrap()
                    .to_string();
                fake.behave(|b| b.pull.fail = Some(message));
            }
            "pull_real" => {
                fake.behave(|b| b.pull.fail = None);
            }
            _ => {}
        }
        assert_like(rec, &replay(&base, rec));
        replayed += 1;
    }
    assert!(replayed >= 28, "only {replayed} recordings were replayed");
}

/// What the recordings say about a real pull, which the code that follows one must be written
/// around (docs/engine-facts.md, O18). If a recording is renewed and one of these stops being so,
/// the design is to be read again.
#[test]
fn p3_c03_the_recordings_show_what_a_real_pull_looks_like() {
    let rec = load();
    let pull = rec.get("pull_real").ndjson();
    let status = |l: &Value| l["status"].as_str().unwrap_or("").to_string();
    assert_eq!(status(&pull[0]), "pulling manifest");
    assert_eq!(status(pull.last().unwrap()), "success");
    assert_eq!(status(&pull[pull.len() - 2]), "writing manifest");
    assert_eq!(status(&pull[pull.len() - 3]), "verifying sha256 digest");

    // a layer is announced with its total a few times before a single byte has moved
    let first_layer = &pull[1];
    assert!(first_layer["total"].is_u64() && first_layer.get("completed").is_none());
    // many lines have no `completed`, and the small layers never get one
    let with = pull.iter().filter(|l| l.get("completed").is_some()).count();
    assert!(
        with > 0 && with < pull.len(),
        "{with} of {} lines have `completed`",
        pull.len()
    );
    let small = pull
        .iter()
        .filter(|l| {
            l["digest"]
                .as_str()
                .is_some_and(|d| d.contains("548455b72658"))
        })
        .count();
    assert!(small >= 2);
    assert!(pull
        .iter()
        .filter(|l| l["digest"]
            .as_str()
            .is_some_and(|d| d.contains("548455b72658")))
        .all(|l| l.get("completed").is_none()));

    // the big layer's last `completed` is short of its total: only `success` means done
    let big: Vec<&Value> = pull
        .iter()
        .filter(|l| {
            l["digest"]
                .as_str()
                .is_some_and(|d| d.contains("797b70c4edf8"))
        })
        .collect();
    let last_completed = big
        .iter()
        .filter_map(|l| l["completed"].as_u64())
        .max()
        .unwrap();
    assert!(last_completed < big[0]["total"].as_u64().unwrap());
    // `completed` only grows within a layer
    let seq: Vec<u64> = big.iter().filter_map(|l| l["completed"].as_u64()).collect();
    assert!(seq.windows(2).all(|w| w[0] <= w[1]));

    // a pull that cannot reach the registry fails INSIDE a 200 stream
    let bad = rec.get("pull_unreachable");
    assert_eq!(bad.status, 200);
    let lines = bad.ndjson();
    assert!(lines.last().unwrap()["error"]
        .as_str()
        .unwrap()
        .contains("registry.ollama.ai"));

    // asking again for what is there answers at once, every layer complete
    let again = rec.get("pull_again").ndjson();
    assert!(again
        .iter()
        .filter(|l| l.get("total").is_some())
        .all(|l| l["completed"] == l["total"]));

    // and the only host a real pull contacted was the registry
    assert_eq!(rec.egress, ["registry.ollama.ai:443"]);
}

#[test]
fn p3_c04_the_recordings_name_the_errors_the_application_will_show() {
    let rec = load();
    let err = |name: &str| rec.get(name).json()["error"].as_str().unwrap().to_string();
    assert_eq!(err("generate_unknown"), "model 'no-such-model' not found");
    assert_eq!(err("chat_unknown"), "model 'no-such-model' not found");
    assert_eq!(err("delete_unknown"), "model 'no-such-model' not found");
    // `show` names the model with its tag; the others do not
    assert_eq!(
        err("show_unknown"),
        "model 'no-such-model:latest' not found"
    );
    assert_eq!(err("pull_empty"), "invalid model name");
    assert_eq!(rec.get("route_unknown").text(), "404 page not found");
    // a load and an unload say which they were
    assert_eq!(rec.get("generate_load").json()["done_reason"], "load");
    assert_eq!(rec.get("generate_unload").json()["done_reason"], "unload");
    // a model kept for ever expires in the year 2319
    assert!(rec.get("ps_loaded").json()["models"][0]["expires_at"]
        .as_str()
        .unwrap()
        .starts_with("2319-"));
    assert_eq!(
        rec.get("ps_unloaded").json()["models"],
        serde_json::json!([])
    );
    // the listing carries what a model can do, with no second request
    assert!(rec.get("tags_one").json()["models"][0]["capabilities"].is_array());
    let _ = shape(&Value::Null, &[]);
}

/// Re-records with a real binary and compares with what is stored: the check that the recordings
/// still describe the Ollama in use, and the way they are renewed for a new one. Needs
/// `QUICKAI_REAL_OLLAMA` (the executable) and `QUICKAI_REAL_GGUF` (a small GGUF model), and
/// bash, curl, jq and python3. Nothing is pulled: that step of `record.sh` needs the internet.
#[cfg(unix)]
#[test]
#[ignore = "manual: needs QUICKAI_REAL_OLLAMA and QUICKAI_REAL_GGUF"]
fn p3_c02_a_real_ollama_still_answers_in_the_recorded_shapes() {
    let (Some(bin), Some(gguf)) = (
        std::env::var_os("QUICKAI_REAL_OLLAMA"),
        std::env::var_os("QUICKAI_REAL_GGUF"),
    ) else {
        panic!("set QUICKAI_REAL_OLLAMA to an ollama executable and QUICKAI_REAL_GGUF to a small GGUF file");
    };
    let out = crate::testkit::scratch("c02");
    let script = crate::testkit::fixtures_dir()
        .join("ollama")
        .join("record.sh");
    let status = std::process::Command::new("bash")
        .arg(&script)
        .arg("--bin")
        .arg(&bin)
        .arg("--gguf")
        .arg(&gguf)
        .arg("--out")
        .arg(out.as_path())
        .status()
        .expect("run record.sh");
    assert!(status.success(), "record.sh failed");

    let stored = load();
    let fresh: Value =
        serde_json::from_slice(&std::fs::read(out.join("index.json")).unwrap()).unwrap();
    for new in fresh["responses"].as_array().unwrap() {
        let name = new["name"].as_str().unwrap();
        let Some(old) = stored.responses.iter().find(|r| r.name == name) else {
            continue;
        };
        let body = std::fs::read(out.join(new["file"].as_str().unwrap())).unwrap();
        let new_rec = Rec {
            name: name.to_string(),
            method: old.method.clone(),
            path: old.path.clone(),
            request: None,
            status: new["status"].as_u64().unwrap() as u16,
            content_type: new["content_type"].as_str().unwrap_or("").to_string(),
            body,
        };
        assert_eq!(new_rec.status, old.status, "{name}: status");
        assert_eq!(
            new_rec.media_type(),
            old.media_type(),
            "{name}: content type"
        );
        match old.media_type().as_str() {
            "application/x-ndjson" => {
                assert_stream_shapes(name, &old.ndjson(), &new_rec.ndjson(), OPAQUE)
            }
            "application/json" if old.status < 400 => {
                assert_same_shape(name, &old.json(), &new_rec.json(), OPAQUE)
            }
            _ => {}
        }
    }
    let _ = std::fs::remove_dir_all(out);
}
