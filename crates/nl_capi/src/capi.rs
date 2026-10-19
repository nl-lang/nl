//! The C ABI of `nlc.h`: what the VM fork links as `libnlc`.
//!
//! `nlc.h` lives with the VM (`LuaJIT/src/nlc.h`); the `#[repr(C)]` types
//! here must match it. It is internal to the VM and not part of the host API.
//!
//! The boundary rules:
//!
//! - The compiler never enters the VM. It reads imported files through the
//!   C loader it is given, whose callbacks are plain C functions.
//! - No Rust panic crosses into C. Every exported function runs its body
//!   under `catch_unwind`; a panic in the compiler becomes a diagnostic with
//!   the code `"internal-error"`.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use crate::driver::{self, CompileOptions, CompileOutput, Diagnostic, Loader, Severity};

/// `nlc_Loader` (same layout as `nl_Loader` in `nl.h`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NlcLoader {
    pub resolve: Option<
        unsafe extern "C" fn(
            data: *mut c_void,
            from: *const c_char,
            path: *const c_char,
            out: *mut c_char,
            size: usize,
        ) -> c_int,
    >,
    pub read: Option<
        unsafe extern "C" fn(data: *mut c_void, name: *const c_char, size: *mut usize) -> *const c_char,
    >,
    pub release:
        Option<unsafe extern "C" fn(data: *mut c_void, name: *const c_char, buf: *const c_char)>,
    pub data: *mut c_void,
}

/// `nlc_Options`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NlcOptions {
    pub checks: c_int,
    pub strip: c_int,
    /// 1: a bytecode dump ([`driver::Emit::Bytecode`]); 0: Lua source.
    pub bytecode: c_int,
    /// The feature level of a dump ([`CompileOptions::level`]): 0 for
    /// stock LuaJIT, 1 for the fork.
    pub level: c_int,
    /// The declaration files in scope: `ndecls`
    /// names, read through the loader's `read` ([`Loader::declarations`]).
    pub decls: *const *const c_char,
    pub ndecls: usize,
    /// `NL_OPT_LOCALIZE` ([`CompileOptions::localize`]): 0 none, 1 the
    /// standard libraries, 2 every Lua global; another value is 1.
    pub localize: c_int,
}

/// `nlc_Diagnostic` (same layout as `nl_Diagnostic` in `nl.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NlcDiagnostic {
    pub severity: c_int,
    pub code: *const c_char,
    pub message: *const c_char,
    pub file: *const c_char,
    pub line: c_int,
    pub column: c_int,
    pub endline: c_int,
    pub endcolumn: c_int,
}

pub const NLC_DIAG_ERROR: c_int = 0;
pub const NLC_DIAG_WARNING: c_int = 1;
pub const NLC_DIAG_LINT: c_int = 2;

/// The size of the buffer a loader's `resolve` writes the canonical name to.
pub(crate) const NAME_MAX: usize = 4096;

/// `nlc_Result`: opaque to C.
pub struct NlcResult {
    chunk: Option<Vec<u8>>,
    diagnostics: Vec<NlcDiagnostic>,
    /// Owns the strings that `diagnostics` points into.
    _strings: Vec<CString>,
}

impl NlcResult {
    fn from_output(out: CompileOutput) -> NlcResult {
        let (diagnostics, strings) = c_diagnostics(&out.diagnostics);
        NlcResult { chunk: out.chunk, diagnostics, _strings: strings }
    }
}

/// Diagnostics for C, with the strings they point into; the pointers stay
/// valid while the strings are kept.
pub(crate) fn c_diagnostics(diags: &[Diagnostic]) -> (Vec<NlcDiagnostic>, Vec<CString>) {
    let mut strings = Vec::with_capacity(diags.len() * 3);
    let mut diagnostics = Vec::with_capacity(diags.len());
    for d in diags {
        let code = c_string(d.code);
        let message = c_string(&d.message);
        let file = c_string(&d.file);
        // The pointers stay valid when the CStrings move into `strings`:
        // a CString owns a heap buffer that moving does not relocate.
        diagnostics.push(NlcDiagnostic {
            severity: match d.severity {
                Severity::Error => NLC_DIAG_ERROR,
                Severity::Warning => NLC_DIAG_WARNING,
                Severity::Lint => NLC_DIAG_LINT,
            },
            code: code.as_ptr(),
            message: message.as_ptr(),
            file: file.as_ptr(),
            line: to_c_int(d.line),
            column: to_c_int(d.column),
            endline: to_c_int(d.end_line),
            endcolumn: to_c_int(d.end_column),
        });
        strings.push(code);
        strings.push(message);
        strings.push(file);
    }
    (diagnostics, strings)
}

fn to_c_int(v: u32) -> c_int {
    c_int::try_from(v).unwrap_or(c_int::MAX)
}

/// A C string for any Rust string: interior NULs become U+FFFD.
pub(crate) fn c_string(s: &str) -> CString {
    CString::new(s).unwrap_or_else(|_| {
        let cleaned = s.replace('\0', "\u{FFFD}");
        CString::new(cleaned).unwrap_or_default()
    })
}

pub(crate) fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_owned()
    }
}

fn file_of(chunk_name: &str) -> String {
    chunk_name.strip_prefix(['@', '=']).unwrap_or(chunk_name).to_owned()
}

/// The result for a compile that panicked.
fn internal_error(chunk_name: &str, payload: &(dyn Any + Send)) -> CompileOutput {
    CompileOutput {
        chunk: None,
        diagnostics: vec![Diagnostic {
            severity: Severity::Error,
            code: "internal-error",
            message: format!("internal compiler error: {}", panic_message(payload)),
            file: file_of(chunk_name),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 1,
        }],
    }
}

/// A [`Loader`] over the C callbacks of an `nlc_Loader`.
///
/// Canonical names are bytes (the C API does not restrict them to UTF-8),
/// while [`Loader`] works in `String`. A name that is not UTF-8 is given to
/// the compiler as a text that stands for it, and `names` maps that text
/// back, so the loader always gets back exactly the bytes it returned.
pub(crate) struct CLoader {
    raw: NlcLoader,
    names: RefCell<HashMap<String, Vec<u8>>>,
    /// The declaration files (`nlc_Options.decls`), as text names.
    pub(crate) decls: RefCell<Vec<String>>,
    /// The declaration files `read` returned nothing for.
    pub(crate) missing: RefCell<Vec<String>>,
}

impl CLoader {
    pub(crate) fn new(raw: NlcLoader) -> CLoader {
        CLoader { raw, names: RefCell::new(HashMap::new()), decls: RefCell::new(Vec::new()), missing: RefCell::new(Vec::new()) }
    }

    /// The text that stands for a name given as bytes.
    pub(crate) fn to_text(&self, bytes: &[u8]) -> String {
        if let Ok(s) = std::str::from_utf8(bytes) {
            return s.to_owned();
        }
        let mut names = self.names.borrow_mut();
        if let Some((k, _)) = names.iter().find(|(_, v)| v.as_slice() == bytes) {
            return k.clone();
        }
        // The lossy text, made unique among the names of this compile.
        let lossy = String::from_utf8_lossy(bytes).into_owned();
        let mut text = lossy.clone();
        let mut n = 1u32;
        while names.contains_key(&text) {
            n += 1;
            text = format!("{lossy}\u{FFFD}{n}");
        }
        names.insert(text.clone(), bytes.to_vec());
        text
    }

    /// The bytes a text stands for.
    fn to_bytes(&self, text: &str) -> Vec<u8> {
        self.names.borrow().get(text).cloned().unwrap_or_else(|| text.as_bytes().to_vec())
    }

    fn c_name(&self, text: &str) -> Option<CString> {
        CString::new(self.to_bytes(text)).ok()
    }
}

impl Loader for CLoader {
    fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
        let resolve = self.raw.resolve?;
        let from_c = match from {
            Some(f) => Some(self.c_name(f)?),
            None => None,
        };
        let path_c = CString::new(path).ok()?;
        let mut out = vec![0u8; NAME_MAX];
        // SAFETY: `resolve` is the host's (or the VM's) callback with the
        // signature of nlc.h. Every pointer passed is valid for the call:
        // `from_c` and `path_c` are NUL-terminated and outlive it, and `out`
        // has `out.len()` writable bytes.
        let found = unsafe {
            resolve(
                self.raw.data,
                from_c.as_ref().map_or(ptr::null(), |f| f.as_ptr()),
                path_c.as_ptr(),
                out.as_mut_ptr().cast::<c_char>(),
                out.len(),
            )
        };
        if found == 0 {
            return None;
        }
        // The callback may fill the whole buffer: stop at the first NUL or
        // at its end.
        let len = out.iter().position(|&b| b == 0).unwrap_or(out.len());
        out.truncate(len);
        Some(self.to_text(&out))
    }

    fn declarations(&self) -> Vec<(String, Vec<u8>)> {
        let names = self.decls.borrow().clone();
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            match self.read(&name) {
                Some(bytes) => out.push((name, bytes)),
                None => self.missing.borrow_mut().push(name),
            }
        }
        out
    }

    fn read(&self, name: &str) -> Option<Vec<u8>> {
        let read = self.raw.read?;
        let name_c = self.c_name(name)?;
        let mut size: usize = 0;
        // SAFETY: as for `resolve`; `size` is a valid out pointer.
        let buf = unsafe { read(self.raw.data, name_c.as_ptr(), &mut size) };
        if buf.is_null() {
            return None;
        }
        let bytes = if size == 0 {
            Vec::new()
        } else {
            // SAFETY: the loader contract (nlc.h) is that a non-NULL `buf`
            // holds `size` readable bytes until `release` is called.
            unsafe { std::slice::from_raw_parts(buf.cast::<u8>(), size) }.to_vec()
        };
        if let Some(release) = self.raw.release {
            // SAFETY: one release per successful read, with the same name and
            // buffer, after the last use of the buffer.
            unsafe { release(self.raw.data, name_c.as_ptr(), buf) };
        }
        Some(bytes)
    }
}

/// The signature of [`driver::compile`]; tests substitute their own.
type CompileFn = fn(&[u8], &str, &CompileOptions, &dyn Loader) -> CompileOutput;

/// The body of `nl_compile` over safe values, with the compile function as a
/// parameter. Never panics.
fn compile_caught(
    compile: CompileFn,
    source: &[u8],
    chunk_name: &str,
    options: &CompileOptions,
    loader: &dyn Loader,
) -> CompileOutput {
    match catch_unwind(AssertUnwindSafe(|| compile(source, chunk_name, options, loader))) {
        Ok(out) => out,
        Err(payload) => {
            // Formatting the message could itself panic in theory (a payload
            // whose Display panics is impossible here: only &str and String
            // are read), so the fallback is guarded too.
            catch_unwind(AssertUnwindSafe(|| internal_error(chunk_name, payload.as_ref())))
                .unwrap_or_default()
        }
    }
}

/// `nl_compile` with an explicit compile function.
///
/// # Safety
///
/// As for [`nl_compile`].
unsafe fn nl_compile_with(
    compile: CompileFn,
    cached: bool,
    source: *const c_char,
    size: usize,
    chunkname: *const c_char,
    options: *const NlcOptions,
    loader: *const NlcLoader,
) -> *mut NlcResult {
    let body = AssertUnwindSafe(|| {
        let src: &[u8] = if source.is_null() || size == 0 {
            &[]
        } else {
            // SAFETY: the caller passes `size` readable bytes at `source`
            // (nlc.h), alive for the duration of the call.
            unsafe { std::slice::from_raw_parts(source.cast::<u8>(), size) }
        };
        // SAFETY: a non-NULL `loader` points to an nlc_Loader (nlc.h). It
        // is copied, so the compile does not depend on the caller's struct.
        let raw = unsafe { loader.as_ref() }.copied().unwrap_or(NlcLoader {
            resolve: None,
            read: None,
            release: None,
            data: ptr::null_mut(),
        });
        let cloader = CLoader::new(raw);
        let name = if chunkname.is_null() {
            "=?".to_owned()
        } else {
            // SAFETY: a non-NULL `chunkname` is a NUL-terminated string
            // (nlc.h), alive for the duration of the call.
            let bytes = unsafe { CStr::from_ptr(chunkname) }.to_bytes();
            // "@name": the name is a canonical name, so it maps back to its
            // bytes when the compiler passes it to resolve as `from`.
            match bytes.split_first() {
                Some((b'@', rest)) => format!("@{}", cloader.to_text(rest)),
                _ => String::from_utf8_lossy(bytes).into_owned(),
            }
        };
        // SAFETY: a non-NULL `options` points to an nlc_Options (nlc.h).
        // The analyzer and static resolution are always on from C: the C
        // API has no option for them.
        let opts = match unsafe { options.as_ref() } {
            Some(o) => {
                if !o.decls.is_null() {
                    let mut decls = cloader.decls.borrow_mut();
                    for i in 0..o.ndecls {
                        // SAFETY: `decls` points to `ndecls` NUL-terminated
                        // names (nlc.h), alive for the duration of the call.
                        let bytes = unsafe { CStr::from_ptr(*o.decls.add(i)) }.to_bytes();
                        decls.push(cloader.to_text(bytes));
                    }
                }
                CompileOptions {
                    checks: o.checks != 0,
                    strip: o.strip != 0,
                    emit: if o.bytecode != 0 { driver::Emit::Bytecode } else { driver::Emit::Lua },
                    level: if o.level >= 1 { 1 } else { 0 },
                    localize: driver::Localize::from_level(i64::from(o.localize)).unwrap_or_default(),
                    ..CompileOptions::default()
                }
            }
            None => CompileOptions::default(),
        };
        // The per-process cache (cache.rs): every state gets the same
        // bytecode for a file. A compile that panicked is not kept.
        let mut out = if cached {
            let decls = cloader.decls.borrow().clone();
            crate::cache::compile_cached(
                crate::cache::Key::new(&name, src, &opts, &decls),
                &cloader,
                || cloader.missing.borrow_mut().clear(),
                |l| compile_caught(compile, src, &name, &opts, l),
                |out| !out.diagnostics.iter().any(|d| d.code == "internal-error"),
            )
        } else {
            compile_caught(compile, src, &name, &opts, &cloader)
        };
        // A declaration file the loader could not read (nlc.h): an error
        // of the compile, reported under that file's name.
        let missing = std::mem::take(&mut *cloader.missing.borrow_mut());
        if !missing.is_empty() {
            out.chunk = None;
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
        Box::into_raw(Box::new(NlcResult::from_output(out)))
    });
    catch_unwind(body).unwrap_or(ptr::null_mut())
}

/// Compiles one NL source file (nlc.h). Returns NULL only when the result
/// cannot be built at all.
///
/// # Safety
///
/// `source` holds `size` readable bytes (or `size` is 0); `chunkname` is NULL
/// or NUL-terminated; `options` and `loader` are NULL or valid; the loader's
/// callbacks follow nlc.h and do not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nl_compile(
    source: *const c_char,
    size: usize,
    chunkname: *const c_char,
    options: *const NlcOptions,
    loader: *const NlcLoader,
) -> *mut NlcResult {
    // SAFETY: the caller's guarantees are those of nl_compile_with.
    unsafe { nl_compile_with(driver::compile, true, source, size, chunkname, options, loader) }
}

/// The chunk, or NULL when the compile failed (nlc.h).
///
/// # Safety
///
/// `r` is NULL or a result of `nl_compile` not yet freed; `size` is NULL or
/// a valid out pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlc_chunk(r: *const NlcResult, size: *mut usize) -> *const c_char {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `r` is NULL or a live result (the caller's guarantee).
        let chunk = unsafe { r.as_ref() }.and_then(|r| r.chunk.as_ref());
        if !size.is_null() {
            // SAFETY: a non-NULL `size` is a valid out pointer.
            unsafe { *size = chunk.map_or(0, Vec::len) };
        }
        chunk.map_or(ptr::null(), |c| c.as_ptr().cast::<c_char>())
    }))
    .unwrap_or(ptr::null())
}

/// The number of diagnostics (nlc.h).
///
/// # Safety
///
/// `r` is NULL or a result of `nl_compile` not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlc_ndiagnostics(r: *const NlcResult) -> usize {
    // SAFETY: `r` is NULL or a live result (the caller's guarantee).
    catch_unwind(AssertUnwindSafe(|| unsafe { r.as_ref() }.map_or(0, |r| r.diagnostics.len())))
        .unwrap_or(0)
}

/// Diagnostic `i`, or NULL past the end (nlc.h).
///
/// # Safety
///
/// `r` is NULL or a result of `nl_compile` not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlc_diagnostic(r: *const NlcResult, i: usize) -> *const NlcDiagnostic {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `r` is NULL or a live result (the caller's guarantee).
        unsafe { r.as_ref() }
            .and_then(|r| r.diagnostics.get(i))
            .map_or(ptr::null(), |d| d as *const NlcDiagnostic)
    }))
    .unwrap_or(ptr::null())
}

/// Frees a result (nlc.h).
///
/// # Safety
///
/// `r` is NULL or a result of `nl_compile` not yet freed; it is not used
/// afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nlc_free(r: *mut NlcResult) {
    if r.is_null() {
        return;
    }
    // SAFETY: `r` came from Box::into_raw in nl_compile_with and is freed
    // once (the caller's guarantee).
    let boxed = unsafe { Box::from_raw(r) };
    // Dropping plain vectors and strings does not panic; the guard keeps the
    // rule that nothing unwinds into C.
    let _ = catch_unwind(AssertUnwindSafe(move || drop(boxed)));
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A C loader over an in-memory file table, counting reads and releases.
    struct Files {
        files: HashMap<String, Vec<u8>>,
        reads: usize,
        releases: usize,
        last_from: RefCell<Option<String>>,
    }

    unsafe extern "C" fn t_resolve(
        data: *mut c_void,
        from: *const c_char,
        path: *const c_char,
        out: *mut c_char,
        size: usize,
    ) -> c_int {
        // SAFETY: tests pass a `Files` as data and valid strings.
        let files = unsafe { &*(data as *const Files) };
        let path = unsafe { CStr::from_ptr(path) }.to_str().unwrap();
        let from = (!from.is_null()).then(|| unsafe { CStr::from_ptr(from) }.to_str().unwrap().to_owned());
        *files.last_from.borrow_mut() = from;
        let name = format!("/{path}.nl");
        if !files.files.contains_key(&name) || name.len() + 1 > size {
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
        // SAFETY: tests pass a `Files` as data.
        let files = unsafe { &mut *(data as *mut Files) };
        let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();
        match files.files.get(name) {
            Some(b) => {
                files.reads += 1;
                unsafe { *size = b.len() };
                b.as_ptr().cast::<c_char>()
            }
            None => ptr::null(),
        }
    }

    unsafe extern "C" fn t_release(data: *mut c_void, _name: *const c_char, _buf: *const c_char) {
        // SAFETY: tests pass a `Files` as data.
        let files = unsafe { &mut *(data as *mut Files) };
        files.releases += 1;
    }

    fn files(entries: &[(&str, &str)]) -> Box<Files> {
        Box::new(Files {
            files: entries.iter().map(|(k, v)| ((*k).to_owned(), v.as_bytes().to_vec())).collect(),
            reads: 0,
            releases: 0,
            last_from: RefCell::new(None),
        })
    }

    fn loader(f: &mut Files) -> NlcLoader {
        NlcLoader {
            resolve: Some(t_resolve),
            read: Some(t_read),
            release: Some(t_release),
            data: (f as *mut Files).cast::<c_void>(),
        }
    }

    /// Severity, code, message, file, line, column.
    type Diag = (c_int, String, String, String, c_int, c_int);

    /// Compiles with `compile` through the C ABI and returns the chunk and
    /// diagnostics read back through the C accessors.
    fn run(compile: CompileFn, src: &str, name: &str, ld: &NlcLoader) -> (Option<Vec<u8>>, Vec<Diag>) {
        let name_c = CString::new(name).unwrap();
        let opts = NlcOptions { checks: 1, strip: 0, bytecode: 0, level: 0, decls: ptr::null(), ndecls: 0, localize: 1 };
        unsafe {
            let r = nl_compile_with(compile, false, src.as_ptr().cast(), src.len(), name_c.as_ptr(), &opts, ld);
            assert!(!r.is_null());
            let mut size = 0usize;
            let c = nlc_chunk(r, &mut size);
            let chunk = (!c.is_null()).then(|| std::slice::from_raw_parts(c.cast::<u8>(), size).to_vec());
            let n = nlc_ndiagnostics(r);
            let mut diags = Vec::new();
            for i in 0..n {
                let d = &*nlc_diagnostic(r, i);
                let s = |p: *const c_char| CStr::from_ptr(p).to_str().unwrap().to_owned();
                diags.push((d.severity, s(d.code), s(d.message), s(d.file), d.line, d.column));
            }
            assert!(nlc_diagnostic(r, n).is_null());
            nlc_free(r);
            (chunk, diags)
        }
    }

    fn echo(src: &[u8], _name: &str, _o: &CompileOptions, _l: &dyn Loader) -> CompileOutput {
        CompileOutput { chunk: Some(src.to_vec()), diagnostics: vec![] }
    }

    #[test]
    fn chunk_passes_through_with_its_length() {
        let mut f = files(&[]);
        let ld = loader(&mut f);
        let (chunk, diags) = run(echo, "return 1\0 with a nul", "=t", &ld);
        assert_eq!(chunk.unwrap(), b"return 1\0 with a nul");
        assert!(diags.is_empty());
    }

    fn diagnose(_src: &[u8], name: &str, _o: &CompileOptions, _l: &dyn Loader) -> CompileOutput {
        let d = |severity, code: &'static str, message: &str, line| Diagnostic {
            severity,
            code,
            message: message.to_owned(),
            file: name.trim_start_matches('@').to_owned(),
            line,
            column: 7,
            end_line: line,
            end_column: 9,
        };
        CompileOutput {
            chunk: None,
            diagnostics: vec![
                d(Severity::Error, "undeclared-name", "x is not declared", 3),
                d(Severity::Warning, "shadow", "nul\0inside", 4),
                d(Severity::Lint, "naming", "use snake_case", u32::MAX),
            ],
        }
    }

    #[test]
    fn diagnostics_cross_with_severity_and_position() {
        let mut f = files(&[]);
        let ld = loader(&mut f);
        let (chunk, diags) = run(diagnose, "", "@/m.nl", &ld);
        assert!(chunk.is_none());
        assert_eq!(diags.len(), 3);
        assert_eq!(diags[0], (NLC_DIAG_ERROR, "undeclared-name".into(), "x is not declared".into(), "/m.nl".into(), 3, 7));
        assert_eq!(diags[1].0, NLC_DIAG_WARNING);
        assert_eq!(diags[1].2, "nul\u{FFFD}inside");
        assert_eq!(diags[2].0, NLC_DIAG_LINT);
        assert_eq!(diags[2].4, c_int::MAX);
    }

    fn imports(_src: &[u8], name: &str, _o: &CompileOptions, l: &dyn Loader) -> CompileOutput {
        let from = name.strip_prefix('@');
        let found = l.resolve(from, "lib/a").expect("lib/a resolves");
        let text = l.read(&found).expect("lib/a reads");
        assert!(l.resolve(from, "missing").is_none());
        assert!(l.read("/missing.nl").is_none());
        CompileOutput { chunk: Some(text), diagnostics: vec![] }
    }

    #[test]
    fn loader_callbacks_are_bridged() {
        let mut f = files(&[("/lib/a.nl", "the contents")]);
        let ld = loader(&mut f);
        let (chunk, diags) = run(imports, "", "@/main.nl", &ld);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(chunk.unwrap(), b"the contents");
        assert_eq!((f.reads, f.releases), (1, 1));
        assert_eq!(f.last_from.borrow().as_deref(), Some("/main.nl"));
    }

    /// A loader whose canonical names are not UTF-8: it records the bytes
    /// it is given back.
    struct Latin1 {
        froms: RefCell<Vec<Vec<u8>>>,
        reads: RefCell<Vec<Vec<u8>>>,
    }

    unsafe extern "C" fn l1_resolve(
        data: *mut c_void,
        from: *const c_char,
        path: *const c_char,
        out: *mut c_char,
        size: usize,
    ) -> c_int {
        // SAFETY: tests pass a `Latin1` as data and valid strings.
        let l = unsafe { &*(data as *const Latin1) };
        if !from.is_null() {
            l.froms.borrow_mut().push(unsafe { CStr::from_ptr(from) }.to_bytes().to_vec());
        }
        let path = unsafe { CStr::from_ptr(path) }.to_bytes();
        // "caf\xe9/" + path + ".nl", as a Latin-1 file system would name it.
        let mut name = b"caf\xe9/".to_vec();
        name.extend_from_slice(path);
        name.extend_from_slice(b".nl");
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

    unsafe extern "C" fn l1_read(data: *mut c_void, name: *const c_char, size: *mut usize) -> *const c_char {
        // SAFETY: tests pass a `Latin1` as data.
        let l = unsafe { &*(data as *const Latin1) };
        l.reads.borrow_mut().push(unsafe { CStr::from_ptr(name) }.to_bytes().to_vec());
        static TEXT: &[u8] = b"contents";
        unsafe { *size = TEXT.len() };
        TEXT.as_ptr().cast::<c_char>()
    }

    fn imports_twice(_src: &[u8], name: &str, _o: &CompileOptions, l: &dyn Loader) -> CompileOutput {
        let from = name.strip_prefix('@');
        let a = l.resolve(from, "a").expect("a resolves");
        let b = l.resolve(from, "b").expect("b resolves");
        assert_ne!(a, b, "two names stay two names");
        assert!(l.read(&a).is_some() && l.read(&b).is_some());
        // An import of the imported file: `from` is a name given back.
        assert!(l.resolve(Some(&a), "c").is_some());
        CompileOutput { chunk: Some(Vec::new()), diagnostics: vec![] }
    }

    #[test]
    fn names_that_are_not_utf8_go_back_as_the_same_bytes() {
        let l = Box::new(Latin1 { froms: RefCell::new(vec![]), reads: RefCell::new(vec![]) });
        let ld = NlcLoader {
            resolve: Some(l1_resolve),
            read: Some(l1_read),
            release: None,
            data: (&*l as *const Latin1).cast_mut().cast::<c_void>(),
        };
        // The chunk is itself a file with a Latin-1 name.
        let name = b"@caf\xe9/main.nl\0";
        let opts = NlcOptions { checks: 1, strip: 0, bytecode: 0, level: 0, decls: ptr::null(), ndecls: 0, localize: 1 };
        unsafe {
            let r = nl_compile_with(imports_twice, false, ptr::null(), 0, name.as_ptr().cast(), &opts, &ld);
            assert!(!r.is_null());
            assert_eq!(nlc_ndiagnostics(r), 0);
            nlc_free(r);
        }
        assert_eq!(*l.reads.borrow(), vec![b"caf\xe9/a.nl".to_vec(), b"caf\xe9/b.nl".to_vec()]);
        assert_eq!(
            *l.froms.borrow(),
            vec![b"caf\xe9/main.nl".to_vec(), b"caf\xe9/main.nl".to_vec(), b"caf\xe9/a.nl".to_vec()]
        );
    }

    fn panics(_src: &[u8], _name: &str, _o: &CompileOptions, _l: &dyn Loader) -> CompileOutput {
        panic!("the resolver fell over");
    }

    #[test]
    fn a_panic_becomes_an_internal_error() {
        let mut f = files(&[]);
        let ld = loader(&mut f);
        let (chunk, diags) = run(panics, "", "=stdin", &ld);
        assert!(chunk.is_none());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].1, "internal-error");
        assert!(diags[0].2.contains("the resolver fell over"), "{}", diags[0].2);
        assert_eq!(diags[0].3, "stdin");
    }

    /// `nlc_Options.bytecode` selects the backend (nlc.h): 1 gives a
    /// LuaJIT dump (its header starts with ESC "LJ"), 0 gives Lua source.
    #[test]
    fn the_bytecode_option_selects_the_backend() {
        let mut f = files(&[]);
        let ld = loader(&mut f);
        let name = c"=stdin";
        let src = b"print(1)\n";
        for (bytecode, is_dump) in [(1, true), (0, false)] {
            let opts = NlcOptions { checks: 1, strip: 0, bytecode, level: 0, decls: ptr::null(), ndecls: 0, localize: 1 };
            let chunk = unsafe {
                let r = nl_compile(src.as_ptr().cast(), src.len(), name.as_ptr(), &opts, &ld);
                assert!(!r.is_null());
                assert_eq!(nlc_ndiagnostics(r), 0);
                let mut size = 0usize;
                let c = nlc_chunk(r, &mut size);
                assert!(!c.is_null());
                let chunk = std::slice::from_raw_parts(c.cast::<u8>(), size).to_vec();
                nlc_free(r);
                chunk
            };
            assert_eq!(chunk.starts_with(b"\x1bLJ"), is_dump, "bytecode={bytecode}");
        }
    }

    /// `nlc_Options.decls` names declaration files: each is
    /// read through the loader, a declared name then resolves, and a name
    /// the loader cannot read is an error of the compile.
    #[test]
    fn declaration_files_are_read_through_the_loader() {
        let mut f = files(&[("game.d.nl", "declare fn host_hello(): String\n")]);
        let ld = loader(&mut f);
        let src = b"print host_hello()\n";
        let name = c"=stdin";
        let run = |names: &[&std::ffi::CStr]| {
            let ptrs: Vec<*const c_char> = names.iter().map(|n| n.as_ptr()).collect();
            let opts = NlcOptions { checks: 1, strip: 0, bytecode: 0, level: 0, decls: ptrs.as_ptr(), ndecls: ptrs.len(), localize: 1 };
            unsafe {
                let r = nl_compile(src.as_ptr().cast(), src.len(), name.as_ptr(), &opts, &ld);
                assert!(!r.is_null());
                let mut size = 0usize;
                let chunk = !nlc_chunk(r, &mut size).is_null();
                let codes: Vec<String> = (0..nlc_ndiagnostics(r)).map(|i| CStr::from_ptr((*nlc_diagnostic(r, i)).code).to_str().unwrap().to_owned()).collect();
                nlc_free(r);
                (chunk, codes)
            }
        };
        let (chunk, codes) = run(&[]);
        assert!(!chunk && codes.contains(&"undeclared-name".to_owned()), "{codes:?}");
        let (chunk, codes) = run(&[c"game.d.nl"]);
        assert!(chunk, "{codes:?}");
        let (chunk, codes) = run(&[c"game.d.nl", c"missing.d.nl"]);
        assert!(!chunk && codes.first().map(String::as_str) == Some("missing-declaration-file"), "{codes:?}");
    }

    #[test]
    fn null_arguments_are_tolerated() {
        unsafe {
            assert!(nlc_chunk(ptr::null(), ptr::null_mut()).is_null());
            assert_eq!(nlc_ndiagnostics(ptr::null()), 0);
            assert!(nlc_diagnostic(ptr::null(), 0).is_null());
            nlc_free(ptr::null_mut());
            let r = nl_compile_with(echo, false, ptr::null(), 0, ptr::null(), ptr::null(), ptr::null());
            assert!(!r.is_null());
            let mut size = 99usize;
            assert!(!nlc_chunk(r, &mut size).is_null());
            assert_eq!(size, 0);
            nlc_free(r);
        }
    }

    #[test]
    fn the_real_compile_answers_through_the_abi() {
        // Whatever the compiler makes of it, the ABI returns a result
        // with a chunk or at least one error.
        let mut f = files(&[]);
        let ld = loader(&mut f);
        let src = "let x = 1\n";
        let name = CString::new("=t").unwrap();
        unsafe {
            let r = nl_compile(src.as_ptr().cast(), src.len(), name.as_ptr(), ptr::null(), &ld);
            assert!(!r.is_null());
            let chunk = nlc_chunk(r, ptr::null_mut());
            let has_error = (0..nlc_ndiagnostics(r)).any(|i| (*nlc_diagnostic(r, i)).severity == NLC_DIAG_ERROR);
            assert!(!chunk.is_null() || has_error);
            nlc_free(r);
        }
    }
}
