# Laperm Plugin API (v1, `apiVersion` 1)

This is the contract between Laperm and a Plugin binary. The design decisions behind it are in
ADR 0022–0026 (`docs/adr/`, Japanese); the vocabulary (Plugin, Manifest, Permission, Plugin Data,
Plugin Settings) is in `CONTEXT.md`. The Rust SDK (`laperm-plugin`) wraps everything below, so a
Plugin author normally never touches the wire format; this document is the source of truth for the
SDK and for Laperm's `PluginKit`.

## 1. Module

- A Plugin is a WebAssembly **core module** compiled for `wasm32-unknown-unknown`. **No WASI.**
  There is no clock, no randomness, no stdout: every way out of the module is the one host import.
  A `wasm32-wasip1` binary fails to load; the error names the right target.
- Location in a Vault: `.laperm/plugins/<id>/` with `manifest.json`, `plugin.wasm`, and, once the
  Plugin has written them, `data.msgpack` (Plugin Data) and `settings.json` (Plugin Settings).
  `.laperm/plugins.json` records which Plugins are installed and enabled.
- Imports (all of them):

  | Module   | Name   | Type                          |
  |----------|--------|-------------------------------|
  | `laperm` | `call` | `(i32 ptr, i32 len) -> i64`   |

- Exports (all required):

  | Name            | Type                                                       |
  |-----------------|------------------------------------------------------------|
  | `memory`        | linear memory                                              |
  | `cabi_realloc`  | `(i32 old_ptr, i32 old_size, i32 align, i32 new_size) -> i32` |
  | `laperm_init`   | `() -> i32`                                                |
  | `laperm_handle` | `(i32 ptr, i32 len) -> i64`                                |

  `cabi_realloc` has the name and signature of the Component Model canonical ABI's allocator so
  that a future move to components changes nothing on the author side. `new_size == 0` frees.

### Buffers

Every message is one MessagePack buffer in the Plugin's linear memory. A `(ptr, len)` pair is
packed into one `i64` as `(ptr << 32) | len`.

- **Host → Plugin** (`laperm_handle`): Laperm allocates the request with
  `cabi_realloc(0, 0, 1, len)`, writes it, calls `laperm_handle(ptr, len)`, and afterwards frees the
  request with `cabi_realloc(ptr, len, 1, 0)`. The Plugin returns its response the same way: a
  buffer it allocated with `cabi_realloc`, which Laperm reads and then frees with
  `cabi_realloc(ptr, len, 1, 0)`. A packed `0` (no buffer) means `{ "ok": true }`.
- **Plugin → Host** (`laperm.call`): the Plugin passes a request buffer it owns. Laperm allocates the
  response in the Plugin's memory with `cabi_realloc(0, 0, 1, len)` **while `laperm.call` is still
  running** (the export is called re-entrantly) and returns the packed pointer. The Plugin owns that
  buffer and frees it when done. Laperm never frees the request buffer.
- `laperm_init()` is called once after instantiation and must return `0`. Any other value stops the
  Plugin; the value is shown as the reason.

### Encoding

MessagePack. Requests and responses are **maps with string keys**. Enumerations are strings.
Structures are never encoded as arrays (Rust: `rmp_serde::to_vec_named`). Binary data uses the
`bin` family. A message longer than 32 MB is refused.

## 2. Host → Plugin messages

```
{ "type": "command", "command": "<name>" }
{ "type": "event", "event": "<eventName>", "path": "<relative path>", "origin": "user" | "plugin:<id>",
  "oldPath": "<relative path>" }     // oldPath only for noteRenamed
```

The Plugin answers `{ "ok": true }` or `{ "ok": false, "error": "<message>" }`. An error from a
command is shown to the user; an error from an event is logged.

`command` is the name from the Manifest (`commands[].id`), without the `plugin.<pluginID>.` prefix
Laperm adds to the Command ID. Events are the names the Manifest subscribed to (`events`):
`noteOpened`, `noteSaved`, `noteCreated`, `noteDeleted`, `noteRenamed`. Only Laperm's own
operations produce events (the user's, and every Plugin's `vault.write`); changes the watcher
detects on disk (iCloud, other apps) do not. Events reach every Plugin including the one that
caused them; `origin` lets a Plugin skip its own.

## 3. Plugin → Host requests (`laperm.call`)

Request: `{ "op": "<op>", ...fields }`.
Response: `{ "ok": true, "value": <op specific, or nil> }` or
`{ "ok": false, "error": { "code": "<code>", "message": "<text>" } }`.

Error codes: `permission-denied`, `invalid-request`, `not-found`, `not-a-note`, `conflict`,
`too-large`, `io`, `unsupported`, `no-open-note`.

Paths are relative to the Vault root, `/`-separated, without a leading `/`, and may not contain
hidden components (`.laperm` and any other dot-prefixed name are refused with `invalid-request`).

| `op`             | Permission     | Request fields                                   | `value`                                                                 |
|------------------|----------------|--------------------------------------------------|-------------------------------------------------------------------------|
| `log`            | none           | `level`: `debug`/`info`/`notice`/`error`, `message` | nil. Goes to OSLog, subsystem `app.kymmt.Laperm`, category `plugin.<id>` |
| `clock.now`      | none           | —                                                | `{ timestamp: i64 ms since Unix epoch, offsetSeconds: i64 from UTC, year, month, day, hour, minute, second, weekday (1 = Sunday … 7 = Saturday) }` in the device's local time zone |
| `settings.get`   | none           | —                                                | `{ values: { <key>: <value> } }` — every declared setting, Manifest defaults filled in |
| `editor.get`     | `editor`       | —                                                | nil when no Note is open, else `{ path, text, version: i64, selection: { start, end } }` — `start`/`end` are **UTF-8 byte offsets** into `text` |
| `editor.replace` | `editor`       | `version`, `start`, `end`, `text`                | `{ version }` (the new version). `conflict` if `version` is not the current one (the user typed since `editor.get`); `invalid-request` if the range is not on character boundaries; `no-open-note` |
| `editor.open`    | `editor`       | `path`                                           | nil. `not-found` if there is no such Note, `not-a-note` if the path is not a Markdown file |
| `vault.read`     | `vault.read`   | `path`                                           | `{ data: bin }` (any non-hidden file: Note or Attachment) |
| `vault.write`    | `vault.write`  | `path`, `data: bin`                              | nil. Creates missing parent folders. `conflict` if `path` is the open Note (use `editor.replace`). Produces `noteCreated` / `noteSaved` when the path is a Note |
| `vault.exists`   | `vault.read`   | `path`                                           | `{ exists: bool, isFolder: bool }` |
| `vault.list`     | `vault.read`   | `folder` (default `""`), `recursive` (default false) | `{ notes: [path], folders: [path] }` — Notes and folders only (no Attachments), sorted |
| `storage.get`    | `storage`      | —                                                | `{ value: <any MessagePack value> }`, `value` nil when nothing was stored |
| `storage.set`    | `storage`      | `value`                                          | nil. The whole document is replaced; the encoded value may not exceed 4 MB (`too-large`) |
| `notice`         | `notice`       | `message`                                        | nil. A transient banner |

`network` is reserved: a Manifest may list it, but every op that would need it is `unsupported`
in v1.

## 4. Manifest (`manifest.json`)

```json
{
  "id": "daily-note",
  "name": "Daily Note",
  "version": "1.0.0",
  "apiVersion": 1,
  "minAppVersion": "1.0",
  "description": "Opens today's Note, creating it from a template.",
  "author": "k-ymmt",
  "authorUrl": "https://github.com/k-ymmt",
  "isDesktopOnly": false,
  "permissions": ["editor", "vault.read", "vault.write"],
  "commands": [
    { "id": "open-today", "name": "Open Today's Note", "shortcut": "cmd+shift+t",
      "systemImage": "calendar", "needsOpenNote": false }
  ],
  "events": ["noteCreated"],
  "settings": [
    { "key": "folder", "label": "Folder", "type": "string", "default": "Daily" },
    { "key": "dateFormat", "label": "Date format", "type": "enum", "default": "yyyy-MM-dd",
      "options": [ { "value": "yyyy-MM-dd", "label": "2026-09-28" }, { "value": "yyyyMMdd", "label": "20260928" } ] },
    { "key": "template", "label": "Template", "type": "string", "default": "# {{date}}\n\n" }
  ]
}
```

- `id`: lowercase letters, digits and hyphens, 1–64 characters, must equal the folder name;
  `laperm` is reserved.
- `version`: the release tag (semantic version, no `v` prefix).
- `apiVersion` (required, integer): Laperm refuses to enable a Plugin whose `apiVersion` it does not
  support. `minAppVersion` is optional and compared with Laperm's marketing version.
- `isDesktopOnly`: iOS refuses to enable the Plugin and says why.
- `permissions`: any of `editor`, `vault.read`, `vault.write`, `storage`, `events`, `notice`,
  `network` (reserved). Calling an op without its Permission is `permission-denied`; subscribing to
  events without `events` is refused at load.
- `commands[]`: `id` (same character rules as the Plugin id), `name`, optional `shortcut`
  (`cmd` / `shift` / `opt` / `ctrl` joined with `+`, then one key character, e.g. `cmd+shift+t`),
  optional `systemImage` (SF Symbol), `needsOpenNote` (default `true`). The Command ID in Laperm is
  `plugin.<pluginID>.<id>`; all of a Plugin's Commands live in the `plugins` group. A shortcut that
  collides with a built-in Command or a Plugin enabled earlier is dropped (the item stays).
- `events[]`: subset of the five event names.
- `settings[]`: `key`, `label`, `type` (`bool` / `string` / `number` / `enum`), `default`, optional
  `description`, and for `enum` an `options[]` of `{ value, label }`. Laperm renders the form; the
  values are stored in `settings.json` and read with `settings.get`.

## 5. Limits and failure

| Limit                           | Value                                        |
|---------------------------------|----------------------------------------------|
| Fuel per Command (and `laperm_init`) | 10,000,000,000 (≈ one Wasm operator each; about 2.5 s on an M-series Mac in a release build, more on a phone) |
| Fuel per event                  | 2,000,000,000                                |
| Linear memory                   | 64 MB (growth beyond it fails, `memory.grow` returns -1) |
| One message                     | 32 MB                                        |
| Plugin Data                     | 4 MB encoded                                 |
| Download (`manifest.json` + `plugin.wasm`) | 20 MB                             |

Running out of fuel, a trap, a non-zero `laperm_init`, a malformed message, or an op without its
Permission **stops the Plugin for the rest of the Vault Session**. The reason is shown in the
Manage Plugins screen and once as a banner. `enabled` in `plugins.json` is not changed; only the user
disables a Plugin. `Reload Plugins` starts everything again.

## 6. Distribution

A release on GitHub carries two assets, `manifest.json` and `plugin.wasm`. Laperm fetches
`https://github.com/<owner>/<repo>/releases/latest/download/{manifest.json,plugin.wasm}`, checks
that the Manifest `id` matches, `apiVersion` is supported, the download is under 20 MB, the module
parses, and its only import is `laperm.call`, then writes both files into
`.laperm/plugins/<id>/`. Once a day per Plugin it fetches `manifest.json` again and offers an update
when `version` differs; nothing is updated automatically.
