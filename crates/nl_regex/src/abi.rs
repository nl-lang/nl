//! The C ABI of `nlregex.h` (`LuaJIT/src/nlregex.h`): what the VM's regex
//! glue (`lib_regex.c`) calls. Exported through the `libnlc` static library
//! of `nl_capi`.
//!
//! The boundary rules: no function here calls into Lua, and none unwinds.
//! Every exported function runs under `catch_unwind` and reports a panic as
//! a failure code (`0`, `NULL`), never by unwinding into C. Positions are
//! byte offsets; the runtime converts them to character positions.
//!
//! Ownership: `nlregex_compile` returns an object freed with
//! `nlregex_free`, and an error message freed with `nlregex_freemsg`.
//! Strings returned by `nlregex_pattern` and `nlregex_groupname` belong to
//! the object and are valid until it is freed.

use std::ffi::{CString, c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use crate::{NO_SPAN, Regex};

/// `nlregex`: opaque to C.
pub struct NlRegex(Regex);

/// A group that did not participate (`NLREGEX_NOSPAN` in `nlregex.h`).
pub const NLREGEX_NOSPAN: usize = NO_SPAN;

/// The subject bytes of a call, or empty when `s` is NULL.
///
/// # Safety
///
/// `s` is NULL or points at `len` readable bytes.
unsafe fn subject<'a>(s: *const c_char, len: usize) -> &'a [u8] {
    if s.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller's guarantee.
        unsafe { std::slice::from_raw_parts(s.cast::<u8>(), len) }
    }
}

fn message(text: &str) -> *mut c_char {
    // A NUL inside the text would cut the message; replace it.
    let clean = text.replace('\0', "\u{fffd}");
    CString::new(clean).map_or(ptr::null_mut(), CString::into_raw)
}

/// Whether the regex library is linked: `1` here, `0` in the VM's stub
/// (`lj_nlnotext.c`).
#[unsafe(no_mangle)]
pub extern "C" fn nlregex_available() -> c_int {
    1
}

/// Compiles the pattern `pat[0..len]`, with `size_limit` bytes of compiled
/// size allowed (0: the default). Returns the regex, or NULL with `*err`
/// set to a message (freed with `nlregex_freemsg`; NULL when even that
/// could not be made).
///
/// # Safety
///
/// `pat` points at `len` readable bytes (or `len` is 0); `err` is NULL or a
/// valid out pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlregex_compile(pat: *const c_char, len: usize, size_limit: usize, err: *mut *mut c_char) -> *mut NlRegex {
    // SAFETY: the caller's guarantee.
    let bytes = unsafe { subject(pat, len) };
    let result = catch_unwind(AssertUnwindSafe(|| match std::str::from_utf8(bytes) {
        Ok(text) => Regex::new(text, size_limit).map_err(|e| message(e.message())),
        Err(_) => Err(message("the pattern is not valid UTF-8")),
    }))
    .unwrap_or_else(|_| Err(message("internal error in the regex engine")));
    match result {
        Ok(re) => Box::into_raw(Box::new(NlRegex(re))),
        Err(msg) => {
            if !err.is_null() {
                // SAFETY: a non-NULL `err` is a valid out pointer.
                unsafe { *err = msg };
            } else if !msg.is_null() {
                // SAFETY: `msg` came from CString::into_raw just above.
                drop(unsafe { CString::from_raw(msg) });
            }
            ptr::null_mut()
        }
    }
}

/// Frees a regex (NULL is allowed).
///
/// # Safety
///
/// `re` is NULL or a result of `nlregex_compile` not yet freed, and is not
/// used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlregex_free(re: *mut NlRegex) {
    if re.is_null() {
        return;
    }
    // SAFETY: `re` came from Box::into_raw in nlregex_compile (the caller's
    // guarantee).
    let boxed = unsafe { Box::from_raw(re) };
    let _ = catch_unwind(AssertUnwindSafe(move || drop(boxed)));
}

/// Frees an error message of `nlregex_compile` (NULL is allowed).
///
/// # Safety
///
/// `msg` is NULL or a message of `nlregex_compile` not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlregex_freemsg(msg: *mut c_char) {
    if msg.is_null() {
        return;
    }
    // SAFETY: `msg` came from CString::into_raw (the caller's guarantee).
    let owned = unsafe { CString::from_raw(msg) };
    let _ = catch_unwind(AssertUnwindSafe(move || drop(owned)));
}

/// # Safety
///
/// `re` is NULL or a live regex.
unsafe fn with<'a, T>(re: *const NlRegex, f: impl FnOnce(&'a Regex) -> T, none: T) -> T {
    // SAFETY: the caller's guarantee.
    match unsafe { re.as_ref() } {
        Some(r) => catch_unwind(AssertUnwindSafe(|| f(&r.0))).unwrap_or(none),
        None => none,
    }
}

/// The heap the compiled regex takes, approximately, in bytes.
///
/// # Safety
///
/// `re` is NULL or a live regex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlregex_memory(re: *const NlRegex) -> usize {
    // SAFETY: the caller's guarantee.
    unsafe { with(re, Regex::memory_usage, 0) }
}

/// The number of capture groups, group 0 included.
///
/// # Safety
///
/// `re` is NULL or a live regex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlregex_ngroups(re: *const NlRegex) -> usize {
    // SAFETY: the caller's guarantee.
    unsafe { with(re, Regex::group_count, 0) }
}

/// The name of group `i` and its length in `*len`, or NULL when the group
/// is unnamed or does not exist. Not NUL-terminated.
///
/// # Safety
///
/// `re` is NULL or a live regex; `len` is NULL or a valid out pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlregex_groupname(re: *const NlRegex, i: usize, len: *mut usize) -> *const c_char {
    // SAFETY: the caller's guarantee.
    let name = unsafe { with(re, |r| r.group_name(i).map(|n| (n.as_ptr(), n.len())), None) };
    let (p, n) = name.unwrap_or((ptr::null(), 0));
    if !len.is_null() {
        // SAFETY: a non-NULL `len` is a valid out pointer.
        unsafe { *len = n };
    }
    p.cast::<c_char>()
}

/// The pattern and its length in `*len`. Not NUL-terminated.
///
/// # Safety
///
/// `re` is NULL or a live regex; `len` is NULL or a valid out pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlregex_pattern(re: *const NlRegex, len: *mut usize) -> *const c_char {
    // SAFETY: the caller's guarantee.
    let (p, n) = unsafe { with(re, |r| (r.pattern().as_ptr(), r.pattern().len()), (ptr::null(), 0)) };
    if !len.is_null() {
        // SAFETY: a non-NULL `len` is a valid out pointer.
        unsafe { *len = n };
    }
    p.cast::<c_char>()
}

/// Whether `s[0..len]` contains a match: 1 or 0.
///
/// # Safety
///
/// `re` is NULL or a live regex; `s` points at `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlregex_test(re: *const NlRegex, s: *const c_char, len: usize) -> c_int {
    // SAFETY: the caller's guarantees.
    let subject = unsafe { subject(s, len) };
    // SAFETY: the caller's guarantees.
    c_int::from(unsafe { with(re, |r| r.is_match(subject), false) })
}

/// The first match at or after byte offset `start`: returns 1 with
/// `*mstart` and `*mend` set to its byte span (end exclusive), or 0.
///
/// # Safety
///
/// `re` is NULL or a live regex; `s` points at `len` readable bytes;
/// `mstart` and `mend` are NULL or valid out pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlregex_find(re: *const NlRegex, s: *const c_char, len: usize, start: usize, mstart: *mut usize, mend: *mut usize) -> c_int {
    // SAFETY: the caller's guarantees.
    let subject = unsafe { subject(s, len) };
    // SAFETY: the caller's guarantees.
    match unsafe { with(re, |r| r.find_at(subject, start), None) } {
        Some((a, b)) => {
            if !mstart.is_null() {
                // SAFETY: a non-NULL `mstart` is a valid out pointer.
                unsafe { *mstart = a };
            }
            if !mend.is_null() {
                // SAFETY: a non-NULL `mend` is a valid out pointer.
                unsafe { *mend = b };
            }
            1
        }
        None => 0,
    }
}

/// The groups of the first match at or after `start`: fills `spans[0..n]`,
/// `n` being the smaller of `nspans` and twice the group count, with the
/// byte start and end of each group in turn, `NLREGEX_NOSPAN` for a group
/// that did not participate. Returns 1, or 0 for no match (`spans` is then
/// untouched).
///
/// # Safety
///
/// `re` is NULL or a live regex; `s` points at `len` readable bytes;
/// `spans` points at `nspans` writable `size_t` (or `nspans` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlregex_captures(re: *const NlRegex, s: *const c_char, len: usize, start: usize, spans: *mut usize, nspans: usize) -> c_int {
    // SAFETY: the caller's guarantees.
    let subject = unsafe { subject(s, len) };
    let out: &mut [usize] = if spans.is_null() || nspans == 0 {
        &mut []
    } else {
        // SAFETY: the caller's guarantee on `spans`.
        unsafe { std::slice::from_raw_parts_mut(spans, nspans) }
    };
    // SAFETY: the caller's guarantees.
    c_int::from(unsafe { with(re, |r| r.captures_at(subject, start, out), false) })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    fn compile(pat: &str) -> Result<*mut NlRegex, String> {
        let mut err: *mut c_char = ptr::null_mut();
        let re = unsafe { nlregex_compile(pat.as_ptr().cast(), pat.len(), 0, &mut err) };
        if re.is_null() {
            let msg = unsafe { CStr::from_ptr(err) }.to_str().unwrap().to_owned();
            unsafe { nlregex_freemsg(err) };
            Err(msg)
        } else {
            Ok(re)
        }
    }

    #[test]
    fn round_trip() {
        let re = compile(r"(?<y>\d{4})-(\d{2})").unwrap();
        unsafe {
            assert_eq!(nlregex_available(), 1);
            assert_eq!(nlregex_ngroups(re), 3);
            let mut n = 0;
            let p = nlregex_groupname(re, 1, &mut n);
            assert_eq!(std::slice::from_raw_parts(p.cast::<u8>(), n), b"y");
            assert!(nlregex_groupname(re, 2, &mut n).is_null());
            assert_eq!(n, 0);
            let p = nlregex_pattern(re, &mut n);
            assert_eq!(std::slice::from_raw_parts(p.cast::<u8>(), n), br"(?<y>\d{4})-(\d{2})".as_slice());
            let s = b"on 2026-10";
            assert_eq!(nlregex_test(re, s.as_ptr().cast(), s.len()), 1);
            let (mut a, mut b) = (0, 0);
            assert_eq!(nlregex_find(re, s.as_ptr().cast(), s.len(), 0, &mut a, &mut b), 1);
            assert_eq!((a, b), (3, 10));
            assert_eq!(nlregex_find(re, s.as_ptr().cast(), s.len(), 4, &mut a, &mut b), 0);
            let mut spans = [0usize; 6];
            assert_eq!(nlregex_captures(re, s.as_ptr().cast(), s.len(), 0, spans.as_mut_ptr(), 6), 1);
            assert_eq!(spans, [3, 10, 3, 7, 8, 10]);
            assert_eq!(nlregex_captures(re, s.as_ptr().cast(), s.len(), 0, ptr::null_mut(), 0), 1);
            assert!(nlregex_memory(re) > 0);
            nlregex_free(re);
        }
    }

    #[test]
    fn errors_and_nulls() {
        let e = compile("(").unwrap_err();
        assert!(e.contains("unclosed group"), "{e}");
        unsafe {
            let mut err: *mut c_char = ptr::null_mut();
            let bad = b"\xff";
            assert!(nlregex_compile(bad.as_ptr().cast(), 1, 0, &mut err).is_null());
            assert!(CStr::from_ptr(err).to_str().unwrap().contains("UTF-8"));
            nlregex_freemsg(err);
            // Without an out pointer the message is dropped, not leaked.
            assert!(nlregex_compile(c"(".as_ptr(), 1, 0, ptr::null_mut()).is_null());
            assert_eq!(nlregex_test(ptr::null(), ptr::null(), 0), 0);
            assert_eq!(nlregex_ngroups(ptr::null()), 0);
            assert!(nlregex_pattern(ptr::null(), ptr::null_mut()).is_null());
            nlregex_free(ptr::null_mut());
            nlregex_freemsg(ptr::null_mut());
        }
    }
}
