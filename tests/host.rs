//! Exercises the wire format against a fake host, natively.

use laperm_plugin::abi::testing;
use laperm_plugin::{clock, editor, notice, settings, storage, vault, Error, Event, EventName, Origin, Plugin};
use rmp_serde::to_vec_named;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Deserialize, Debug)]
struct Request {
    op: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    version: Option<i64>,
    #[serde(default, with = "serde_bytes")]
    data: Option<Vec<u8>>,
}

#[derive(Serialize)]
struct Ok<T: Serialize> {
    ok: bool,
    value: T,
}

#[derive(Serialize)]
struct Fail<'a> {
    ok: bool,
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

fn ok<T: Serialize>(value: T) -> Vec<u8> {
    to_vec_named(&Ok { ok: true, value }).unwrap()
}

fn fail(code: &str, message: &str) -> Vec<u8> {
    to_vec_named(&Fail { ok: false, error: ErrorBody { code, message } }).unwrap()
}

#[test]
fn requests_are_named_maps_and_responses_decode() {
    let seen: Rc<RefCell<Vec<Request>>> = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    testing::set_handler(move |bytes| {
        let request: Request = rmp_serde::from_slice(bytes).unwrap();
        let response = match request.op.as_str() {
            "notice" | "log" => ok(()),
            "clock.now" => {
                #[derive(Serialize)]
                #[serde(rename_all = "camelCase")]
                struct T { timestamp: i64, offset_seconds: i64, year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32, weekday: u32 }
                ok(T { timestamp: 1_790_000_000_000, offset_seconds: 32400, year: 2026, month: 9, day: 21, hour: 23, minute: 13, second: 20, weekday: 2 })
            }
            "settings.get" => {
                #[derive(Serialize)]
                struct V { values: S }
                #[derive(Serialize)]
                struct S { folder: String, count: f64, loud: bool }
                ok(V { values: S { folder: "Daily".into(), count: 2.0, loud: true } })
            }
            "editor.get" => {
                #[derive(Serialize)]
                struct E { path: String, text: String, version: i64, selection: Sel }
                #[derive(Serialize)]
                struct Sel { start: usize, end: usize }
                ok(E { path: "A.md".into(), text: "日本 x".into(), version: 3, selection: Sel { start: 3, end: 6 } })
            }
            "editor.replace" => {
                if request.version == Some(3) {
                    #[derive(Serialize)]
                    struct V { version: i64 }
                    ok(V { version: 4 })
                } else {
                    fail("conflict", "the Note changed since editor.get")
                }
            }
            "vault.read" => {
                #[derive(Serialize)]
                struct D { #[serde(with = "serde_bytes")] data: Vec<u8> }
                ok(D { data: b"# A".to_vec() })
            }
            "vault.write" | "storage.set" | "editor.open" => ok(()),
            "vault.exists" => {
                #[derive(Serialize)]
                #[serde(rename_all = "camelCase")]
                struct K { exists: bool, is_folder: bool }
                ok(K { exists: true, is_folder: false })
            }
            "vault.list" => {
                #[derive(Serialize)]
                struct L { notes: Vec<String>, folders: Vec<String> }
                ok(L { notes: vec!["A.md".into()], folders: vec![] })
            }
            "storage.get" => {
                #[derive(Serialize)]
                struct V { value: Option<Doc> }
                ok(V { value: Some(Doc { count: 7 }) })
            }
            other => fail("invalid-request", &format!("unknown op {other}")),
        };
        log.borrow_mut().push(request);
        response
    });

    notice("hi").unwrap();
    laperm_plugin::log::info("logged").unwrap();
    let now = clock::now().unwrap();
    assert_eq!(now.date_iso(), "2026-09-21");
    assert_eq!(now.time_hm(), "23:13");
    assert_eq!(now.format("yyyy/MM/dd HH:mm:ss"), "2026/09/21 23:13:20");
    assert_eq!(now.format("yyyyMMdd"), "20260921");

    #[derive(Deserialize, Debug, PartialEq)]
    struct S { folder: String, count: f64, loud: bool }
    assert_eq!(settings::get::<S>().unwrap(), S { folder: "Daily".into(), count: 2.0, loud: true });

    let state = editor::get().unwrap().unwrap();
    assert_eq!(state.selected_text(), "本");
    assert_eq!(state.replace_selection("語").unwrap(), 4);
    match editor::replace(2, 0, 0, "!") {
        Err(Error::Host(e)) => assert!(e.is_conflict()),
        other => panic!("{other:?}"),
    }
    editor::open("A.md").unwrap();

    assert_eq!(vault::read_text("A.md").unwrap(), "# A");
    vault::write_text("New/B.md", "b").unwrap();
    assert!(vault::exists("A.md").unwrap().exists);
    assert_eq!(vault::list("", true).unwrap().notes, vec!["A.md"]);

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Doc { count: u32 }
    assert_eq!(storage::get::<Doc>().unwrap(), Some(Doc { count: 7 }));
    storage::set(&Doc { count: 8 }).unwrap();

    let seen = seen.borrow();
    assert_eq!(seen[0].op, "notice");
    assert_eq!(seen[0].message.as_deref(), Some("hi"));
    let write = seen.iter().find(|r| r.op == "vault.write").unwrap();
    assert_eq!(write.path.as_deref(), Some("New/B.md"));
    assert_eq!(write.data.as_deref(), Some(&b"b"[..]), "data goes on the wire as bin");
    let replace = seen.iter().find(|r| r.op == "editor.replace").unwrap();
    assert_eq!(replace.text.as_deref(), Some("語"));
    testing::clear_handler();
}

struct Recorder {
    commands: Vec<String>,
    events: Vec<Event>,
}

impl Plugin for Recorder {
    fn new() -> laperm_plugin::Result<Self> {
        Ok(Recorder { commands: vec![], events: vec![] })
    }
    fn run_command(&mut self, command: &str) -> laperm_plugin::Result<()> {
        self.commands.push(command.to_owned());
        if command == "fail" { Err("nope".into()) } else { Ok(()) }
    }
    fn on_event(&mut self, event: &Event) -> laperm_plugin::Result<()> {
        self.events.push(event.clone());
        Ok(())
    }
}

#[test]
fn host_messages_dispatch_to_the_plugin() {
    use laperm_plugin::abi::handle_message;
    let mut plugin = Recorder::new().unwrap();

    #[derive(Serialize)]
    struct Cmd<'a> { r#type: &'a str, command: &'a str }
    let response = handle_message(&mut plugin, &to_vec_named(&Cmd { r#type: "command", command: "go" }).unwrap());
    assert_eq!(response, vec![0x81, 0xa2, b'o', b'k', 0xc3]);
    let response = handle_message(&mut plugin, &to_vec_named(&Cmd { r#type: "command", command: "fail" }).unwrap());
    #[derive(Deserialize)]
    struct Out { ok: bool, error: Option<String> }
    let out: Out = rmp_serde::from_slice(&response).unwrap();
    assert!(!out.ok);
    assert_eq!(out.error.as_deref(), Some("nope"));

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Ev<'a> { r#type: &'a str, event: &'a str, path: &'a str, origin: &'a str, old_path: Option<&'a str> }
    handle_message(&mut plugin, &to_vec_named(&Ev { r#type: "event", event: "noteRenamed", path: "B.md", origin: "plugin:daily-note", old_path: Some("A.md") }).unwrap());
    handle_message(&mut plugin, &to_vec_named(&Ev { r#type: "event", event: "noteOpened", path: "C.md", origin: "user", old_path: None }).unwrap());
    assert_eq!(plugin.commands, vec!["go", "fail"]);
    assert_eq!(plugin.events[0], Event { name: EventName::NoteRenamed, path: "B.md".into(), origin: Origin::Plugin("daily-note".into()), old_path: Some("A.md".into()) });
    assert!(plugin.events[0].origin.is_plugin("daily-note"));
    assert_eq!(plugin.events[1].origin, Origin::User);

    let garbage = handle_message(&mut plugin, b"\xc0");
    let out: Out = rmp_serde::from_slice(&garbage).unwrap();
    assert!(!out.ok);
}

#[test]
fn abi_packing_round_trips() {
    use laperm_plugin::abi::{cabi_realloc, pack, unpack};
    assert_eq!(unpack(pack(0x1234, 0x56)), (0x1234, 0x56));
    assert_eq!(unpack(pack(u32::MAX, u32::MAX)), (u32::MAX, u32::MAX));
    assert_eq!(pack(0, 0), 0);
    unsafe {
        let ptr = cabi_realloc(std::ptr::null_mut(), 0, 1, 5);
        assert!(!ptr.is_null());
        std::ptr::copy_nonoverlapping(b"hello".as_ptr(), ptr, 5);
        let grown = cabi_realloc(ptr, 5, 1, 10);
        assert_eq!(std::slice::from_raw_parts(grown, 5), b"hello");
        assert!(cabi_realloc(grown, 10, 1, 0).is_null());
        assert!(cabi_realloc(std::ptr::null_mut(), 0, 1, 0).is_null());
    }
}

#[test]
fn manifest_round_trips_and_validates() {
    use laperm_plugin::manifest::Manifest;
    let json = r#"{ "id": "daily-note", "name": "Daily Note", "version": "1.0.0", "apiVersion": 1,
      "permissions": ["editor", "vault.read", "vault.write"],
      "commands": [ { "id": "open-today", "name": "Open Today's Note", "shortcut": "cmd+shift+t", "needsOpenNote": false } ],
      "settings": [ { "key": "folder", "label": "Folder", "type": "string", "default": "Daily" },
                    { "key": "dateFormat", "type": "enum", "default": "yyyy-MM-dd", "options": [ { "value": "yyyy-MM-dd" } ] } ] }"#;
    let manifest: Manifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.validate(), Ok(()));
    assert!(!manifest.commands[0].needs_open_note);
    let mut bad = manifest.clone();
    bad.id = "Daily".into();
    assert!(bad.validate().is_err());
    bad = manifest.clone();
    bad.events = vec!["noteCreated".into()];
    assert!(bad.validate().unwrap_err().contains("events permission"));
    let again: Manifest = serde_json::from_str(&serde_json::to_string(&manifest).unwrap()).unwrap();
    assert_eq!(again, manifest);
}
