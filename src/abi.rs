//! The wire-level glue: buffers in linear memory, the one host import, and the dispatch of
//! host → Plugin messages. Plugin authors normally only use [`export_plugin!`](crate::export_plugin).

use crate::{Error, Event, EventName, Origin, Plugin, Result};
use serde::{Deserialize, Serialize};
use std::alloc::{alloc, dealloc, realloc, Layout};

/// Packs a buffer's `(ptr, len)` into the `i64` the ABI uses: `(ptr << 32) | len`.
pub fn pack(ptr: u32, len: u32) -> i64 {
    (((ptr as u64) << 32) | len as u64) as i64
}

/// The inverse of [`pack`].
pub fn unpack(packed: i64) -> (u32, u32) {
    let value = packed as u64;
    ((value >> 32) as u32, value as u32)
}

/// The allocator Laperm calls for request buffers (same name and signature as the Component
/// Model canonical ABI). `new_size == 0` frees. Alignment 1 is what Laperm uses.
///
/// # Safety
/// `old_ptr` must be null or a pointer returned by this function with `old_size`, `align`.
pub unsafe fn cabi_realloc(old_ptr: *mut u8, old_size: usize, align: usize, new_size: usize) -> *mut u8 {
    let align = align.max(1);
    if new_size == 0 {
        if !old_ptr.is_null() && old_size != 0 {
            dealloc(old_ptr, Layout::from_size_align_unchecked(old_size, align));
        }
        return std::ptr::null_mut();
    }
    let layout = Layout::from_size_align_unchecked(new_size, align);
    let ptr = if old_ptr.is_null() || old_size == 0 {
        alloc(layout)
    } else {
        realloc(old_ptr, Layout::from_size_align_unchecked(old_size, align), new_size)
    };
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    ptr
}

/// Moves `bytes` into a buffer Laperm will free with `cabi_realloc(ptr, len, 1, 0)` and returns
/// the packed pointer. Only meaningful in wasm32, where pointers fit in 32 bits.
#[cfg(target_arch = "wasm32")]
pub fn leak_response(bytes: Vec<u8>) -> i64 {
    let len = bytes.len();
    if len == 0 {
        return 0;
    }
    // Reallocate with alignment 1 so the host's `cabi_realloc(ptr, len, 1, 0)` matches the layout.
    let ptr = unsafe { cabi_realloc(std::ptr::null_mut(), 0, 1, len) };
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len) };
    pack(ptr as u32, len as u32)
}

/// Takes ownership of a buffer Laperm allocated with `cabi_realloc(0, 0, 1, len)` (a host
/// response) and returns its bytes, freeing the buffer.
///
/// # Safety
/// `packed` must come from `laperm.call`.
#[cfg(target_arch = "wasm32")]
pub unsafe fn take_host_buffer(packed: i64) -> Vec<u8> {
    let (ptr, len) = unpack(packed);
    if ptr == 0 || len == 0 {
        return Vec::new();
    }
    let slice = std::slice::from_raw_parts(ptr as *const u8, len as usize);
    let bytes = slice.to_vec();
    cabi_realloc(ptr as *mut u8, len as usize, 1, 0);
    bytes
}

#[cfg(target_arch = "wasm32")]
mod imports {
    #[link(wasm_import_module = "laperm")]
    extern "C" {
        pub fn call(ptr: *const u8, len: u32) -> i64;
    }
}

/// Sends one MessagePack request to Laperm and returns the raw response bytes.
pub fn call_raw(request: &[u8]) -> Vec<u8> {
    #[cfg(target_arch = "wasm32")]
    {
        let packed = unsafe { imports::call(request.as_ptr(), request.len() as u32) };
        unsafe { take_host_buffer(packed) }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        testing::dispatch(request)
    }
}

/// Off wasm (unit tests), `laperm.call` is routed to a handler installed with
/// [`testing::set_handler`], so a Plugin's logic can be tested natively.
#[cfg(not(target_arch = "wasm32"))]
pub mod testing {
    use std::cell::RefCell;

    type Handler = Box<dyn FnMut(&[u8]) -> Vec<u8>>;

    thread_local! {
        static HANDLER: RefCell<Option<Handler>> = const { RefCell::new(None) };
    }

    /// Installs the fake host for the current thread.
    pub fn set_handler(handler: impl FnMut(&[u8]) -> Vec<u8> + 'static) {
        HANDLER.with(|h| *h.borrow_mut() = Some(Box::new(handler)));
    }

    /// Removes the fake host.
    pub fn clear_handler() {
        HANDLER.with(|h| *h.borrow_mut() = None);
    }

    pub(crate) fn dispatch(request: &[u8]) -> Vec<u8> {
        HANDLER.with(|h| match h.borrow_mut().as_mut() {
            Some(handler) => handler(request),
            None => panic!("laperm.call was made outside wasm without a test handler (laperm_plugin::abi::testing::set_handler)"),
        })
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    #[serde(default = "Option::default")]
    value: Option<T>,
    #[serde(default)]
    error: Option<crate::HostError>,
}

/// Encodes `request` as a named map, sends it, and decodes `value` from the response.
/// `Req` must serialize to a map that includes `"op"`.
pub fn call<Req: Serialize, Resp: for<'de> Deserialize<'de>>(request: &Req) -> Result<Resp> {
    let bytes = rmp_serde::to_vec_named(request)?;
    let response = call_raw(&bytes);
    let envelope: Envelope<Resp> = rmp_serde::from_slice(&response)?;
    if envelope.ok {
        match envelope.value {
            Some(value) => Ok(value),
            None => rmp_serde::from_slice::<Resp>(&[0xc0]).map_err(Into::into), // nil
        }
    } else {
        Err(Error::Host(envelope.error.unwrap_or_else(|| crate::HostError {
            code: "unknown".into(),
            message: "Laperm returned ok: false without an error".into(),
        })))
    }
}

/// A host → Plugin message (`docs/plugin-api.md` §2).
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Incoming {
    /// `{ "type": "command", "command": "<name>" }`
    Command {
        /// The Command name from the Manifest.
        command: String,
    },
    /// `{ "type": "event", "event": ..., "path": ..., "origin": ..., "oldPath"?: ... }`
    #[serde(rename_all = "camelCase")]
    Event {
        /// The event name.
        event: EventName,
        /// The Note's path.
        path: String,
        /// `user` or `plugin:<id>`.
        origin: String,
        /// The previous path for `noteRenamed`.
        old_path: Option<String>,
    },
}

#[derive(Serialize)]
struct Outcome<'a> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

/// Decodes a host message, runs it against `plugin`, and returns the encoded response.
pub fn handle_message<P: Plugin>(plugin: &mut P, message: &[u8]) -> Vec<u8> {
    let result = match rmp_serde::from_slice::<Incoming>(message) {
        Ok(Incoming::Command { command }) => plugin.run_command(&command),
        Ok(Incoming::Event { event, path, origin, old_path }) => plugin.on_event(&Event {
            name: event,
            path,
            origin: Origin::parse(&origin),
            old_path,
        }),
        Err(e) => Err(Error::Encoding(format!("unreadable message from Laperm: {e}"))),
    };
    let text;
    let outcome = match &result {
        Ok(()) => Outcome { ok: true, error: None },
        Err(e) => {
            text = e.to_string();
            Outcome { ok: false, error: Some(&text) }
        }
    };
    rmp_serde::to_vec_named(&outcome).unwrap_or_else(|_| vec![0x82, 0xa2, b'o', b'k', 0xc2, 0xa5, b'e', b'r', b'r', b'o', b'r', 0xa0])
}

/// Installs a panic hook that sends the panic message to Laperm's log (the module then traps and
/// Laperm stops the Plugin with that reason visible in the log).
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let _ = crate::host::log::error(&format!("panic: {info}"));
    }));
}

/// Defines the exports Laperm needs for a [`Plugin`] type. The exports exist only in a wasm32 build,
/// so the Plugin crate's own native `cargo test` still compiles.
#[macro_export]
macro_rules! export_plugin {
    ($plugin:ty) => {
        #[cfg(target_arch = "wasm32")]
        mod __laperm_exports {
            use super::*;

            static mut PLUGIN: Option<$plugin> = None;

            #[no_mangle]
            pub unsafe extern "C" fn cabi_realloc(old_ptr: *mut u8, old_size: usize, align: usize, new_size: usize) -> *mut u8 {
                $crate::abi::cabi_realloc(old_ptr, old_size, align, new_size)
            }

            #[no_mangle]
            pub extern "C" fn laperm_init() -> i32 {
                $crate::abi::install_panic_hook();
                match <$plugin as $crate::Plugin>::new() {
                    Ok(plugin) => {
                        unsafe { PLUGIN = Some(plugin) };
                        0
                    }
                    Err(e) => {
                        let _ = $crate::log::error(&format!("init failed: {e}"));
                        1
                    }
                }
            }

            #[no_mangle]
            pub unsafe extern "C" fn laperm_handle(ptr: *const u8, len: u32) -> i64 {
                let message = std::slice::from_raw_parts(ptr, len as usize);
                let plugin = match (*std::ptr::addr_of_mut!(PLUGIN)).as_mut() {
                    Some(plugin) => plugin,
                    None => return $crate::abi::leak_response(Vec::new()),
                };
                let response = $crate::abi::handle_message(plugin, message);
                $crate::abi::leak_response(response)
            }
        }
    };
}
