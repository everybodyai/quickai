//! Group J of docs/phase3-test-plan.md, the half that needs the internet: the list the application ships,
//! held against the sites it was made from (docs/catalog.md, section 5). Both tests are run by hand, before a
//! release and when the list seems wrong: the sites change without telling anyone, and an Ollama tag is
//! the one thing in the list that is allowed to move.

use std::time::Duration;

use quickai_core::registry::catalog::Run;
use serde_json::Value;

use crate::config::{build_registry, ConfigDirs};

/// One GET, with a pause before it (the sites are asked politely) and a wait when they say to.
fn get_json(agent: &ureq::Agent, url: &str, accept: Option<&str>) -> Result<(u16, Value), String> {
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(250));
        let mut request = agent.get(url).header("User-Agent", "quickai-catalog-check");
        if let Some(accept) = accept {
            request = request.header("Accept", accept);
        }
        let mut reply = request.call().map_err(|e| e.to_string())?;
        let status = reply.status().as_u16();
        if status == 429 {
            std::thread::sleep(Duration::from_secs(30));
            continue;
        }
        let body = reply
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        return Ok((status, serde_json::from_str(&body).unwrap_or(Value::Null)));
    }
    Err("the site kept refusing (HTTP 429)".to_string())
}

/// What the list says is held against what the sites say now: every repository exists at its commit and
/// needs no login, every file is there with the size and the SHA-256 the list holds, and every Ollama
/// name still pulls what the list says it pulls (the sum of the layers of its manifest).
#[test]
#[ignore = "manual: needs the internet (cargo test -p quickai p3_j22 -- --ignored)"]
fn p3_j22_the_real_sites_still_have_what_the_list_says() {
    let registry = build_registry(&ConfigDirs::default());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(40)))
        .build()
        .into();
    let mut problems: Vec<String> = Vec::new();
    let mut checked = 0;
    for model in registry.catalog().models() {
        for run in model.runs.values() {
            checked += 1;
            match run {
                Run::Files(run) => {
                    let url = format!(
                        "https://huggingface.co/api/models/{}/revision/{}?blobs=true",
                        run.repo, run.rev
                    );
                    let answer = match get_json(&agent, &url, None) {
                        Ok((200, v)) => v,
                        Ok((code, _)) => {
                            problems.push(format!(
                                "{}: {} at {} answers HTTP {code}",
                                model.id,
                                run.repo,
                                &run.rev[..7]
                            ));
                            continue;
                        }
                        Err(e) => {
                            problems.push(format!("{}: {e}", model.id));
                            continue;
                        }
                    };
                    if answer["gated"] != Value::Bool(false) {
                        problems.push(format!(
                            "{}: {} needs a login now ({})",
                            model.id, run.repo, answer["gated"]
                        ));
                    }
                    let siblings = answer["siblings"].as_array().cloned().unwrap_or_default();
                    for file in run
                        .files
                        .iter()
                        .chain(run.companions.iter().map(|(_, f)| f))
                    {
                        let Some(found) = siblings
                            .iter()
                            .find(|s| s["rfilename"] == file.path.as_str())
                        else {
                            problems.push(format!(
                                "{}: {} is not in {} at {}",
                                model.id,
                                file.path,
                                run.repo,
                                &run.rev[..7]
                            ));
                            continue;
                        };
                        if found["size"].as_u64() != file.size {
                            problems.push(format!(
                                "{}: {} is {} bytes, the list says {:?}",
                                model.id, file.path, found["size"], file.size
                            ));
                        }
                        if found["lfs"]["sha256"].as_str() != file.sha256.as_deref() {
                            problems.push(format!(
                                "{}: {} hashes to {}, the list says {:?}",
                                model.id, file.path, found["lfs"]["sha256"], file.sha256
                            ));
                        }
                    }
                }
                Run::Reference(run) => {
                    let (name, tag) = run
                        .reference
                        .split_once(':')
                        .unwrap_or((&run.reference, "latest"));
                    let url =
                        format!("https://registry.ollama.ai/v2/library/{name}/manifests/{tag}");
                    match get_json(
                        &agent,
                        &url,
                        Some("application/vnd.docker.distribution.manifest.v2+json"),
                    ) {
                        Ok((200, v)) => {
                            let layers: u64 = v["layers"]
                                .as_array()
                                .map(|l| l.iter().filter_map(|x| x["size"].as_u64()).sum())
                                .unwrap_or(0);
                            let total = layers + v["config"]["size"].as_u64().unwrap_or(0);
                            if Some(total) != run.size {
                                problems.push(format!(
                                    "{}: {} pulls {total} bytes now, the list says {:?}",
                                    model.id, run.reference, run.size
                                ));
                            }
                        }
                        Ok((code, _)) => problems.push(format!(
                            "{}: {} answers HTTP {code}",
                            model.id, run.reference
                        )),
                        Err(e) => problems.push(format!("{}: {e}", model.id)),
                    }
                }
            }
        }
    }
    assert!(checked >= 100, "only {checked} runs were looked at");
    assert!(
        problems.is_empty(),
        "{} of {checked} runs have moved:\n{}",
        problems.len(),
        problems.join("\n")
    );
}
