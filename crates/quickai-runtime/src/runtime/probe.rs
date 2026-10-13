//! The `probe.http@1` primitive: ask an engine once whether it answers a request with the status
//! that means ready. The supervisor asks again every `interval` until it does, or until the
//! plan's timeout says to stop asking.

use std::time::Duration;

/// How long one question may take. A server that is loading a model answers at once with a "not
/// yet" status; one that accepts a connection and says nothing must not hold up a Stop.
const ASK_TIMEOUT: Duration = Duration::from_secs(1);

pub struct HttpProbe {
    agent: ureq::Agent,
    url: String,
    expect_status: u16,
}

impl HttpProbe {
    pub fn new(host: &str, port: u16, path: &str, expect_status: u16) -> Self {
        HttpProbe {
            // a status is an answer here, not an error: the plan says which one means ready
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(ASK_TIMEOUT))
                .build()
                .into(),
            url: format!("http://{host}:{port}{path}"),
            expect_status,
        }
    }

    /// True when the engine answered with the status that means ready.
    pub fn ready(&self) -> bool {
        self.agent
            .get(&self.url)
            .call()
            .is_ok_and(|reply| reply.status().as_u16() == self.expect_status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(status: u16) -> (std::sync::Arc<tiny_http::Server>, u16) {
        let server = std::sync::Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let port = server.server_addr().to_ip().unwrap().port();
        let worker = server.clone();
        std::thread::spawn(move || {
            while let Ok(request) = worker.recv() {
                let _ = request.respond(tiny_http::Response::empty(status));
            }
        });
        (server, port)
    }

    #[test]
    fn it_is_ready_only_on_the_status_that_means_ready() {
        let (up, port) = server(200);
        assert!(HttpProbe::new("127.0.0.1", port, "/health", 200).ready());
        assert!(
            !HttpProbe::new("127.0.0.1", port, "/health", 204).ready(),
            "another status than the expected one is not ready"
        );
        up.unblock();

        // a server that says "not yet" (llama-server while it loads) is not ready, and not an error
        let (loading, port) = server(503);
        assert!(!HttpProbe::new("127.0.0.1", port, "/health", 200).ready());
        assert!(HttpProbe::new("127.0.0.1", port, "/health", 503).ready());
        loading.unblock();
    }

    #[test]
    fn nothing_listening_is_not_ready() {
        let port = crate::runtime::process::ephemeral_port().unwrap();
        assert!(!HttpProbe::new("127.0.0.1", port, "/health", 200).ready());
    }
}
