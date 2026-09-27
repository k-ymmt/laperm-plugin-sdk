//! The host API (`docs/plugin-api.md` §3), one module per area. Every function sends one
//! `laperm.call` and needs the Permission noted in its documentation.

use crate::abi::call;
use crate::Result;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Serialize)]
struct OpOnly {
    op: &'static str,
}

/// Logging (no Permission). Goes to macOS / iOS unified logging, subsystem `app.kymmt.Laperm`,
/// category `plugin.<id>`.
pub mod log {
    use super::*;

    #[derive(Serialize)]
    struct Log<'a> {
        op: &'static str,
        level: &'static str,
        message: &'a str,
    }

    fn send(level: &'static str, message: &str) -> Result<()> {
        call::<_, ()>(&Log { op: "log", level, message })
    }

    /// `debug` level.
    pub fn debug(message: &str) -> Result<()> {
        send("debug", message)
    }
    /// `info` level.
    pub fn info(message: &str) -> Result<()> {
        send("info", message)
    }
    /// `notice` level.
    pub fn notice(message: &str) -> Result<()> {
        send("notice", message)
    }
    /// `error` level.
    pub fn error(message: &str) -> Result<()> {
        send("error", message)
    }
}

/// The device's local time (no Permission). There is no WASI clock in a Plugin.
pub mod clock {
    use super::*;

    /// A local date and time.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LocalTime {
        /// Milliseconds since the Unix epoch.
        pub timestamp: i64,
        /// The local time zone's offset from UTC, in seconds.
        pub offset_seconds: i64,
        /// Local calendar fields.
        pub year: i32,
        /// 1–12.
        pub month: u32,
        /// 1–31.
        pub day: u32,
        /// 0–23.
        pub hour: u32,
        /// 0–59.
        pub minute: u32,
        /// 0–60.
        pub second: u32,
        /// 1 = Sunday … 7 = Saturday.
        pub weekday: u32,
    }

    impl LocalTime {
        /// `YYYY-MM-DD`.
        pub fn date_iso(&self) -> String {
            format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
        }

        /// `HH:MM`.
        pub fn time_hm(&self) -> String {
            format!("{:02}:{:02}", self.hour, self.minute)
        }

        /// Formats with a small subset of Unicode date patterns: `yyyy`, `MM`, `dd`, `HH`, `mm`,
        /// `ss`; everything else is copied as is.
        pub fn format(&self, pattern: &str) -> String {
            let mut out = String::with_capacity(pattern.len() + 8);
            let mut rest = pattern;
            while !rest.is_empty() {
                let (token, value): (&str, Option<String>) = if rest.starts_with("yyyy") {
                    ("yyyy", Some(format!("{:04}", self.year)))
                } else if rest.starts_with("MM") {
                    ("MM", Some(format!("{:02}", self.month)))
                } else if rest.starts_with("dd") {
                    ("dd", Some(format!("{:02}", self.day)))
                } else if rest.starts_with("HH") {
                    ("HH", Some(format!("{:02}", self.hour)))
                } else if rest.starts_with("mm") {
                    ("mm", Some(format!("{:02}", self.minute)))
                } else if rest.starts_with("ss") {
                    ("ss", Some(format!("{:02}", self.second)))
                } else {
                    let ch = rest.chars().next().unwrap();
                    (&rest[..ch.len_utf8()], None)
                };
                match value {
                    Some(v) => out.push_str(&v),
                    None => out.push_str(token),
                }
                rest = &rest[token.len()..];
            }
            out
        }
    }

    /// The current local time.
    pub fn now() -> Result<LocalTime> {
        call(&OpOnly { op: "clock.now" })
    }
}

/// Plugin Settings (no Permission): the values the user set in Laperm's form, with the
/// Manifest defaults filled in.
pub mod settings {
    use super::*;

    #[derive(Deserialize)]
    struct Values<T> {
        values: T,
    }

    /// Decodes the settings map into `T` (a struct whose field names are the setting keys, or a
    /// `HashMap<String, rmp_serde::…>`).
    pub fn get<T: DeserializeOwned>() -> Result<T> {
        let values: Values<T> = call(&OpOnly { op: "settings.get" })?;
        Ok(values.values)
    }
}

/// The open Note (Permission `editor`).
pub mod editor {
    use super::*;

    /// A selection as UTF-8 byte offsets into [`EditorState::text`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    pub struct Selection {
        /// Start offset (bytes).
        pub start: usize,
        /// End offset (bytes), `>= start`.
        pub end: usize,
    }

    /// What `editor::get` returns.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    pub struct EditorState {
        /// The Note's path relative to the Vault root.
        pub path: String,
        /// The whole text.
        pub text: String,
        /// The version to pass to [`replace`]; changes with every keystroke.
        pub version: i64,
        /// The selection (empty when it is a caret).
        pub selection: Selection,
    }

    impl EditorState {
        /// The selected text.
        pub fn selected_text(&self) -> &str {
            &self.text[self.selection.start..self.selection.end]
        }

        /// Replaces the selection (or inserts at the caret). Returns the new version.
        pub fn replace_selection(&self, text: &str) -> Result<i64> {
            replace(self.version, self.selection.start, self.selection.end, text)
        }
    }

    /// The open Note, or `None` when nothing is open.
    pub fn get() -> Result<Option<EditorState>> {
        call(&OpOnly { op: "editor.get" })
    }

    #[derive(Serialize)]
    struct Replace<'a> {
        op: &'static str,
        version: i64,
        start: usize,
        end: usize,
        text: &'a str,
    }

    #[derive(Deserialize)]
    struct Version {
        version: i64,
    }

    /// Replaces `start..end` (UTF-8 byte offsets on character boundaries) with `text` when
    /// `version` is still current. Returns the new version. A `conflict` error means the user typed
    /// since [`get`]; read again and retry.
    pub fn replace(version: i64, start: usize, end: usize, text: &str) -> Result<i64> {
        let v: Version = call(&Replace { op: "editor.replace", version, start, end, text })?;
        Ok(v.version)
    }

    #[derive(Serialize)]
    struct Open<'a> {
        op: &'static str,
        path: &'a str,
    }

    /// Opens the Note at `path` in the editor.
    pub fn open(path: &str) -> Result<()> {
        call(&Open { op: "editor.open", path })
    }
}

/// Files in the Vault (Permissions `vault.read` / `vault.write`).
pub mod vault {
    use super::*;
    use serde_bytes::ByteBuf;

    #[derive(Serialize)]
    struct Path<'a> {
        op: &'static str,
        path: &'a str,
    }

    #[derive(Deserialize)]
    struct DataValue {
        data: ByteBuf,
    }

    /// Reads any non-hidden file (`vault.read`).
    pub fn read(path: &str) -> Result<Vec<u8>> {
        let v: DataValue = call(&Path { op: "vault.read", path })?;
        Ok(v.data.into_vec())
    }

    /// Reads a file as UTF-8 text (`vault.read`).
    pub fn read_text(path: &str) -> Result<String> {
        String::from_utf8(read(path)?).map_err(|e| crate::Error::Encoding(format!("{path} is not UTF-8: {e}")))
    }

    #[derive(Serialize)]
    struct Write<'a> {
        op: &'static str,
        path: &'a str,
        #[serde(with = "serde_bytes")]
        data: &'a [u8],
    }

    /// Creates or replaces a file, creating missing folders (`vault.write`). The open Note is
    /// refused with `conflict`: use [`crate::editor::replace`] for it.
    pub fn write(path: &str, data: &[u8]) -> Result<()> {
        call(&Write { op: "vault.write", path, data })
    }

    /// [`write`] for text.
    pub fn write_text(path: &str, text: &str) -> Result<()> {
        write(path, text.as_bytes())
    }

    /// What is at a path.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ItemKind {
        /// Something exists there.
        pub exists: bool,
        /// It is a folder.
        pub is_folder: bool,
    }

    /// Whether a file or folder exists (`vault.read`). Hidden paths count as missing.
    pub fn exists(path: &str) -> Result<ItemKind> {
        call(&Path { op: "vault.exists", path })
    }

    /// Notes and folders under a folder.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    pub struct Listing {
        /// Note paths, sorted.
        pub notes: Vec<String>,
        /// Folder paths, sorted.
        pub folders: Vec<String>,
    }

    #[derive(Serialize)]
    struct List<'a> {
        op: &'static str,
        folder: &'a str,
        recursive: bool,
    }

    /// Lists the Notes and folders in `folder` (`""` for the Vault root), optionally recursively
    /// (`vault.read`). Attachments are not listed.
    pub fn list(folder: &str, recursive: bool) -> Result<Listing> {
        call(&List { op: "vault.list", folder, recursive })
    }
}

/// Plugin Data (Permission `storage`): one MessagePack document per Plugin, in the Vault,
/// synchronized with it. At most 4 MB.
pub mod storage {
    use super::*;

    #[derive(Deserialize)]
    struct Value<T> {
        value: Option<T>,
    }

    /// The stored document, or `None` when nothing was stored yet.
    pub fn get<T: DeserializeOwned>() -> Result<Option<T>> {
        let v: Value<T> = call(&OpOnly { op: "storage.get" })?;
        Ok(v.value)
    }

    #[derive(Serialize)]
    struct Set<'a, T: Serialize> {
        op: &'static str,
        value: &'a T,
    }

    /// Replaces the stored document.
    pub fn set<T: Serialize>(value: &T) -> Result<()> {
        call(&Set { op: "storage.set", value })
    }
}

#[derive(Serialize)]
struct Notice<'a> {
    op: &'static str,
    message: &'a str,
}

/// Shows a transient message to the user (Permission `notice`).
pub fn notice(message: &str) -> Result<()> {
    call(&Notice { op: "notice", message })
}
