//! What the background threads tell the UI: log lines, state changes and proxied traffic. They
//! travel over one channel, drained once per frame by `ServerManager::poll`. Kept apart from both
//! the server manager and the proxy so neither has to import the other.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ServerState {
    Idle,
    Starting,
    Running,
    Stopping,
    Crashed,
}

impl ServerState {
    pub fn label(&self) -> &'static str {
        match self {
            ServerState::Idle => "Stopped",
            ServerState::Starting => "Starting",
            ServerState::Running => "Running",
            ServerState::Stopping => "Stopping",
            ServerState::Crashed => "Crashed",
        }
    }
}

pub enum Event {
    Log(String),
    StateChanged(ServerState),
    /// The model the engine has loaded changed (`None`: nothing is loaded).
    Model(Option<String>),
    /// The run is using an engine that was already running (`true`), which QuickAI did not start and
    /// whose environment is not its own.
    Attached(bool),
    Traffic(TrafficEvent),
}

/// One update about traffic passing through the proxy, for the GUI's Live traffic
/// window. `id` correlates a request with its (possibly streamed) response.
#[derive(Clone, Debug)]
pub enum TrafficEvent {
    /// `session` identifies which client this came from — the client's
    /// `ip:port`, which is stable for one TCP connection (so several
    /// requests over one keep-alive connection share a session) and
    /// distinct across simultaneous clients (a browser tab vs. an external
    /// API caller, say). It's a heuristic, not a true higher-level "chat
    /// session": a client that reconnects (e.g. after an idle timeout)
    /// shows up as a new one.
    Request {
        id: u64,
        session: String,
        method: String,
        path: String,
        prompt: Option<String>,
        /// Best-effort description of any non-text parts of that same
        /// message — an attached image, PDF, etc. — so a client that sends
        /// a document alongside its text still shows up as more than a
        /// blank prompt in the Live traffic window. Empty when the message had
        /// no such parts, or wasn't in a shape this recognises.
        attachments: Vec<String>,
    },
    ResponseDelta {
        id: u64,
        text: String,
    },
    ResponseDone {
        id: u64,
        full_text: Option<String>,
    },
    ResponseError {
        id: u64,
        message: String,
    },
}
