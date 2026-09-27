use serde::Deserialize;

/// The events a Plugin can subscribe to in its Manifest (`events`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventName {
    /// A Note was opened in the editor.
    NoteOpened,
    /// The open Note was saved.
    NoteSaved,
    /// A Note was created (by the user, or by a Plugin's `vault::write`).
    NoteCreated,
    /// A Note was deleted.
    NoteDeleted,
    /// A Note was renamed or moved; `old_path` holds the previous path.
    NoteRenamed,
}

/// Who caused an event.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Origin {
    /// The user, through Laperm's own UI.
    User,
    /// A Plugin's `vault::write` (its id). A Plugin sees its own events too.
    Plugin(String),
}

impl Origin {
    pub(crate) fn parse(text: &str) -> Origin {
        match text.strip_prefix("plugin:") {
            Some(id) => Origin::Plugin(id.to_owned()),
            None => Origin::User,
        }
    }

    /// True when this Plugin (by id) caused the event.
    pub fn is_plugin(&self, id: &str) -> bool {
        matches!(self, Origin::Plugin(p) if p == id)
    }
}

/// An event delivered to [`crate::Plugin::on_event`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Which event.
    pub name: EventName,
    /// The Note's path relative to the Vault root (the new path for `NoteRenamed`).
    pub path: String,
    /// Who caused it.
    pub origin: Origin,
    /// The previous path for `NoteRenamed`.
    pub old_path: Option<String>,
}
