//! SDK for Laperm Plugins.
//!
//! A Laperm Plugin is a WebAssembly core module built for `wasm32-unknown-unknown` that talks to
//! Laperm through one host import, `laperm.call`, carrying MessagePack messages. This crate wraps
//! the wire format (see `docs/plugin-api.md`) so a Plugin only implements the [`Plugin`] trait:
//!
//! ```ignore
//! use laperm_plugin::{editor, notice, Event, Plugin};
//!
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn new() -> Self { Hello }
//!
//!     fn run_command(&mut self, command: &str) -> laperm_plugin::Result<()> {
//!         match command {
//!             "hello" => { notice("Hello from a Plugin")?; Ok(()) }
//!             other => Err(format!("unknown command {other}").into()),
//!         }
//!     }
//!
//!     fn on_event(&mut self, event: &Event) -> laperm_plugin::Result<()> { Ok(()) }
//! }
//!
//! laperm_plugin::export_plugin!(Hello);
//! ```
//!
//! `export_plugin!` defines the exports Laperm needs (`cabi_realloc`, `laperm_init`,
//! `laperm_handle`) and installs a panic hook that forwards the panic message to Laperm's log
//! before the module traps.

#![deny(missing_docs)]

pub mod abi;
mod error;
mod event;
pub mod host;
pub mod manifest;

pub use error::{Error, HostError, Result};
pub use event::{Event, EventName, Origin};
pub use host::clock::{self, LocalTime};
pub use host::editor::{self, EditorState, Selection};
pub use host::log;
pub use host::notice;
pub use host::settings;
pub use host::storage;
pub use host::vault::{self, ItemKind, Listing};

// Re-exported so `export_plugin!` and Plugin authors use the same versions.
#[doc(hidden)]
pub use rmp_serde;
#[doc(hidden)]
pub use serde;
#[doc(hidden)]
pub use serde_bytes;

/// The Plugin API version this SDK speaks (`apiVersion` in the Manifest).
pub const API_VERSION: u32 = 1;

/// What a Plugin implements. One instance lives for the whole Vault Session.
pub trait Plugin: Sized {
    /// Called once from `laperm_init`. Returning an error stops the Plugin; the message is shown.
    fn new() -> Result<Self>;

    /// A Command from the Manifest (`commands[].id`) was invoked by the user.
    /// An error is shown to the user.
    fn run_command(&mut self, command: &str) -> Result<()>;

    /// An event the Manifest subscribed to. An error is logged.
    fn on_event(&mut self, _event: &Event) -> Result<()> {
        Ok(())
    }
}
