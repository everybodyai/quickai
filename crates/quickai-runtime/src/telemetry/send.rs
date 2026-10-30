//! The requests telemetry makes to `api.everybodyai.dev` (section 9 of docs/reports-and-community.md), and what an
//! answer means for what is queued. A build without the feature `telemetry` has none of them.

use std::time::Duration;

/// The address reports go to; `QUICKAI_TELEMETRY_URL` replaces it (for a test, or a server of one's own).
pub const DEFAULT_ENDPOINT: &str = "https://api.everybodyai.dev";

/// What an answer means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Taken (2xx).
    Sent,
    /// The schema refused it (400, 413, 422): it will never pass, so it is dropped.
    Refused,
    /// This install or network is blocked (403): nothing is sent for 30 days.
    Blocked,
    /// This version is no longer collected (410): nothing is sent until QuickAI is updated.
    Retired,
    /// Not now: too many (429, with the seconds to wait when the server says), the server failed (5xx), or no
    /// answer at all.
    Later(Option<u64>),
}

/// What an HTTP status means for what was sent.
pub fn answer(status: u16, retry_after: Option<u64>) -> Answer {
    match status {
        200..=299 => Answer::Sent,
        400 | 413 | 422 => Answer::Refused,
        403 => Answer::Blocked,
        410 => Answer::Retired,
        429 => Answer::Later(retry_after),
        _ => Answer::Later(None),
    }
}

#[cfg(feature = "telemetry")]
fn user_agent() -> String {
    format!("quickai/{}", env!("CARGO_PKG_VERSION"))
}

#[cfg(feature = "telemetry")]
fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// POSTs `body` (JSON) to `endpoint` + `path`, for the install `install`.
#[cfg(feature = "telemetry")]
pub fn post(endpoint: &str, path: &str, install: &str, body: &str, timeout: Duration) -> Answer {
    let url = format!("{}{path}", endpoint.trim_end_matches('/'));
    match agent(timeout)
        .post(&url)
        .header("content-type", "application/json")
        .header("user-agent", user_agent())
        .header("QuickAI-Install", install)
        .send(body)
    {
        Ok(response) => {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            answer(response.status().as_u16(), retry_after)
        }
        Err(_) => Answer::Later(None),
    }
}

#[cfg(not(feature = "telemetry"))]
pub fn post(_: &str, _: &str, _: &str, _: &str, _: Duration) -> Answer {
    Answer::Refused
}

/// `GET /v1/me` (everything held for `install`, as JSON text) or, with `delete`, `DELETE /v1/me` (the receipt).
#[cfg(feature = "telemetry")]
pub fn me(
    endpoint: &str,
    install: &str,
    delete: bool,
    timeout: Duration,
) -> Result<String, String> {
    let url = format!("{}/v1/me", endpoint.trim_end_matches('/'));
    let agent = agent(timeout);
    let request = if delete {
        agent.delete(&url)
    } else {
        agent.get(&url)
    };
    let response = request
        .header("user-agent", user_agent())
        .header("QuickAI-Install", install)
        .call()
        .map_err(|e| format!("EverybodyAI's server could not be reached ({e})."))?;
    let status = response.status().as_u16();
    let text = response.into_body().read_to_string().unwrap_or_default();
    match status {
        200..=299 => Ok(text),
        404 => Err("EverybodyAI holds nothing for this install id.".to_string()),
        _ => Err(format!("EverybodyAI's server answered {status}.")),
    }
}

#[cfg(not(feature = "telemetry"))]
pub fn me(_: &str, _: &str, _: bool, _: Duration) -> Result<String, String> {
    Err("This build of QuickAI sends no telemetry.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_status_means_one_thing() {
        assert_eq!(answer(202, None), Answer::Sent);
        assert_eq!(answer(400, None), Answer::Refused);
        assert_eq!(answer(413, None), Answer::Refused);
        assert_eq!(answer(403, None), Answer::Blocked);
        assert_eq!(answer(410, None), Answer::Retired);
        assert_eq!(answer(429, Some(120)), Answer::Later(Some(120)));
        assert_eq!(answer(503, None), Answer::Later(None));
    }
}
