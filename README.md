# laperm-plugin

The Rust SDK for writing [Laperm](https://github.com/k-ymmt/Laperm) Plugins.

A Laperm Plugin is a WebAssembly core module (`wasm32-unknown-unknown`, no WASI) that lives in a Vault
under `.laperm/plugins/<id>/` and talks to Laperm through one host import, `laperm.call`, carrying
MessagePack messages. This crate wraps that wire format ([`docs/plugin-api.md`](docs/plugin-api.md)) so a
Plugin only implements one trait:

```rust
use laperm_plugin::{editor, notice, Plugin, Result};

struct Hello;

impl Plugin for Hello {
    fn new() -> Result<Self> { Ok(Hello) }

    fn run_command(&mut self, command: &str) -> Result<()> {
        match command {
            "hello" => notice("Hello from a Plugin"),
            other => Err(format!("unknown command {other}").into()),
        }
    }
}

laperm_plugin::export_plugin!(Hello);
```

`export_plugin!` defines the exports Laperm needs (`cabi_realloc`, `laperm_init`, `laperm_handle`) and
installs a panic hook that sends the panic message to Laperm's log before the module traps.

## Host API

| Module                    | Permission               | What                                                        |
|---------------------------|--------------------------|-------------------------------------------------------------|
| `laperm_plugin::log`      | none                     | `debug` / `info` / `notice` / `error` → unified logging     |
| `laperm_plugin::clock`    | none                     | `now()` → local date and time (`format("yyyy-MM-dd")`)      |
| `laperm_plugin::settings` | none                     | `get::<T>()` → the Plugin Settings the user chose            |
| `laperm_plugin::editor`   | `editor`                 | `get()`, `replace(version, start, end, text)`, `open(path)` |
| `laperm_plugin::vault`    | `vault.read` / `vault.write` | `read`, `read_text`, `write`, `write_text`, `exists`, `list` |
| `laperm_plugin::storage`  | `storage`                | `get::<T>()` / `set(&value)` — one document, 4 MB, in the Vault |
| `laperm_plugin::notice`   | `notice`                 | a transient banner                                          |

Events (`Plugin::on_event`) arrive when the Manifest subscribes to them and declares the `events`
Permission: `noteOpened`, `noteSaved`, `noteCreated`, `noteDeleted`, `noteRenamed`, each with the path and
an `Origin` (`User` or `Plugin(id)`).

`laperm_plugin::manifest::Manifest` mirrors `manifest.json` so a test can validate the file a Plugin ships
(`Manifest::validate`).

## Getting started

Start from [laperm-plugin-template](https://github.com/k-ymmt/laperm-plugin-template): it has the
`Cargo.toml` profile, a `manifest.json`, a test that keeps the Manifest valid, and a GitHub Actions
workflow that builds `plugin.wasm` and publishes a Release when you push a `v<version>` tag. Laperm
installs from `https://github.com/<owner>/<repo>/releases/latest/download/{manifest.json,plugin.wasm}`.
[laperm-daily-note](https://github.com/k-ymmt/laperm-daily-note) is a complete sample.

## Testing without Laperm

Off wasm, `laperm.call` is routed to a handler you install with
`laperm_plugin::abi::testing::set_handler`, so a Plugin's logic can be unit-tested natively against a fake
host (see `tests/host.rs`).

## License

MIT.
