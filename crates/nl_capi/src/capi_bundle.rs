//! The C ABI of `include/nlbundle.h`: the packager ([`crate::bundle`]), the
//! minifier ([`crate::minify`]) and sourcemaps ([`crate::sourcemap`]) for a
//! host in C or C++, such as a Garry's Mod `lua_shared` built on the VM.
//! The `#[repr(C)]` types here must match the header.
//!
//! The rules of `capi.rs` hold: nothing here enters a VM (the host's
//! callbacks do, if they like), and no Rust panic crosses into C: every
//! exported function runs its body under `catch_unwind`, and a panic is a
//! NULL result or a diagnostic with the code `"internal-error"`. Memory
//! this library returns is freed only by its own `nlb_*_free` functions.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use crate::bundle::{BundleOptions, DEFAULT_PART_LIMIT};
use crate::capi::{CLoader, NlcDiagnostic, NlcLoader, c_diagnostics, c_string, panic_message};
use crate::driver::{CompileOptions, Diagnostic, Localize, Profile, Severity};
use crate::minify::MinifyOptions;
use crate::sourcemap::SourceMap;

/// `NLB_ABI_VERSION`: changes when a type or function of the header does.
pub const NLB_ABI_VERSION: c_int = 2;

pub const NLB_PROFILE_PORTABLE: c_int = 0;
pub const NLB_PROFILE_GMOD_CLIENT: c_int = 1;

pub const NLB_MINIFY_GLUA: c_int = 1;
pub const NLB_MINIFY_ESCAPE_MACRO: c_int = 2;

/// The size of the buffer a check writes its message to.
const CHECK_ERROR_MAX: usize = 1024;

/// `nlb_Check`.
pub type NlbCheck = unsafe extern "C" fn(data: *mut c_void, chunkname: *const c_char, text: *const c_char, size: usize, err: *mut c_char, errsize: usize) -> c_int;

/// `nlb_Options`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NlbOptions {
    pub profile: c_int,
    pub name: *const c_char,
    pub checks: c_int,
    pub minify: c_int,
    pub case_tables: c_int,
    pub part_limit: usize,
    pub sourcemap: c_int,
    pub localize: c_int,
    pub decls: *const *const c_char,
    pub ndecls: usize,
    pub check: Option<NlbCheck>,
    pub check_data: *mut c_void,
    pub runtime_include: *const c_char,
    pub include_parts: *const c_char,
}

/// `nlb_Result`: opaque to C.
pub struct NlbResult {
    /// (file name, NUL-terminated text) of each part.
    parts: Vec<(CString, Vec<u8>)>,
    build: CString,
    diagnostics: Vec<NlcDiagnostic>,
    /// Owns the strings that `diagnostics` points into.
    _strings: Vec<CString>,
    sourcemap: Option<Box<NlbSourceMap>>,
}

/// `nlb_SourceMap`: opaque to C.
pub struct NlbSourceMap {
    map: SourceMap,
    bundle: CString,
    build: CString,
    /// The source files, for `nlb_sourcemap_lookup`'s `file`.
    files: Vec<CString>,
}

impl NlbSourceMap {
    fn new(map: SourceMap) -> NlbSourceMap {
        NlbSourceMap { bundle: c_string(&map.bundle), build: c_string(&map.build), files: map.files.iter().map(|f| c_string(f)).collect(), map }
    }
}

/// `nlb_Text`: opaque to C. Either text (with a NUL after it) or an error.
pub struct NlbText {
    text: Option<Vec<u8>>,
    error: Option<CString>,
}

impl NlbText {
    fn ok(text: String) -> *mut NlbText {
        let mut bytes = text.into_bytes();
        bytes.push(0);
        Box::into_raw(Box::new(NlbText { text: Some(bytes), error: None }))
    }
    fn err(message: &str) -> *mut NlbText {
        Box::into_raw(Box::new(NlbText { text: None, error: Some(c_string(message)) }))
    }
}

fn with_nul(text: String) -> Vec<u8> {
    let mut bytes = text.into_bytes();
    bytes.push(0);
    bytes
}

/// The bytes at `p`, `size` long; empty for NULL.
///
/// # Safety
///
/// A non-NULL `p` has `size` readable bytes, alive while the slice is.
unsafe fn bytes<'a>(p: *const c_char, size: usize) -> &'a [u8] {
    if p.is_null() || size == 0 {
        return &[];
    }
    // SAFETY: as the caller promises.
    unsafe { std::slice::from_raw_parts(p.cast::<u8>(), size) }
}

/// A NUL-terminated string, or `None` for NULL.
///
/// # Safety
///
/// A non-NULL `p` is NUL-terminated and alive while the result is.
unsafe fn c_str<'a>(p: *const c_char) -> Option<&'a CStr> {
    if p.is_null() {
        None
    } else {
        // SAFETY: as the caller promises.
        Some(unsafe { CStr::from_ptr(p) })
    }
}

/// Copies `message` into `err[size]`, NUL-terminated and cut to fit.
///
/// # Safety
///
/// A non-NULL `err` has `size` writable bytes.
unsafe fn write_error(err: *mut c_char, size: usize, message: &str) {
    if err.is_null() || size == 0 {
        return;
    }
    let n = message.len().min(size - 1);
    // SAFETY: `err` has `size` writable bytes and `n + 1 <= size`.
    unsafe {
        ptr::copy_nonoverlapping(message.as_ptr().cast::<c_char>(), err, n);
        *err.add(n) = 0;
    }
}

/// The ABI version of the library (`NLB_ABI_VERSION`).
#[unsafe(no_mangle)]
pub extern "C" fn nlb_abi_version() -> c_int {
    NLB_ABI_VERSION
}

/// Fills `o` with the defaults (nlbundle.h).
///
/// # Safety
///
/// `o` is NULL or points to an `nlb_Options`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_options_init(o: *mut NlbOptions) {
    // SAFETY: as the caller promises.
    if let Some(o) = unsafe { o.as_mut() } {
        *o = NlbOptions {
            profile: NLB_PROFILE_PORTABLE,
            name: ptr::null(),
            checks: 1,
            minify: 0,
            case_tables: 1,
            part_limit: DEFAULT_PART_LIMIT,
            sourcemap: 0,
            localize: 1,
            decls: ptr::null(),
            ndecls: 0,
            check: None,
            check_data: ptr::null_mut(),
            runtime_include: ptr::null(),
            include_parts: ptr::null(),
        };
    }
}

/// Bundles `entries` (nlbundle.h). Returns NULL only when the result
/// cannot be built at all.
///
/// # Safety
///
/// `entries` points to `nentries` NUL-terminated strings; `options` is
/// NULL or points to an `nlb_Options` whose strings and arrays are valid;
/// `loader` is NULL or points to an `nlb_Loader` whose callbacks follow
/// its contract; all for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_bundle(entries: *const *const c_char, nentries: usize, options: *const NlbOptions, loader: *const NlcLoader) -> *mut NlbResult {
    // SAFETY: as the caller promises.
    unsafe { make(entries, nentries, options, loader, false) }
}

/// The runtime alone, for bundles made with `runtime_include` (nlbundle.h).
/// Returns NULL only when the result cannot be built at all.
///
/// # Safety
///
/// `options` is NULL or points to an `nlb_Options` whose strings and
/// arrays are valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_runtime(options: *const NlbOptions) -> *mut NlbResult {
    // SAFETY: no entries and no loader; `options` as the caller promises.
    unsafe { make(ptr::null(), 0, options, ptr::null(), true) }
}

/// `nlb_bundle`, or with `runtime_only` `nlb_runtime`.
///
/// # Safety
///
/// As for `nlb_bundle`.
unsafe fn make(entries: *const *const c_char, nentries: usize, options: *const NlbOptions, loader: *const NlcLoader, runtime_only: bool) -> *mut NlbResult {
    let body = AssertUnwindSafe(|| {
        // SAFETY: a non-NULL `loader` points to an nlb_Loader, copied.
        let raw = unsafe { loader.as_ref() }.copied().unwrap_or(NlcLoader { resolve: None, read: None, release: None, data: ptr::null_mut() });
        let cloader = CLoader::new(raw);
        let mut defaults = std::mem::MaybeUninit::<NlbOptions>::uninit();
        // SAFETY: `defaults` is a valid place for an NlbOptions; init
        // writes every field.
        let o = match unsafe { options.as_ref() } {
            Some(o) => *o,
            None => unsafe {
                nlb_options_init(defaults.as_mut_ptr());
                defaults.assume_init()
            },
        };
        let mut names = Vec::new();
        if !entries.is_null() {
            for i in 0..nentries {
                // SAFETY: `entries` holds `nentries` strings.
                if let Some(e) = unsafe { c_str(*entries.add(i)) } {
                    names.push(cloader.to_text(e.to_bytes()));
                }
            }
        }
        if !o.decls.is_null() {
            let mut decls = cloader.decls.borrow_mut();
            for i in 0..o.ndecls {
                // SAFETY: `decls` holds `ndecls` strings.
                if let Some(d) = unsafe { c_str(*o.decls.add(i)) } {
                    decls.push(cloader.to_text(d.to_bytes()));
                }
            }
        }
        // SAFETY: a non-NULL name is NUL-terminated.
        let name = unsafe { c_str(o.name) }.map_or_else(|| "bundle".to_owned(), |n| n.to_string_lossy().into_owned());
        // SAFETY: non-NULL paths are NUL-terminated.
        let runtime_include = unsafe { c_str(o.runtime_include) }.map(|p| p.to_string_lossy().into_owned());
        // SAFETY: as above.
        let include_parts = unsafe { c_str(o.include_parts) }.map(|p| p.to_string_lossy().into_owned());
        let check_fn = o.check;
        let check_data = o.check_data;
        let check = move |chunkname: &str, text: &str| -> Result<(), String> {
            let Some(f) = check_fn else { return Ok(()) };
            let name_c = c_string(chunkname);
            let mut err = vec![0u8; CHECK_ERROR_MAX];
            // SAFETY: the host's callback with the signature of nlb_Check;
            // `name_c` is NUL-terminated, `text` has `text.len()` bytes, and
            // `err` has `err.len()` writable bytes, all for the call.
            let ok = unsafe { f(check_data, name_c.as_ptr(), text.as_ptr().cast::<c_char>(), text.len(), err.as_mut_ptr().cast::<c_char>(), err.len()) };
            if ok != 0 {
                return Ok(());
            }
            let len = err.iter().position(|&b| b == 0).unwrap_or(err.len());
            Err(String::from_utf8_lossy(err.get(..len).unwrap_or_default()).into_owned())
        };
        let options = BundleOptions {
            compile: CompileOptions { checks: o.checks != 0, localize: Localize::from_level(i64::from(o.localize)).unwrap_or_default(), ..CompileOptions::default() },
            profile: if o.profile == NLB_PROFILE_GMOD_CLIENT { Profile::GmodClient } else { Profile::Portable },
            name: &name,
            minify: o.minify != 0,
            case_tables: o.case_tables != 0,
            part_limit: if o.part_limit == 0 { DEFAULT_PART_LIMIT } else { o.part_limit },
            sourcemap: o.sourcemap != 0,
            check: if check_fn.is_some() { Some(&check) } else { None },
            runtime_include: runtime_include.as_deref(),
            include_parts: include_parts.as_deref(),
        };
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let made = || if runtime_only { crate::bundle::runtime(&options) } else { crate::bundle::bundle(&refs, &options, &cloader) };
        let mut out = match catch_unwind(AssertUnwindSafe(made)) {
            Ok(out) => out,
            Err(payload) => crate::bundle::Bundle {
                diagnostics: vec![Diagnostic {
                    severity: Severity::Error,
                    code: "internal-error",
                    message: format!("internal compiler error: {}", panic_message(payload.as_ref())),
                    file: name.clone(),
                    line: 1,
                    column: 1,
                    end_line: 1,
                    end_column: 1,
                }],
                ..Default::default()
            },
        };
        // A declaration file the loader could not read: an error, under
        // that file's name, once.
        let mut missing = std::mem::take(&mut *cloader.missing.borrow_mut());
        missing.sort();
        missing.dedup();
        if !missing.is_empty() {
            out.parts.clear();
            out.sourcemap = None;
            let errors = missing.into_iter().map(|file| Diagnostic {
                severity: Severity::Error,
                code: "missing-declaration-file",
                message: "the declaration file cannot be read".to_owned(),
                file,
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 1,
            });
            out.diagnostics.splice(0..0, errors);
        }
        let (diagnostics, strings) = c_diagnostics(&out.diagnostics);
        Box::into_raw(Box::new(NlbResult {
            parts: out.parts.into_iter().map(|p| (c_string(&p.name), with_nul(p.text))).collect(),
            build: c_string(&out.build),
            diagnostics,
            _strings: strings,
            sourcemap: out.sourcemap.map(|m| Box::new(NlbSourceMap::new(m))),
        }))
    });
    catch_unwind(body).unwrap_or(ptr::null_mut())
}

/// # Safety
///
/// `r` is NULL or a result of `nlb_bundle` not yet freed.
unsafe fn result<'a>(r: *const NlbResult) -> Option<&'a NlbResult> {
    // SAFETY: as the caller promises.
    unsafe { r.as_ref() }
}

/// The number of files of the bundle: 0 when a diagnostic is an error.
///
/// # Safety
///
/// As for [`result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_nparts(r: *const NlbResult) -> usize {
    // SAFETY: as the caller promises.
    unsafe { result(r) }.map_or(0, |r| r.parts.len())
}

/// The text of file `i` (NUL-terminated; `*size` without the NUL), or NULL.
///
/// # Safety
///
/// As for [`result`]; `size` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_part(r: *const NlbResult, i: usize, size: *mut usize) -> *const c_char {
    // SAFETY: as the caller promises.
    let Some((_, text)) = (unsafe { result(r) }).and_then(|r| r.parts.get(i)) else { return ptr::null() };
    // SAFETY: a non-NULL `size` is writable.
    if let Some(s) = unsafe { size.as_mut() } {
        *s = text.len() - 1;
    }
    text.as_ptr().cast::<c_char>()
}

/// The file name of file `i` (`<name>.lua`, `<name>_2.lua`, …), or NULL.
///
/// # Safety
///
/// As for [`result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_part_name(r: *const NlbResult, i: usize) -> *const c_char {
    // SAFETY: as the caller promises.
    (unsafe { result(r) }).and_then(|r| r.parts.get(i)).map_or(ptr::null(), |(n, _)| n.as_ptr())
}

/// The bundle's build: a hash of its content ("" when it was not made).
///
/// # Safety
///
/// As for [`result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_build(r: *const NlbResult) -> *const c_char {
    // SAFETY: as the caller promises.
    (unsafe { result(r) }).map_or(c"".as_ptr(), |r| r.build.as_ptr())
}

/// # Safety
///
/// As for [`result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_ndiagnostics(r: *const NlbResult) -> usize {
    // SAFETY: as the caller promises.
    unsafe { result(r) }.map_or(0, |r| r.diagnostics.len())
}

/// # Safety
///
/// As for [`result`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_diagnostic(r: *const NlbResult, i: usize) -> *const NlcDiagnostic {
    // SAFETY: as the caller promises.
    (unsafe { result(r) }).and_then(|r| r.diagnostics.get(i)).map_or(ptr::null(), |d| d as *const NlcDiagnostic)
}

/// The bundle's sourcemap, now the caller's (`nlb_sourcemap_free`); NULL
/// without one, or when it was taken already.
///
/// # Safety
///
/// `r` is NULL or a result of `nlb_bundle` not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_take_sourcemap(r: *mut NlbResult) -> *mut NlbSourceMap {
    // SAFETY: as the caller promises.
    match unsafe { r.as_mut() }.and_then(|r| r.sourcemap.take()) {
        Some(m) => Box::into_raw(m),
        None => ptr::null_mut(),
    }
}

/// # Safety
///
/// `r` is NULL or a result of `nlb_bundle` not yet freed; it is not used
/// after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_free(r: *mut NlbResult) {
    if !r.is_null() {
        // SAFETY: `r` came from Box::into_raw in nlb_bundle.
        drop(unsafe { Box::from_raw(r) });
    }
}

/// Minifies Lua or GLua (nlbundle.h).
///
/// # Safety
///
/// `src` has `size` readable bytes (or is NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_minify(src: *const c_char, size: usize, flags: c_int) -> *mut NlbText {
    let body = AssertUnwindSafe(|| {
        // SAFETY: as the caller promises.
        let text = String::from_utf8_lossy(unsafe { bytes(src, size) });
        let options = MinifyOptions { glua: flags & NLB_MINIFY_GLUA != 0, escape_gmod_macro: flags & NLB_MINIFY_ESCAPE_MACRO != 0 };
        match crate::minify::minify(&text, options) {
            Ok(t) => NlbText::ok(t),
            Err(e) => NlbText::err(&e),
        }
    });
    catch_unwind(body).unwrap_or(ptr::null_mut())
}

/// The text (NUL-terminated; `*size` without the NUL), or NULL when the
/// operation failed.
///
/// # Safety
///
/// `t` is NULL or a text not yet freed; `size` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_text(t: *const NlbText, size: *mut usize) -> *const c_char {
    // SAFETY: as the caller promises.
    let Some(text) = (unsafe { t.as_ref() }).and_then(|t| t.text.as_ref()) else { return ptr::null() };
    // SAFETY: a non-NULL `size` is writable.
    if let Some(s) = unsafe { size.as_mut() } {
        *s = text.len() - 1;
    }
    text.as_ptr().cast::<c_char>()
}

/// Why the operation failed, or NULL when it did not.
///
/// # Safety
///
/// `t` is NULL or a text not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_text_error(t: *const NlbText) -> *const c_char {
    // SAFETY: as the caller promises.
    (unsafe { t.as_ref() }).and_then(|t| t.error.as_ref()).map_or(ptr::null(), |e| e.as_ptr())
}

/// # Safety
///
/// `t` is NULL or a text not yet freed; it is not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_text_free(t: *mut NlbText) {
    if !t.is_null() {
        // SAFETY: `t` came from Box::into_raw.
        drop(unsafe { Box::from_raw(t) });
    }
}

/// The size of `data` compressed as gmod compresses the files it sends to
/// clients (LZMA), which it holds to 64 KiB.
///
/// # Safety
///
/// `data` has `size` readable bytes (or is NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_compressed_size(data: *const c_char, size: usize) -> usize {
    // SAFETY: as the caller promises.
    let bytes = unsafe { bytes(data, size) };
    catch_unwind(|| crate::bundle::lzma_size(bytes)).unwrap_or(usize::MAX)
}

/// A sourcemap from its JSON (`nlb_sourcemap_json`, `nlc bundle --map`), or
/// NULL with the reason in `err[errsize]`.
///
/// # Safety
///
/// `json` has `size` readable bytes (or is NULL); `err` is NULL or has
/// `errsize` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_sourcemap_parse(json: *const c_char, size: usize, err: *mut c_char, errsize: usize) -> *mut NlbSourceMap {
    let body = AssertUnwindSafe(|| {
        // SAFETY: as the caller promises.
        let text = String::from_utf8_lossy(unsafe { bytes(json, size) });
        match SourceMap::from_json(&text) {
            Ok(m) => Box::into_raw(Box::new(NlbSourceMap::new(m))),
            Err(e) => {
                // SAFETY: as the caller promises.
                unsafe { write_error(err, errsize, &e) };
                ptr::null_mut()
            }
        }
    });
    catch_unwind(body).unwrap_or(ptr::null_mut())
}

/// # Safety
///
/// `m` is NULL or a sourcemap not yet freed.
unsafe fn map<'a>(m: *const NlbSourceMap) -> Option<&'a NlbSourceMap> {
    // SAFETY: as the caller promises.
    unsafe { m.as_ref() }
}

/// The map as JSON, to keep it.
///
/// # Safety
///
/// As for [`map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_sourcemap_json(m: *const NlbSourceMap) -> *mut NlbText {
    // SAFETY: as the caller promises.
    let Some(m) = (unsafe { map(m) }) else { return ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| NlbText::ok(m.map.to_json()))).unwrap_or(ptr::null_mut())
}

/// # Safety
///
/// As for [`map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_sourcemap_bundle(m: *const NlbSourceMap) -> *const c_char {
    // SAFETY: as the caller promises.
    (unsafe { map(m) }).map_or(c"".as_ptr(), |m| m.bundle.as_ptr())
}

/// # Safety
///
/// As for [`map`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_sourcemap_build(m: *const NlbSourceMap) -> *const c_char {
    // SAFETY: as the caller promises.
    (unsafe { map(m) }).map_or(c"".as_ptr(), |m| m.build.as_ptr())
}

/// The source of a position as LuaJIT reports it (`short_src`, a line):
/// 1 with `*file` and `*srcline` set, or 0 when the map has none.
///
/// # Safety
///
/// As for [`map`]; `short_src` is NULL or NUL-terminated; `file` and
/// `srcline` are NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_sourcemap_lookup(m: *const NlbSourceMap, short_src: *const c_char, line: c_int, file: *mut *const c_char, srcline: *mut c_int) -> c_int {
    // SAFETY: as the caller promises.
    let (Some(m), Some(src)) = (unsafe { map(m) }, unsafe { c_str(short_src) }) else { return 0 };
    let Ok(line) = u32::try_from(line) else { return 0 };
    let found = catch_unwind(AssertUnwindSafe(|| {
        let src = src.to_string_lossy();
        let (f, l) = m.map.lookup_short_src(&src, line)?;
        let i = m.map.files.iter().position(|x| x == f)?;
        Some((i, l))
    }));
    let Ok(Some((i, l))) = found else { return 0 };
    let Some(f) = m.files.get(i) else { return 0 };
    // SAFETY: non-NULL out pointers are writable.
    unsafe {
        if let Some(out) = file.as_mut() {
            *out = f.as_ptr();
        }
        if let Some(out) = srcline.as_mut() {
            *out = c_int::try_from(l).unwrap_or(c_int::MAX);
        }
    }
    1
}

/// `text` with every `chunk:line` of a chunk of `maps` replaced by its
/// source file and line.
///
/// # Safety
///
/// `maps` holds `nmaps` sourcemaps (or NULLs) not yet freed; `text` has
/// `size` readable bytes (or is NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_remap(maps: *const *const NlbSourceMap, nmaps: usize, text: *const c_char, size: usize) -> *mut NlbText {
    let body = AssertUnwindSafe(|| {
        let mut list = Vec::new();
        if !maps.is_null() {
            for i in 0..nmaps {
                // SAFETY: `maps` holds `nmaps` pointers, each NULL or a map.
                if let Some(m) = unsafe { map(*maps.add(i)) } {
                    list.push(&m.map);
                }
            }
        }
        // SAFETY: as the caller promises.
        let text = String::from_utf8_lossy(unsafe { bytes(text, size) });
        NlbText::ok(crate::sourcemap::remap_text(&list, &text))
    });
    catch_unwind(body).unwrap_or(ptr::null_mut())
}

/// # Safety
///
/// `m` is NULL or a sourcemap of `nlb_sourcemap_parse` or
/// `nlb_take_sourcemap` not yet freed; it is not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlb_sourcemap_free(m: *mut NlbSourceMap) {
    if !m.is_null() {
        // SAFETY: `m` came from Box::into_raw.
        drop(unsafe { Box::from_raw(m) });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An in-memory file table behind a C loader: `./x` in a file is
    /// relative to its folder; `.nl` may be left out.
    struct Files(HashMap<String, Vec<u8>>);

    unsafe extern "C" fn t_resolve(data: *mut c_void, from: *const c_char, path: *const c_char, out: *mut c_char, size: usize) -> c_int {
        // SAFETY: tests pass a `Files` and valid strings.
        let files = unsafe { &*(data as *const Files) };
        let path = unsafe { CStr::from_ptr(path) }.to_str().unwrap();
        let from = (!from.is_null()).then(|| unsafe { CStr::from_ptr(from) }.to_str().unwrap().to_owned());
        let base = match (path.strip_prefix("./"), from) {
            (Some(rest), Some(f)) => match f.rsplit_once('/') {
                Some((dir, _)) => format!("{dir}/{rest}"),
                None => rest.to_owned(),
            },
            _ => path.to_owned(),
        };
        let Some(name) = [base.clone(), format!("{base}.nl")].into_iter().find(|n| files.0.contains_key(n)) else { return 0 };
        if name.len() + 1 > size {
            return 0;
        }
        // SAFETY: `out` has `size` bytes and the name fits.
        unsafe {
            ptr::copy_nonoverlapping(name.as_ptr(), out.cast::<u8>(), name.len());
            *out.add(name.len()) = 0;
        }
        1
    }

    unsafe extern "C" fn t_read(data: *mut c_void, name: *const c_char, size: *mut usize) -> *const c_char {
        // SAFETY: tests pass a `Files`.
        let files = unsafe { &*(data as *const Files) };
        let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();
        match files.0.get(name) {
            Some(b) => {
                unsafe { *size = b.len() };
                b.as_ptr().cast::<c_char>()
            }
            None => ptr::null(),
        }
    }

    fn loader(f: &Files) -> NlcLoader {
        NlcLoader { resolve: Some(t_resolve), read: Some(t_read), release: None, data: (f as *const Files).cast_mut().cast::<c_void>() }
    }

    fn files(entries: &[(&str, &str)]) -> Files {
        Files(entries.iter().map(|(k, v)| ((*k).to_owned(), v.as_bytes().to_vec())).collect())
    }

    /// Counts its calls; fails a chunk whose text holds "FAIL_TO_LOAD".
    unsafe extern "C" fn t_check(data: *mut c_void, chunkname: *const c_char, text: *const c_char, size: usize, err: *mut c_char, errsize: usize) -> c_int {
        // SAFETY: tests pass a usize counter and valid buffers.
        unsafe { *(data as *mut usize) += 1 };
        let text = unsafe { std::slice::from_raw_parts(text.cast::<u8>(), size) };
        if !String::from_utf8_lossy(text).contains("FAIL_TO_LOAD") {
            return 1;
        }
        let name = unsafe { CStr::from_ptr(chunkname) }.to_str().unwrap();
        let msg = format!("{}:3: function at line 1 has more than 200 local variables", name.trim_start_matches('@'));
        unsafe { write_error(err, errsize, &msg) };
        0
    }

    fn cstr(p: *const c_char) -> String {
        assert!(!p.is_null());
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }

    /// (code, file, line) of each diagnostic of a result.
    fn diagnostics(r: *const NlbResult) -> Vec<(String, String, c_int)> {
        unsafe { (0..nlb_ndiagnostics(r)).map(|i| &*nlb_diagnostic(r, i)).map(|d| (cstr(d.code), cstr(d.file), d.line)).collect() }
    }

    fn options(profile: c_int, name: &CStr) -> NlbOptions {
        let mut o = std::mem::MaybeUninit::<NlbOptions>::uninit();
        unsafe {
            nlb_options_init(o.as_mut_ptr());
            let mut o = o.assume_init();
            o.profile = profile;
            o.name = name.as_ptr();
            o
        }
    }

    const MAIN: &str = "import { greet } from \"./greet\"\nif SERVER\n  IO.write(\"SERVER_ONLY\")\nend\nprint greet(\"client\")\n";
    const GREET: &str = "pub fn greet(n: String): String\n  \"hi #{n}\"\nend\n";

    #[test]
    fn a_gmod_bundle_and_its_sourcemap() {
        let f = files(&[("addon/main.nl", MAIN), ("addon/greet.nl", GREET)]);
        let ld = loader(&f);
        let mut checked = 0usize;
        let mut o = options(NLB_PROFILE_GMOD_CLIENT, c"client");
        o.sourcemap = 1;
        o.check = Some(t_check);
        o.check_data = (&mut checked as *mut usize).cast::<c_void>();
        let entries = [c"addon/main.nl".as_ptr()];
        unsafe {
            let r = nlb_bundle(entries.as_ptr(), 1, &o, &ld);
            assert!(!r.is_null());
            assert!(diagnostics(r).is_empty(), "{:?}", diagnostics(r));
            assert_eq!(checked, 2);
            assert_eq!(nlb_nparts(r), 1);
            assert_eq!(cstr(nlb_part_name(r, 0)), "client.lua");
            let mut size = 0usize;
            let text = cstr(nlb_part(r, 0, &mut size));
            assert_eq!(text.len(), size);
            assert!(text.starts_with(crate::bundle::MARK) && text.contains("if SERVER then AddCSLuaFile() return end"), "{}", &text[..200]);
            assert!(!text.contains("SERVER_ONLY"));
            assert!(nlb_part(r, 1, ptr::null_mut()).is_null() && nlb_part_name(r, 1).is_null());
            let build = cstr(nlb_build(r));
            assert_eq!(build.len(), 16);

            let m = nlb_take_sourcemap(r);
            assert!(!m.is_null() && nlb_take_sourcemap(r).is_null());
            nlb_free(r);
            // The map outlives the result.
            assert_eq!(cstr(nlb_sourcemap_bundle(m)), "client");
            assert_eq!(cstr(nlb_sourcemap_build(m)), build);
            let (mut file, mut line) = (ptr::null::<c_char>(), 0 as c_int);
            assert_eq!(nlb_sourcemap_lookup(m, c"addon/greet.nl".as_ptr(), 2, &mut file, &mut line), 1);
            assert_eq!((cstr(file), line), ("addon/greet.nl".to_owned(), 2));
            assert_eq!(nlb_sourcemap_lookup(m, c"nl runtime".as_ptr(), 1, &mut file, &mut line), 1);
            assert_eq!((cstr(file), line), ("runtime/nl.lua".to_owned(), 1));
            assert_eq!(nlb_sourcemap_lookup(m, c"...eet.nl".as_ptr(), 1, &mut file, &mut line), 1);
            assert_eq!(nlb_sourcemap_lookup(m, c"other.nl".as_ptr(), 1, &mut file, &mut line), 0);
            assert_eq!(nlb_sourcemap_lookup(m, c"addon/greet.nl".as_ptr(), -1, &mut file, &mut line), 0);

            // Through JSON and back.
            let json = nlb_sourcemap_json(m);
            let mut n = 0usize;
            let j = nlb_text(json, &mut n);
            let m2 = nlb_sourcemap_parse(j, n, ptr::null_mut(), 0);
            assert!(!m2.is_null());
            nlb_text_free(json);
            let maps = [m.cast_const(), ptr::null(), m2.cast_const()];
            let msg = "nl runtime:1: x\n\t...eet.nl:2: in function 'greet'";
            let t = nlb_remap(maps.as_ptr(), maps.len(), msg.as_ptr().cast(), msg.len());
            assert_eq!(cstr(nlb_text(t, ptr::null_mut())), "runtime/nl.lua:1: x\n\taddon/greet.nl:2: in function 'greet'");
            assert!(nlb_text_error(t).is_null());
            nlb_text_free(t);
            nlb_sourcemap_free(m);
            nlb_sourcemap_free(m2);
        }
    }

    #[test]
    fn errors_are_diagnostics() {
        let f = files(&[
            ("realm.nl", ":util.AddNetworkString(\"x\")\n"),
            ("big.nl", "let s = \"FAIL_TO_LOAD\"\nprint s\n"),
        ]);
        let ld = loader(&f);
        let mut checked = 0usize;
        let mut o = options(NLB_PROFILE_GMOD_CLIENT, c"c");
        o.check = Some(t_check);
        o.check_data = (&mut checked as *mut usize).cast::<c_void>();
        let run = |entry: &CStr, o: &NlbOptions| unsafe {
            let r = nlb_bundle([entry.as_ptr()].as_ptr(), 1, o, &ld);
            assert_eq!(nlb_nparts(r), 0);
            assert!(nlb_take_sourcemap(r).is_null());
            let d = diagnostics(r);
            nlb_free(r);
            d
        };
        assert_eq!(run(c"realm.nl", &o), [("wrong-realm".to_owned(), "realm.nl".to_owned(), 1)]);
        assert_eq!(run(c"big.nl", &o), [("lua-limit".to_owned(), "big.nl".to_owned(), 3)]);
        assert_eq!(run(c"nowhere.nl", &o), [("cannot-read".to_owned(), "nowhere.nl".to_owned(), 1)]);
        let decls = [c"missing.d.nl".as_ptr()];
        let mut with_decl = o;
        with_decl.decls = decls.as_ptr();
        with_decl.ndecls = 1;
        let d = run(c"realm.nl", &with_decl);
        assert_eq!(d[0], ("missing-declaration-file".to_owned(), "missing.d.nl".to_owned(), 1), "{d:?}");
        let mut bad_name = o;
        bad_name.name = c"a/b".as_ptr();
        assert_eq!(run(c"realm.nl", &bad_name)[0].0, "bundle-name");
    }

    #[test]
    fn defaults_and_null_arguments() {
        let f = files(&[("a.nl", "print \"hi\"\n")]);
        let ld = loader(&f);
        unsafe {
            let r = nlb_bundle([c"a.nl".as_ptr()].as_ptr(), 1, ptr::null(), &ld);
            assert_eq!((nlb_nparts(r), cstr(nlb_part_name(r, 0))), (1, "bundle.lua".to_owned()));
            assert!(cstr(nlb_part(r, 0, ptr::null_mut())).starts_with("-- Built by `nlc bundle`, portable profile"));
            nlb_free(r);
            let r = nlb_bundle(ptr::null(), 0, ptr::null(), ptr::null());
            assert_eq!((nlb_nparts(r), diagnostics(r)[0].0.as_str()), (0, "no-entry"));
            nlb_free(r);
            assert_eq!(nlb_nparts(ptr::null()), 0);
            assert!(nlb_part(ptr::null(), 0, ptr::null_mut()).is_null());
            assert_eq!(cstr(nlb_build(ptr::null())), "");
            nlb_free(ptr::null_mut());
            nlb_text_free(ptr::null_mut());
            nlb_sourcemap_free(ptr::null_mut());
            assert_eq!(nlb_abi_version(), NLB_ABI_VERSION);
        }
    }

    #[test]
    fn minify_and_measure() {
        unsafe {
            let src = "local a = 1 // c\nprint('DEFINE_BASECLASS') /* x */";
            let t = nlb_minify(src.as_ptr().cast(), src.len(), NLB_MINIFY_GLUA);
            assert_eq!(cstr(nlb_text(t, ptr::null_mut())), "local a=1\nprint('DEFINE_BASECLASS')");
            nlb_text_free(t);
            let t = nlb_minify(src.as_ptr().cast(), src.len(), NLB_MINIFY_GLUA | NLB_MINIFY_ESCAPE_MACRO);
            assert_eq!(cstr(nlb_text(t, ptr::null_mut())), "local a=1\nprint('DEFINE_\\066ASECLASS')");
            nlb_text_free(t);
            let bad = "x = 'open";
            let t = nlb_minify(bad.as_ptr().cast(), bad.len(), 0);
            assert!(nlb_text(t, ptr::null_mut()).is_null());
            assert!(cstr(nlb_text_error(t)).contains("unfinished string"));
            nlb_text_free(t);
            let t = nlb_minify(ptr::null(), 0, 0);
            assert_eq!(cstr(nlb_text(t, ptr::null_mut())), "");
            nlb_text_free(t);
            let data = "local x = 1\n".repeat(1000);
            let n = nlb_compressed_size(data.as_ptr().cast(), data.len());
            assert!(n > 13 && n < 500, "{n}");
            let mut err = [0 as c_char; 64];
            assert!(nlb_sourcemap_parse(c"{}".as_ptr(), 2, err.as_mut_ptr(), err.len()).is_null());
            assert!(cstr(err.as_ptr()).contains("not a sourcemap"));
            // A message longer than the buffer is cut, NUL-terminated.
            let mut tiny = [1 as c_char; 4];
            assert!(nlb_sourcemap_parse(c"x".as_ptr(), 1, tiny.as_mut_ptr(), tiny.len()).is_null());
            assert_eq!(cstr(tiny.as_ptr()).len(), 3);
        }
    }
}
