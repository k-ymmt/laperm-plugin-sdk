use std::fmt;

/// Result type used throughout the SDK.
pub type Result<T> = std::result::Result<T, Error>;

/// An error from a host call, or a Plugin's own error message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Laperm answered `ok: false`.
    Host(HostError),
    /// The Plugin's own failure (shown to the user for a Command, logged for an event).
    Message(String),
    /// The host's response could not be decoded (a bug or a version mismatch).
    Encoding(String),
}

/// The error Laperm returned for a host call: a `code` from `docs/plugin-api.md` and a message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct HostError {
    /// `permission-denied`, `invalid-request`, `not-found`, `not-a-note`, `conflict`, `too-large`,
    /// `io`, `unsupported`, `no-open-note`.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

impl HostError {
    /// The Note changed since `editor::get` (or the path is the open Note).
    pub fn is_conflict(&self) -> bool {
        self.code == "conflict"
    }

    /// Nothing at that path.
    pub fn is_not_found(&self) -> bool {
        self.code == "not-found"
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Host(e) => write!(f, "{} ({})", e.message, e.code),
            Error::Message(m) => f.write_str(m),
            Error::Encoding(m) => write!(f, "encoding error: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Error::Message(message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Error::Message(message.to_owned())
    }
}

impl From<HostError> for Error {
    fn from(e: HostError) -> Self {
        Error::Host(e)
    }
}

impl From<rmp_serde::decode::Error> for Error {
    fn from(e: rmp_serde::decode::Error) -> Self {
        Error::Encoding(e.to_string())
    }
}

impl From<rmp_serde::encode::Error> for Error {
    fn from(e: rmp_serde::encode::Error) -> Self {
        Error::Encoding(e.to_string())
    }
}
