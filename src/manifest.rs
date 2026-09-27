//! The Manifest (`manifest.json`, `docs/plugin-api.md` §4) as Rust types, so a Plugin's tests can
//! check the file it ships (`serde_json::from_str::<Manifest>(include_str!("../manifest.json"))`).

use serde::{Deserialize, Serialize};

/// A Plugin's `manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Lowercase letters, digits and hyphens; equals the folder name in the Vault.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Semantic version, equal to the release tag without a `v`.
    pub version: String,
    /// The Plugin API version (`1`).
    pub api_version: u32,
    /// Optional minimum Laperm version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_app_version: Option<String>,
    /// One line about the Plugin.
    #[serde(default)]
    pub description: String,
    /// Author name.
    #[serde(default)]
    pub author: String,
    /// Author URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_url: Option<String>,
    /// iOS refuses to enable the Plugin.
    #[serde(default)]
    pub is_desktop_only: bool,
    /// Permissions: `editor`, `vault.read`, `vault.write`, `storage`, `events`, `notice`.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Commands.
    #[serde(default)]
    pub commands: Vec<Command>,
    /// Subscribed events.
    #[serde(default)]
    pub events: Vec<String>,
    /// Plugin Settings.
    #[serde(default)]
    pub settings: Vec<Setting>,
}

/// A Command declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Command {
    /// Same character rules as the Plugin id; passed to `run_command`.
    pub id: String,
    /// Menu title.
    pub name: String,
    /// Default shortcut, e.g. `cmd+shift+t`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
    /// SF Symbol name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_image: Option<String>,
    /// Enabled only while a Note is open (default true).
    #[serde(default = "default_true")]
    pub needs_open_note: bool,
}

fn default_true() -> bool {
    true
}

/// A Plugin Settings declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Setting {
    /// The key used in `settings::get`.
    pub key: String,
    /// Form label.
    #[serde(default)]
    pub label: String,
    /// `bool`, `string`, `number` or `enum`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The default value (a JSON bool, string or number).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<SettingValue>,
    /// Help text under the control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Choices for `enum`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SettingOption>,
}

/// A setting value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettingValue {
    /// A boolean.
    Bool(bool),
    /// A number.
    Number(f64),
    /// A string (also the value of an `enum` setting).
    String(String),
}

/// One choice of an `enum` setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingOption {
    /// The stored value.
    pub value: String,
    /// The label shown; defaults to the value.
    #[serde(default)]
    pub label: String,
}

impl Manifest {
    /// Checks the rules Laperm enforces at load: id syntax, `apiVersion`, command ids and names,
    /// setting keys and types. Returns the first problem.
    pub fn validate(&self) -> Result<(), String> {
        fn valid_id(id: &str) -> bool {
            !id.is_empty()
                && id.len() <= 64
                && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        }
        if !valid_id(&self.id) {
            return Err(format!("id {:?} must be 1-64 lowercase letters, digits or hyphens", self.id));
        }
        if self.id == "laperm" {
            return Err("id \"laperm\" is reserved".into());
        }
        if self.api_version != crate::API_VERSION {
            return Err(format!("apiVersion {} is not {}", self.api_version, crate::API_VERSION));
        }
        if self.name.trim().is_empty() {
            return Err("name is empty".into());
        }
        if self.version.trim().is_empty() {
            return Err("version is empty".into());
        }
        for command in &self.commands {
            if !valid_id(&command.id) {
                return Err(format!("command id {:?} is invalid", command.id));
            }
            if command.name.trim().is_empty() {
                return Err(format!("command {:?} has no name", command.id));
            }
        }
        for permission in &self.permissions {
            if !["editor", "vault.read", "vault.write", "storage", "events", "notice", "network"].contains(&permission.as_str()) {
                return Err(format!("unknown permission {permission:?}"));
            }
        }
        if !self.events.is_empty() && !self.permissions.iter().any(|p| p == "events") {
            return Err("events are subscribed without the events permission".into());
        }
        for event in &self.events {
            if !["noteOpened", "noteSaved", "noteCreated", "noteDeleted", "noteRenamed"].contains(&event.as_str()) {
                return Err(format!("unknown event {event:?}"));
            }
        }
        for setting in &self.settings {
            if setting.key.is_empty() || !setting.key.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                return Err(format!("setting key {:?} is invalid", setting.key));
            }
            match setting.kind.as_str() {
                "bool" | "string" | "number" => {}
                "enum" => {
                    if setting.options.is_empty() {
                        return Err(format!("enum setting {:?} has no options", setting.key));
                    }
                }
                other => return Err(format!("setting {:?} has unknown type {other:?}", setting.key)),
            }
        }
        Ok(())
    }
}
