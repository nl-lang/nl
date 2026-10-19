//! The C ABI of `nltext.h` (`LuaJIT/src/nltext.h`): what the VM's string
//! glue (`lib_nltext.c`) calls. Exported through the `libnlc` static
//! library of `nl_capi`.
//!
//! The boundary rules: no function here calls into Lua, and none unwinds.
//! Every exported function runs under `catch_unwind` and reports a panic
//! as a failure value (`0`, `NULL`, `NLTEXT_NONE`), never by unwinding into
//! C. Byte offsets are 0-based; the NL positions that `nltext_slice` and
//! `nltext_charat` take are 1-based and negative from the end, as NL
//! defines them.

use std::ffi::{c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use crate::CaseMode;

/// `NLTEXT_NONE`: "no position".
pub const NLTEXT_NONE: usize = usize::MAX;

/// The bytes of a call, or empty when `s` is NULL.
///
/// # Safety
///
/// `s` is NULL or points at `len` readable bytes.
unsafe fn bytes<'a>(s: *const c_char, len: usize) -> &'a [u8] {
    if s.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller's guarantee.
        unsafe { std::slice::from_raw_parts(s.cast::<u8>(), len) }
    }
}

/// # Safety
///
/// `p` is NULL or a valid out pointer.
unsafe fn put<T>(p: *mut T, v: T) {
    if !p.is_null() {
        // SAFETY: the caller's guarantee.
        unsafe { *p = v };
    }
}

/// Whether the text library is linked: `1` here, `0` in the VM's stub
/// (`lj_nlnotext.c`).
#[unsafe(no_mangle)]
pub extern "C" fn nltext_available() -> c_int {
    1
}

/// The byte index of the first invalid UTF-8 sequence, or `NLTEXT_NONE`
/// when `s[0..len]` is valid.
///
/// # Safety
///
/// `s` points at `len` readable bytes (or `len` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nltext_invalid_at(s: *const c_char, len: usize) -> usize {
    // SAFETY: the caller's guarantee.
    let b = unsafe { bytes(s, len) };
    catch_unwind(AssertUnwindSafe(|| crate::invalid_at(b).unwrap_or(NLTEXT_NONE))).unwrap_or(0)
}

/// The number of characters.
///
/// # Safety
///
/// `s` points at `len` readable bytes (or `len` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nltext_count(s: *const c_char, len: usize) -> usize {
    // SAFETY: the caller's guarantee.
    let b = unsafe { bytes(s, len) };
    catch_unwind(AssertUnwindSafe(|| crate::count(b))).unwrap_or(0)
}

/// The byte offset where the character with 0-based index `pos` starts,
/// `len` when there is no such character.
///
/// # Safety
///
/// `s` points at `len` readable bytes (or `len` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nltext_offset(s: *const c_char, len: usize, pos: usize) -> usize {
    // SAFETY: the caller's guarantee.
    let b = unsafe { bytes(s, len) };
    catch_unwind(AssertUnwindSafe(|| crate::offset(b, pos))).unwrap_or(len)
}

/// The number of characters that start before byte offset `byte`.
///
/// # Safety
///
/// `s` points at `len` readable bytes (or `len` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nltext_charpos(s: *const c_char, len: usize, byte: usize) -> usize {
    // SAFETY: the caller's guarantee.
    let b = unsafe { bytes(s, len) };
    catch_unwind(AssertUnwindSafe(|| crate::charpos(b, byte))).unwrap_or(0)
}

/// `s[first..last]` (`s[first...last]` when `exclusive`): sets `*start`
/// and `*end` to the byte range of the selected characters and returns 1,
/// or returns 0 when nothing is selected.
///
/// # Safety
///
/// `s` points at `len` readable bytes (or `len` is 0); `start` and `end`
/// are NULL or valid out pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nltext_slice(s: *const c_char, len: usize, first: i64, last: i64, exclusive: c_int, start: *mut usize, end: *mut usize) -> c_int {
    // SAFETY: the caller's guarantee.
    let b = unsafe { bytes(s, len) };
    match catch_unwind(AssertUnwindSafe(|| crate::slice(b, first, last, exclusive != 0))).unwrap_or(None) {
        Some((a, e)) => {
            // SAFETY: the caller's guarantee on the out pointers.
            unsafe {
                put(start, a);
                put(end, e);
            }
            1
        }
        None => 0,
    }
}

/// `s[pos]`: sets `*start` and `*end` to the byte range of the character
/// at NL position `pos` and returns 1, or returns 0 when it is out of
/// range.
///
/// # Safety
///
/// `s` points at `len` readable bytes (or `len` is 0); `start` and `end`
/// are NULL or valid out pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nltext_charat(s: *const c_char, len: usize, pos: i64, start: *mut usize, end: *mut usize) -> c_int {
    // SAFETY: the caller's guarantee.
    let b = unsafe { bytes(s, len) };
    match catch_unwind(AssertUnwindSafe(|| crate::char_at(b, pos))).unwrap_or(None) {
        Some((a, e)) => {
            // SAFETY: the caller's guarantee on the out pointers.
            unsafe {
                put(start, a);
                put(end, e);
            }
            1
        }
        None => 0,
    }
}

/// The case mapping `mode` (0 upcase, 1 downcase, 2 capitalize, 3
/// swapcase) of `s[0..len]`, which must be valid UTF-8: a new buffer of
/// `*outlen` bytes, freed with `nltext_free`; NULL when the input is not
/// valid UTF-8, the mode is unknown, or the mapping failed.
///
/// # Safety
///
/// `s` points at `len` readable bytes (or `len` is 0); `outlen` is NULL or
/// a valid out pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nltext_case(s: *const c_char, len: usize, mode: c_int, outlen: *mut usize) -> *mut c_char {
    // SAFETY: the caller's guarantee.
    let b = unsafe { bytes(s, len) };
    let mapped = catch_unwind(AssertUnwindSafe(|| {
        let mode = CaseMode::from_code(mode)?;
        let text = std::str::from_utf8(b).ok()?;
        Some(crate::map_case(text, mode).into_bytes().into_boxed_slice())
    }))
    .unwrap_or(None);
    match mapped {
        Some(out) => {
            let n = out.len();
            // SAFETY: the caller's guarantee on `outlen`.
            unsafe { put(outlen, n) };
            Box::into_raw(out).cast::<c_char>()
        }
        None => {
            // SAFETY: the caller's guarantee on `outlen`.
            unsafe { put(outlen, 0) };
            ptr::null_mut()
        }
    }
}

/// Frees a buffer of `nltext_case` of `len` bytes (NULL is allowed).
///
/// # Safety
///
/// `p` is NULL or a buffer of `nltext_case` with the `*outlen` it gave,
/// not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nltext_free(p: *mut c_char, len: usize) {
    if p.is_null() {
        return;
    }
    // SAFETY: `p` and `len` came from Box::into_raw of a boxed slice of
    // that length in nltext_case (the caller's guarantee).
    let boxed = unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(p.cast::<u8>(), len)) };
    let _ = catch_unwind(AssertUnwindSafe(move || drop(boxed)));
}

/// The code point that starts at byte offset `byte`, with its byte length
/// in `*clen`; `*clen` is 0 (and the result 0) at the end of the string or
/// where no valid character starts.
///
/// # Safety
///
/// `s` points at `len` readable bytes (or `len` is 0); `clen` is NULL or a
/// valid out pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nltext_decode(s: *const c_char, len: usize, byte: usize, clen: *mut usize) -> u32 {
    // SAFETY: the caller's guarantee.
    let b = unsafe { bytes(s, len) };
    let (cp, n) = catch_unwind(AssertUnwindSafe(|| crate::decode(b, byte))).unwrap_or(None).unwrap_or((0, 0));
    // SAFETY: the caller's guarantee on `clen`.
    unsafe { put(clen, n) };
    cp
}

/// Writes the UTF-8 bytes of code point `cp` to `out` (room for 4) and
/// returns their number; 0 when `cp` is not a Unicode scalar value.
///
/// # Safety
///
/// `out` is NULL or points at 4 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nltext_encode(cp: u32, out: *mut c_char) -> usize {
    match catch_unwind(AssertUnwindSafe(|| crate::encode(cp))).unwrap_or(None) {
        Some((buf, n)) => {
            if !out.is_null() {
                // SAFETY: the caller's guarantee: `out` has room for 4
                // bytes, and `n` is at most 4.
                unsafe { ptr::copy_nonoverlapping(buf.as_ptr(), out.cast::<u8>(), n) };
            }
            n
        }
        None => 0,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = "héllo";
        let p = s.as_ptr().cast::<c_char>();
        unsafe {
            assert_eq!(nltext_available(), 1);
            assert_eq!(nltext_invalid_at(p, s.len()), NLTEXT_NONE);
            assert_eq!(nltext_invalid_at(c"ab\xff".as_ptr(), 3), 2);
            assert_eq!(nltext_invalid_at(ptr::null(), 0), NLTEXT_NONE);
            assert_eq!(nltext_count(p, s.len()), 5);
            assert_eq!(nltext_offset(p, s.len(), 2), 3);
            assert_eq!(nltext_offset(p, s.len(), 9), s.len());
            assert_eq!(nltext_charpos(p, s.len(), 3), 2);
            let (mut a, mut b) = (0usize, 0usize);
            assert_eq!(nltext_slice(p, s.len(), -3, -1, 0, &mut a, &mut b), 1);
            assert_eq!(&s[a..b], "llo");
            assert_eq!(nltext_slice(p, s.len(), 1, 2, 1, &mut a, &mut b), 1);
            assert_eq!(&s[a..b], "h");
            assert_eq!(nltext_slice(p, s.len(), 3, 2, 0, ptr::null_mut(), ptr::null_mut()), 0);
            assert_eq!(nltext_charat(p, s.len(), 2, &mut a, &mut b), 1);
            assert_eq!(&s[a..b], "é");
            assert_eq!(nltext_charat(p, s.len(), 6, &mut a, &mut b), 0);
            let mut n = 0usize;
            let up = nltext_case(p, s.len(), 0, &mut n);
            assert_eq!(std::slice::from_raw_parts(up.cast::<u8>(), n), "HÉLLO".as_bytes());
            nltext_free(up, n);
            let empty = nltext_case(p, 0, 1, &mut n);
            assert!(!empty.is_null());
            assert_eq!(n, 0);
            nltext_free(empty, n);
            assert!(nltext_case(c"\xff".as_ptr(), 1, 0, &mut n).is_null());
            assert_eq!(n, 0);
            assert!(nltext_case(p, s.len(), 7, &mut n).is_null());
            nltext_free(ptr::null_mut(), 0);
            let mut cl = 0usize;
            assert_eq!(nltext_decode(p, s.len(), 1, &mut cl), 0xE9);
            assert_eq!(cl, 2);
            assert_eq!(nltext_decode(p, s.len(), 6, &mut cl), 0);
            assert_eq!(cl, 0);
            let mut out = [0 as c_char; 4];
            assert_eq!(nltext_encode(0x20AC, out.as_mut_ptr()), 3);
            assert_eq!(&out[..3], &[-30, -126, -84]);
            assert_eq!(nltext_encode(0xD800, out.as_mut_ptr()), 0);
            assert_eq!(nltext_encode(0x41, ptr::null_mut()), 1);
        }
    }
}
