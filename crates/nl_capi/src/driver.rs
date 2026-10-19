//! The compile entry point: NL source in; a chunk and diagnostics out.
//!
//! The compiler is pure. It
//! reads imported files only through the [`Loader`] it is given and never
//! enters a Lua state, so it can be called from any thread.
//!
//! # The chunk
//!
//! [`CompileOutput::chunk`] is input for `lua_load`: a LuaJIT bytecode dump
//! from the bytecode backend ([`Emit::Bytecode`]), or Lua source text
//! printed by the Lua backend ([`Emit::Lua`], the default until the VM
//! fork selects bytecode). Either is loaded in two steps, because Lua source
//! has no other way to reach the runtime library without a global:
//!
//! 1. `lua_load(chunk, chunk_name)` gives a function `P`;
//! 2. `P(rt)`, where `rt` is the table that `runtime/nl.lua` returned for
//!    this state, returns the file's main function.
//!
//! The main function is what `nl_loadbuffer` pushes: calling it runs the file
//! and returns its export table.
//!
//! # Names
//!
//! `chunk_name` follows Lua's chunk names: `@name` for a file,
//! where `name` is the canonical name the loader resolved, and `=text` for
//! anything else. Imports in a chunk named `@name` resolve with `name` as the
//! importing file; in any other chunk they resolve with no importing file.

pub use nl_ir::Localize;
pub use nl_sema::Profile;

/// Which backend produces the chunk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Emit {
    /// Lua source from `nl_luagen`: the oracle backend and `nlc build
    /// --emit=lua`.
    #[default]
    Lua,
    /// A LuaJIT bytecode dump from `nl_codegen`.
    Bytecode,
}

/// Per-compile options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompileOptions {
    /// Emit runtime type checks (`NL_OPT_CHECKS`). `false` is the unchecked
    /// build.
    pub checks: bool,
    /// Leave debug information out (`NL_OPT_STRIP`).
    pub strip: bool,
    /// The backend.
    pub emit: Emit,
    /// Run the analyzer: type errors stop the
    /// compile, warnings and lints are reported. Off, only the resolver's
    /// checks apply. The analyzer still runs for the member keys of
    /// declaration files, which generated code needs, but reports nothing.
    pub analyze: bool,
    /// Let the analyzer's proven checks and static resolution from
    /// declared types change generated code. Off, every
    /// annotation is checked and every member tested at runtime.
    pub static_resolution: bool,
    /// The feature level of a bytecode dump: 0
    /// uses stock opcodes and loads in a stock LuaJIT; 1 uses the fork's
    /// opcodes, flags and events. Ignored for Lua source.
    pub level: u8,
    /// Which Lua paths are read once, when the file loads, into hidden
    /// locals (`NL_OPT_LOCALIZE`).
    pub localize: Localize,
    /// The subset of the language the target supports: a use of a name
    /// the profile leaves out is an error (`nl_sema::profile`), in this
    /// file and in the files it imports.
    pub profile: Profile,
}

impl Default for CompileOptions {
    fn default() -> Self {
        CompileOptions {
            checks: true,
            strip: false,
            emit: Emit::Lua,
            analyze: true,
            static_resolution: true,
            level: 0,
            localize: Localize::Std,
            profile: Profile::Full,
        }
    }
}

/// How the compiler finds and reads the files a source imports
/// (`nl_Loader`). The search order of imports is the loader's
/// business, not the compiler's.
pub trait Loader {
    /// Turns the path written in an `import` into the file's canonical name.
    /// `from` is the canonical name of the importing file, or `None`.
    /// `None` is returned when there is no such file.
    fn resolve(&self, from: Option<&str>, path: &str) -> Option<String>;

    /// The contents of the file with this canonical name.
    fn read(&self, name: &str) -> Option<Vec<u8>>;

    /// The declaration files in scope: each as
    /// its canonical name and its contents, in the order they apply. The
    /// built-in declarations of `decl/` are always in scope and are not
    /// listed here. The default is none.
    ///
    /// For the C API this is what `nl_adddecl` collects.
    fn declarations(&self) -> Vec<(String, Vec<u8>)> {
        Vec::new()
    }
}

/// `NL_DIAG_ERROR`, `NL_DIAG_WARNING`, `NL_DIAG_LINT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Lint,
}

/// One diagnostic of a compile (`nl_Diagnostic`). Lines and
/// columns are 1-based; a column counts characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// A stable identifier, such as `"undeclared-name"`.
    pub code: &'static str,
    pub message: String,
    /// The canonical name of the file, or the chunk name without its `=`.
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// What a compile produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompileOutput {
    /// The chunk (see the module documentation), or `None` when a
    /// diagnostic is an error.
    pub chunk: Option<Vec<u8>>,
    /// Every diagnostic, of this file and of the files it imports, errors
    /// first within a file and otherwise in source order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Compiles one NL source file: parse, resolve, analyze, lower, then print
/// Lua or emit bytecode. Never panics;
/// every problem is a diagnostic.
pub fn compile(
    source: &[u8],
    chunk_name: &str,
    options: &CompileOptions,
    loader: &dyn Loader,
) -> CompileOutput {
    pipeline::compile(source, chunk_name, options, loader)
}

/// The declarations a compile with `profile` has in scope through
/// `loader`: the built-in ones of `decl/`, gmod's for a gmod profile, and
/// the loader's (their diagnostics are a compile's to report).
pub fn declarations_in_scope(loader: &dyn Loader, profile: Profile) -> nl_sema::Declarations {
    pipeline::load_declarations(loader, profile).0
}

mod imports;
mod pipeline;
mod regex_literal;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    struct NoFiles;

    impl Loader for NoFiles {
        fn resolve(&self, _from: Option<&str>, _path: &str) -> Option<String> {
            None
        }
        fn read(&self, _name: &str) -> Option<Vec<u8>> {
            None
        }
    }

    fn lua(src: &str) -> String {
        let out = compile(src.as_bytes(), "@t.nl", &CompileOptions::default(), &NoFiles);
        assert!(out.chunk.is_some(), "{:?}", out.diagnostics);
        String::from_utf8(out.chunk.unwrap()).unwrap()
    }

    /// A loader with one declaration file.
    struct Decls(&'static str);

    impl Loader for Decls {
        fn resolve(&self, _from: Option<&str>, _path: &str) -> Option<String> {
            None
        }
        fn read(&self, _name: &str) -> Option<Vec<u8>> {
            None
        }
        fn declarations(&self) -> Vec<(String, Vec<u8>)> {
            vec![("game.d.nl".to_string(), self.0.as_bytes().to_vec())]
        }
    }

    /// Declaration files: a declared name compiles to its
    /// Lua path, a member to its Lua key, a method with `self` to a method
    /// call; a `declare struct` is checked by its metatable and a
    /// `declare type` by its registered name; the analyzer checks calls
    /// against the declared signatures; a `const` cannot be assigned.
    #[test]
    fn declaration_files() {
        const DECL: &str = "declare module Foo = :foo\n  coolness_factor: Number as 'coolnessFactor'\n  const limit: Number\n  fn return_largest(a: Number, b: Number): Number as 'ReturnLargest'\nend\ndeclare module Json = :require('json')\n  fn encode(value: any): String\nend\ndeclare struct Point\n  x: Number\n  fn new(x: Number?, y: Number?): Point\n  fn length(self): Number\nend\ndeclare type Player : Entity\n  fn pos(self): Point as 'GetPos'\nend\ndeclare type Entity\nend\ndeclare fn local_player(): Player = :LocalPlayer\ndeclare const max_players: Number = :config.maxPlayers\n";
        let src = "let n: Number = Foo.return_largest(1, 2)\nFoo.coolness_factor = n + Foo.limit\nprint Json.encode({a: 1})\nlet p = Point.new(1, 2)\nprint p.length\nfn at(pt: Point, pl: Player): Number\n  pl.pos.x + pt.x\nend\nprint at(p, local_player)\nprint max_players\n";
        let out = compile(src.as_bytes(), "@t.nl", &CompileOptions::default(), &Decls(DECL));
        assert!(out.chunk.is_some(), "{:?}", out.diagnostics);
        let lua = String::from_utf8(out.chunk.unwrap()).unwrap();
        assert!(lua.contains("foo.ReturnLargest(1, 2)"), "{lua}");
        assert!(lua.contains("foo.coolnessFactor = ") && lua.contains("foo.limit"), "{lua}");
        assert!(lua.contains("(require(\"json\")).encode("), "{lua}");
        // The Lua backend prints `x:m()` as `__rt.lmethod.m(x)`.
        assert!(lua.contains("Point.new(1, 2)") && lua.contains("lmethod.length(p)"), "{lua}");
        assert!(lua.contains("lmethod.GetPos(pl)"), "{lua}");
        assert!(lua.contains("getmetatable(v) == Point"), "{lua}");
        assert!(lua.contains("__rt.gettype(\"Player\")"), "{lua}");
        assert!(lua.contains("LocalPlayer()") && lua.contains("config.maxPlayers"), "{lua}");

        // The analyzer: a wrong argument, a block, an assigned const.
        let bad = "Foo.return_largest(\"a\", 2)\nFoo.limit = 3\nFoo.return_largest 1 do end\n";
        let out = compile(bad.as_bytes(), "@t.nl", &CompileOptions::default(), &Decls(DECL));
        assert!(out.chunk.is_none());
        let codes: Vec<(u32, &str)> = out.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| (d.line, d.code)).collect();
        assert!(codes.contains(&(1, "type-mismatch")) && codes.contains(&(2, "assign-declared-const")) && codes.contains(&(3, "block-without-function-param")), "{codes:?}");

        // Without the analyzer the names still compile to their paths.
        let opts = CompileOptions { analyze: false, ..CompileOptions::default() };
        let out = compile(src.as_bytes(), "@t.nl", &opts, &Decls(DECL));
        let lua = String::from_utf8(out.chunk.expect("chunk")).unwrap();
        assert!(lua.contains("foo.ReturnLargest(1, 2)") && lua.contains("lmethod.GetPos(pl)"), "{lua}");
    }

    /// Type values are built once, in the pool of the block that declares
    /// their types, not at each check.
    #[test]
    fn type_values_are_built_once() {
        let lua = lua("struct Pt\n  x: Number\nend\nfn f(a: Number?, b: Number?, p: Pt?, q: {x: Number})\n  a\nend\nprint f(1, 2, nil, {x: 1})\n");
        assert_eq!(lua.matches("__rt.optional(").count(), 2, "{lua}");
        assert_eq!(lua.matches("__rt.record(").count(), 1, "{lua}");
        let body = lua.split("function(a, b, p, q)").nth(1).unwrap();
        assert!(!body.contains("__rt.optional(") && !body.contains("__rt.record("), "{lua}");
        assert!(body.contains("__rt.check(a, types[1]") && body.contains("__rt.check(b, types[1]"), "{lua}");
    }

    /// `==` and `!=` go through `rt.eq` (the
    /// left operand decides), except against a literal number, string,
    /// boolean or nil, which stay Lua's comparisons.
    #[test]
    fn equality_with_literals_stays_lua() {
        let lua = lua("let a: any = {}\nlet b: any = {}\nprint(a == nil, a != 1, \"s\" == a, a == true)\nprint(a == b, a != b)\n");
        assert!(lua.contains("a == nil") && lua.contains("a ~= 1") && lua.contains("\"s\" == a") && lua.contains("a == true"), "{lua}");
        assert!(lua.contains("__rt.eq(a, b)") && lua.contains("not __rt.eq(a, b)"), "{lua}");
    }

    /// The owner of the chunk's
    /// primitive definitions is computed once per load, outside the main
    /// function, and only when the chunk needs it.
    #[test]
    fn chunk_owner_is_computed_once_per_load() {
        let out = lua("fn Number.dbl\n  self * 2\nend\nfn Bool.+(r)\n  1\nend\n");
        // A line map may come between (crates/nl_luagen), on the same line.
        assert!(out.starts_with("local __rt = ...; local owner = __rt.owner(\"@t.nl\");"), "{out}");
        assert!(out.lines().next().unwrap().contains(" return function(...)"), "{out}");
        assert_eq!(out.matches("__rt.owner(").count(), 1, "{out}");
        assert!(out.contains("method, owner)") && out.contains("__rt.primop(\"Bool\", \"__add\""), "{out}");
        let plain = lua("print 1\n");
        assert!(!plain.contains("__rt.owner"), "{plain}");
    }

    /// The bytecode backend produces a dump of the same chunk, and a
    /// function over LuaJIT's limits is an error naming it.
    #[test]
    fn bytecode_backend() {
        let opts = CompileOptions { emit: Emit::Bytecode, ..CompileOptions::default() };
        let out = compile(b"fn f(a)\n  a + 1\nend\nprint f(1)\n", "@t.nl", &opts, &NoFiles);
        let chunk = out.chunk.expect("a dump");
        assert_eq!(&chunk[..3], b"\x1bLJ", "{:?}", out.diagnostics);
        let mut src = String::from("fn big()\n");
        for i in 0..205 {
            src.push_str(&format!("  let v{i} = {i}\n"));
        }
        src.push_str("  v0\nend\n");
        let out = compile(src.as_bytes(), "@t.nl", &opts, &NoFiles);
        assert!(out.chunk.is_none());
        let d = &out.diagnostics[0];
        assert_eq!(d.code, "vm-limit");
        assert!(d.message.contains("big"), "{}", d.message);
    }

    /// The instructions of every prototype of a stripped dump
    /// (`nl_codegen::dump` gives the layout), with the dump's flags.
    fn dump_instructions(dump: &[u8]) -> (u32, Vec<nl_codegen::bc::Ins>) {
        fn uleb(d: &[u8], at: &mut usize) -> u32 {
            let (mut v, mut shift) = (0u32, 0);
            loop {
                let b = d[*at];
                *at += 1;
                v |= u32::from(b & 0x7f) << shift;
                if b < 0x80 {
                    return v;
                }
                shift += 7;
            }
        }
        assert_eq!(&dump[..3], b"\x1bLJ");
        let mut at = 4;
        let flags = uleb(dump, &mut at);
        assert_ne!(flags & 0x02, 0, "a stripped dump");
        let mut ins = Vec::new();
        loop {
            let len = uleb(dump, &mut at) as usize;
            if len == 0 {
                return (flags, ins);
            }
            let end = at + len;
            let mut p = at + 4;
            let _kgc = uleb(dump, &mut p);
            let _kn = uleb(dump, &mut p);
            let nbc = uleb(dump, &mut p) as usize;
            for i in 0..nbc {
                let w = &dump[p + 4 * i..p + 4 * i + 4];
                ins.push(u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
            }
            at = end;
        }
    }

    /// At level 1, bitwise operators on operands proven
    /// numbers are the VM's bit opcodes; on operands declared `any`, or
    /// without static resolution, they call the runtime's helpers (`KINTR`
    /// + `CALL`); a level-0 dump has no bit opcode.
    #[test]
    fn bit_opcodes_for_proven_numbers() {
        use nl_codegen::bc::{self, Op};
        use nl_codegen::intrinsics::index;
        let compile_at = |src: &str, level: u8, static_resolution: bool| {
            let opts = CompileOptions { emit: Emit::Bytecode, strip: true, level, static_resolution, ..CompileOptions::default() };
            let out = compile(src.as_bytes(), "@t.nl", &opts, &NoFiles);
            dump_instructions(&out.chunk.unwrap_or_else(|| panic!("{:?}", out.diagnostics)))
        };
        let ops = |ins: &[bc::Ins]| ins.iter().map(|i| Op::from_ins(*i).expect("a known opcode")).collect::<Vec<_>>();
        // The bitwise intrinsics `KINTR` loads, by name.
        let names = ["band", "bor", "bxor", "shl", "shr", "ushr", "bnot", "nband", "nbor", "nbxor", "nlshift", "narshift", "nrshift"];
        let loaded = |ins: &[bc::Ins]| -> Vec<&str> {
            ins.iter()
                .filter(|i| Op::from_ins(**i) == Some(Op::KINTR))
                .filter_map(|i| names.iter().copied().find(|n| index(n) == Some(bc::d_of(*i))))
                .collect()
        };
        let typed = "fn f(a: Number, b: Number): Number\n  ~(a & b | a ^ b << 1 >> 2 >>> 3)\nend\nlet x: Number = 5\nx &= 3\nprint f(x, 2)\n";
        let (flags, ins) = compile_at(typed, 1, true);
        let o = ops(&ins);
        assert_eq!(flags & 0x10, 0x10, "BCDUMP_F_BITOP");
        for op in [Op::BAND, Op::BOR, Op::BXOR, Op::BSHL, Op::BSAR, Op::BSHR, Op::BNOT] {
            let n = o.iter().filter(|x| **x == op).count();
            assert_eq!(n, if op == Op::BAND { 2 } else { 1 }, "{op:?} in {o:?}");
        }
        assert_eq!(loaded(&ins), Vec::<&str>::new());
        let untyped = typed.replace(": Number", ": any");
        for (src, level, sr) in [(untyped.as_str(), 1, true), (typed, 1, false), (typed, 0, true)] {
            let (flags, ins) = compile_at(src, level, sr);
            assert_eq!(flags & 0x10, 0, "level {level}, static resolution {sr}: {:?}", ops(&ins));
            assert!(!ops(&ins).iter().any(|o| o.is_bitop()), "level {level}, static resolution {sr}: {:?}", ops(&ins));
            if level == 1 {
                let mut l = loaded(&ins);
                l.sort_unstable();
                assert_eq!(l, ["band", "band", "bnot", "bor", "bxor", "shl", "shr", "ushr"]);
            }
        }
    }

    /// A primitive method is registered by name only after `primmethod`
    /// accepted it.
    #[test]
    fn primitive_methods_are_registered_after_the_check() {
        let lua = lua("fn Number.dbl\n  self * 2\nend\n");
        let define = lua.find("__rt.primmethod(").unwrap();
        let register = lua.find("__rt.register(").unwrap();
        assert!(define < register, "{lua}");
        assert!(!lua.contains("__rt.primmethod(\"Number\", \"dbl\", __rt.register"), "{lua}");
    }
}
