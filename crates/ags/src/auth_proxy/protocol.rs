use serde::{Deserialize, Serialize};

/// Message sent from the container shim to the host proxy.
///
/// Messages are newline-delimited JSON over a Unix domain socket.
/// Each connection represents one session (one URL open request).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShimMessage {
    /// Request to open a URL in the host browser.
    ///
    /// If `callback_port` is set, the URL contains a localhost callback
    /// (e.g. OAuth redirect_uri). The host proxy will capture the callback
    /// HTTP request and relay it back through the socket for the shim to
    /// replay against the container-local listener.
    OpenUrl {
        session_id: String,
        url: String,
        callback_port: Option<u16>,
    },

    /// Response to a relayed callback request.
    ///
    /// Sent after the shim replays the HTTP request to the container-local
    /// server and captures the response.
    CallbackResponse {
        session_id: String,
        request_id: String,
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    },
}

/// Message sent from the host proxy to the container shim.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMessage {
    /// Result of the user prompt (allow/deny).
    PromptResult { session_id: String, allowed: bool },

    /// A callback HTTP request captured by the host proxy's loopback listener.
    ///
    /// The shim must replay this request to the container-local server and
    /// respond with a `CallbackResponse`.
    CallbackRequest {
        session_id: String,
        request_id: String,
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: String,
    },

    /// The session completed successfully.
    SessionComplete { session_id: String },

    /// An error occurred during the session.
    Error { session_id: String, message: String },
}
