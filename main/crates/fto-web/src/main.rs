//! Unsafe code is confined to the C ABI string boundary; the solver stays safe Rust.
use std::{
    ffi::{CStr, CString, c_char},
    sync::Mutex,
};
static OUTPUT: Mutex<Option<CString>> = Mutex::new(None);
fn output(value: String) -> *const c_char {
    let mut slot = OUTPUT.lock().unwrap();
    *slot = Some(CString::new(value).expect("JSON has no literal NUL"));
    slot.as_ref().unwrap().as_ptr()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fto_dispatch(input: *const c_char) -> *const c_char {
    // Emscripten ccall owns a valid UTF-8, NUL-terminated argument for this call.
    let input = unsafe { CStr::from_ptr(input) }.to_str().unwrap_or("");
    output(fto_web_backend::browser::dispatch(input))
}
#[unsafe(no_mangle)]
pub extern "C" fn fto_events() -> *const c_char {
    output(fto_web_backend::browser::events())
}
fn main() {}
