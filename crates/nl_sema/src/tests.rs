use std::path::{Path, PathBuf};

use crate::*;
#[allow(unused_imports)]
use crate::decl::prelude_files;

/// Accepts every import, with every name a variable.
struct AnyImports;

impl ImportProvider for AnyImports {
    fn exports(&self, _path: &str) -> Result<Vec<ExportInfo>, String> {
        Ok(Vec::new())
    }
}

fn run_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run")
}

fn errors(src: &str) -> Vec<(u32, String)> {
    let parse = nl_syntax::parse(src);
    assert!(!parse.has_errors(), "parse errors: {:?}", parse.diagnostics());
    let res = resolve(&parse.syntax(), &AnyImports);
    let index = nl_syntax::LineIndex::new(src);
    res.diagnostics
        .iter()
        .filter(|d| d.error && d.code != "not-exported")
        .map(|d| (index.line_col(src, d.range.start()).line, d.message.clone()))
        .collect()
}

/// The resolver's errors, then the analyzer's when the resolver found
/// none (the pipeline's order), with the built-in declarations.
fn analyzer_diagnostics(src: &str) -> Vec<(u32, &'static str, String, bool)> {
    let parse = nl_syntax::parse(src);
    assert!(!parse.has_errors(), "parse errors: {:?}", parse.diagnostics());
    let decls = prelude_declarations();
    let root = parse.syntax();
    let res = resolve_with(&root, &AnyImports, &decls);
    let index = nl_syntax::LineIndex::new(src);
    let line = |r: nl_syntax::TextRange| index.line_col(src, r.start()).line;
    let mut out: Vec<(u32, &'static str, String, bool)> = res.diagnostics.iter().filter(|d| d.code != "not-exported").map(|d| (line(d.range), d.code, d.message.clone(), d.error)).collect();
    if !res.has_errors() {
        let opts = AnalyzeOptions { static_resolution: true, decls: Some(&decls), lints: true, imports: None, record: false };
        let analysis = analyze(&root, &res, &opts);
        out.extend(analysis.diagnostics.iter().map(|d| (line(d.range), d.code, d.message.clone(), d.error)));
    }
    out
}

fn analyzer_errors(src: &str) -> Vec<(u32, String)> {
    analyzer_diagnostics(src).into_iter().filter(|d| d.3).map(|d| (d.0, d.2)).collect()
}

fn analyzer_codes(src: &str) -> Vec<(u32, &'static str)> {
    analyzer_diagnostics(src).into_iter().map(|d| (d.0, d.1)).collect()
}

#[test]
fn simple_resolution() {
    let src = "let a = 1\nfn f(x) x + a end\nprint f(2)\n";
    assert!(errors(src).is_empty());
    let errs = errors("print nope\n");
    assert_eq!(errs.len(), 1);
}

#[test]
fn hoisting() {
    assert!(errors("print g()\nfn g 1 end\n").is_empty());
}

/// A `const` is declared at the top of its block: a function above it
/// sees it; the block's own code before its line, its own initializer
/// included, is an error.
#[test]
fn const_hoisting() {
    assert!(errors("fn f\n  X\nend\nconst X = 1\n").is_empty());
    assert!(errors("module M\n  fn f\n    X\n  end\n  const X = 1\nend\n").is_empty());
    assert!(errors("let g = fn() X end\nconst X = 1\nprint g()\n").is_empty());
    assert_eq!(errors("print X\nconst X = 1\n").len(), 1);
    assert_eq!(errors("if true\n  print X\nend\nconst X = 1\n").len(), 1);
    assert_eq!(errors("const X = X\n").len(), 1);
    // Redeclared in its block: not hoisted, so the read sees nothing.
    assert_eq!(errors("fn f\n  X\nend\nconst X = 1\nconst X = 2\n").len(), 1);
    // `let` is not hoisted.
    assert_eq!(errors("fn f\n  x\nend\nlet x = 1\n").len(), 1);
}

/// Every entry program resolves without errors; the excluded areas are
/// those of tests/run/README.md that need later milestones.
#[test]
fn entry_programs_resolve() {
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(run_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "nl") || !path.with_extension("out").exists() {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let errs = errors(&src);
        if !errs.is_empty() {
            failures.push(format!("{}: {:?}", path.display(), errs));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Rejected programs that the resolver or the analyzer must catch report
/// an error on the line after `# expect-error:`.
#[test]
fn reject_programs() {
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(run_dir().join("reject")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with("regex_") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let expect = src.lines().position(|l| l.trim_start().starts_with("# expect-error:")).unwrap() as u32 + 2;
        let errs = analyzer_errors(&src);
        if !errs.iter().any(|(line, _)| *line == expect) {
            failures.push(format!("{name}: expected an error on line {expect}, got {errs:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every entry program passes the analyzer without errors, with the
/// built-in declarations in scope.
#[test]
fn entry_programs_analyze() {
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(run_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "nl") || !path.with_extension("out").exists() {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let errs = analyzer_errors(&src);
        if !errs.is_empty() {
            failures.push(format!("{}: {:?}", path.display(), errs));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The built-in declaration files parse and load without a diagnostic.
#[test]
fn prelude_declarations_are_clean() {
    let errs = crate::decl::prelude_syntax_errors();
    assert!(errs.is_empty(), "{}", errs.join("\n"));
    let (decls, diags) = Declarations::load(&crate::decl::prelude_files());
    assert!(diags.is_empty(), "{diags:?}");
    for name in ["Number", "String", "Table", "IO", "OS", "Event", "Worker", "Task", "Regex", "Time", "Duration", "print", "raise", "File"] {
        assert!(decls.lookup(name).is_some(), "{name} is not declared");
    }
}

// ---- the analyzer ----------------------------------------------------------

fn has_code(src: &str, line: u32, code: &str) -> bool {
    analyzer_codes(src).iter().any(|(l, c)| *l == line && *c == code)
}

#[test]
fn literal_mismatches_are_errors() {
    assert!(has_code("let n: Number = \"text\"\n", 1, "type-mismatch"));
    assert!(has_code("let n: Number = 1\nn = \"s\"\n", 2, "type-mismatch"));
    assert!(has_code("let f = fn 1 end\nf = 5\n", 2, "type-mismatch"));
    assert!(has_code("fn half(n: Number)\n  n / 2\nend\nhalf(\"x\")\n", 4, "type-mismatch"));
    assert!(has_code("fn half(n: Number)\n  n / 2\nend\nhalf()\n", 4, "missing-argument"));
    assert!(has_code("struct P\n  x: Number\nend\nlet p = P { y: 1 }\n", 4, "unknown-field"));
    assert!(has_code("struct P\n  x: Number\nend\nlet p = P { x: \"s\" }\n", 4, "type-mismatch"));
    assert!(has_code("fn f(a, b)\n  a\nend\nf(1, k: 2)\n", 4, "keyed-args-rejected"));
    assert!(has_code("fn f(a)\n  a\nend\nf(1, k: 2)\n", 4, "keyed-args-dropped"));
    // `any` opts out, in both directions.
    assert!(analyzer_errors("let v: any = \"text\"\nlet n: Number = v\n").is_empty());
    assert!(analyzer_errors("let n = 5\nn = \"s\"\nprint n\n").is_empty());
}

#[test]
fn narrowing_and_inference() {
    // `is` narrows; the else branch excludes.
    let src = "fn f(v: Number | String)\n  if v is Number\n    v + 1\n  else\n    v <> \"!\"\n  end\nend\n";
    assert!(analyzer_errors(src).is_empty(), "{:?}", analyzer_errors(src));
    // A `let` without annotation that is never assigned has its initializer's type.
    assert!(has_code("let s = \"x\"\nlet n: Number = s\n", 2, "type-mismatch"));
    // Inferred return types reach calls written before the declaration (hoisting).
    assert!(has_code("let n: Number = name()\nfn name\n  \"x\"\nend\n", 1, "type-mismatch"));
    // `unknown` must be narrowed before use.
    assert!(has_code("let u: unknown = 5\nlet n: Number = u\n", 2, "unknown-not-narrowed"));
    assert!(analyzer_errors("let u: unknown = 5\nif u is Number\n  print u + 1\nend\n").is_empty());
    // A record literal is a `{…}` type, a list a `[T]`.
    assert!(analyzer_errors("type P = {x: Number, y: Number}\nlet p: P = {x: 1, y: 2}\nlet l: [Number] = [1, 2]\nprint p, l\n").is_empty());
    assert!(has_code("type P = {x: Number, y: Number}\nlet p: P = {x: 1}\n", 2, "type-mismatch"));
}

#[test]
fn exhaustiveness_and_unreachable_clauses() {
    let src = "enum D\n  A\n  B\nend\nfn f(d: D)\n  case d\n  when D.A\n    1\n  end\nend\n";
    assert!(has_code(src, 6, "non-exhaustive"));
    let src = "struct C\n  r: Number\nend\nstruct R\n  w: Number\nend\ntype S = C | R\nfn f(s: S)\n  case s\n  when C {r}\n    r\n  when R {w}\n    w\n  end\nend\n";
    assert!(analyzer_codes(src).iter().all(|(_, c)| *c != "non-exhaustive"), "{:?}", analyzer_codes(src));
    let src = "fn f(v: Number | String)\n  case v\n  when is Number\n    1\n  when is String\n    2\n  when is String\n    3\n  end\nend\n";
    assert!(has_code(src, 7, "unreachable-when"));
    let src = "try\n  print 1\ncatch e: Exception\n  print 2\ncatch\n  print 3\nend\n";
    assert!(has_code(src, 5, "unreachable-catch"));
}

#[test]
fn lints() {
    assert!(has_code("let a = 1\ndo\n  let a = 2\n  print a\nend\nprint a\n", 3, "shadowing"));
    assert!(has_code("let unused = 1\n", 1, "unused-binding"));
    assert!(has_code("global Score = 1\nprint Score\n", 2, "global-use"));
    assert!(has_code("let v = 1\nprint v!\n", 2, "redundant-bang"));
    assert!(has_code("fn reg(cb: Function)\n  cb\nend\nfn cb\n  1\nend\nreg cb\n", 7, "fn-kind-argument"));
    assert!(has_code("struct S\n  x: Number\nend\nfn S.m\n  1\nend\ninterface I\n  fn m: Number\nend\ninterface I for S\n  fn m: Number\n    2\n  end\nend\n", 11, "method-replaced"));
}

#[test]
fn lua_path_to_own_function() {
    let code = "lua-path-own-function";
    // A call, a step after a call, a closure inside the function, a module
    // function, a function assigned to a variable, and a path on a line of
    // its own.
    assert!(has_code("pub fn f(x)\n  :f(x)\nend\n", 2, code));
    assert!(has_code("fn f(x)\n  if x\n    print :f(x).y\n  end\nend\n", 3, code));
    assert!(has_code("fn f\n  let g = fn(y) :f(y) end\n  g\nend\n", 2, code));
    assert!(has_code("module M\n  fn f(x)\n    :f(x)\n  end\nend\n", 3, code));
    assert!(has_code("let f = nil\nf = fn(x)\n  :f(x)\nend\n", 3, code));
    assert!(has_code("fn f\n  :f\n  1\nend\n", 2, code));
    // The global through `_G`; the function's own value; a `let` whose
    // initializer cannot see the name, so `:g` is the global; another
    // function's name.
    let quiet = ["fn f(x)\n  :_G.f(x)\nend\n", "fn f\n  let me = :f\n  me\nend\n", "fn f\n  :f\nend\n", "let g = fn(x)\n  :g(x)\nend\nprint g\n", "fn f(x)\n  x\nend\nfn h(x)\n  :f(x)\nend\n"];
    for src in quiet {
        assert!(analyzer_codes(src).iter().all(|(_, c)| *c != code), "{src}: {:?}", analyzer_codes(src));
    }
}

/// The last statement `x if c` is the tail `x`, as `x` alone would be:
/// its value is used, so it may be any expression or a list of values; a
/// loop modifier and a statement that is not last discard theirs.
#[test]
fn postfix_if_tail_is_a_value() {
    assert!(errors("fn f(x, ok)\n  x + 1 if ok\nend\n").is_empty());
    assert!(errors("fn f(ok)\n  1, 2 unless ok\nend\n").is_empty());
    let errs = errors("fn f(x, ok)\n  x + 1 if ok\n  x\nend\n");
    assert!(errs.len() == 1 && errs[0].0 == 2, "{errs:?}");
    let errs = errors("fn f(x, ok)\n  x + 1 while ok\nend\n");
    assert!(errs.len() == 1 && errs[0].0 == 2, "{errs:?}");
    // The analyzer types the value as optional.
    let src = "fn f(x: Number, ok: Bool): String\n  x if ok\nend\n";
    assert!(has_code(src, 2, "return-type"), "{:?}", analyzer_codes(src));
}

#[test]
fn raw_string_escapes() {
    let code = "raw-escape";
    for src in ["print 'a\\nb'\n", "print 'tab\\t'\n", "print 'nul\\0'\n", "print '\\x41'\n", "print '\\u{48}'\n", "print 'say \\\"hi\\\"'\n", "print 'it\\'s\\n'\n"] {
        assert!(has_code(src, 1, code), "{src}: {:?}", analyzer_codes(src));
    }
    // The raw escapes themselves, `\\n` (a backslash and an n, spelled out),
    // a backslash before anything else (a regex class, a path), a regex
    // pattern, and the other string forms.
    let quiet = [
        "print 'it\\'s'\n",
        "print 'back\\\\slash'\n",
        "print '\\\\n'\n",
        "print '^\\d+\\n'\n",
        "print 'C:\\games\\new'\n",
        "let re = Regex.new '\\n+'\nprint re\n",
        "print \"a\\nb\"\n",
        "print \"\"\"=\na\\nb\n\"\"\"\n",
        "print '%d+'\n",
    ];
    for src in quiet {
        assert!(analyzer_codes(src).iter().all(|(_, c)| *c != code), "{src}: {:?}", analyzer_codes(src));
    }
}

#[test]
fn library_keyed_options() {
    // The String library declares its keyed options as records.
    assert!(analyzer_errors("let p = \"a,b\".split(\",\", keep_empty: true, limit: 2)\nprint p\n").is_empty());
    assert!(has_code("let p = \"a,b\".split(\",\", keep: true)\nprint p\n", 1, "unknown-keyed-arg"));
    assert!(has_code("let p = \"a,b\".split(\",\", limit: \"2\")\nprint p\n", 1, "type-mismatch"));
    assert!(analyzer_errors("let s = \"{a}\".fmt(a: 1, b: 2)\nprint s\n").is_empty());
    assert!(has_code("let n: Number = \"{a}\".fmt(a: 1)\nprint n\n", 1, "type-mismatch"));
}

/// The members resolved to functions of `Table` ([`MemberHint::TableFn`]),
/// by line, with the hints the lowering gets in the mode.
fn table_fns(src: &str, static_resolution: bool) -> Vec<(u32, String)> {
    let parse = nl_syntax::parse(src);
    assert!(!parse.has_errors(), "parse errors: {:?}", parse.diagnostics());
    let decls = prelude_declarations();
    let root = parse.syntax();
    let res = resolve_with(&root, &AnyImports, &decls);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let opts = AnalyzeOptions { static_resolution, decls: Some(&decls), lints: true, imports: None, record: false };
    let analysis = analyze(&root, &res, &opts);
    let hints = if static_resolution { analysis.hints } else { analysis.hints.without_static_resolution() };
    let index = nl_syntax::LineIndex::new(src);
    let mut out: Vec<(u32, String)> = hints
        .members_declared
        .iter()
        .filter_map(|(r, h)| match h {
            MemberHint::TableFn { key } => Some((index.line_col(src, r.start()).line, key.clone())),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

#[test]
fn table_functions_are_methods_of_tables() {
    let src = "\
let xs: [Number] = [3, 1, 2]
print xs.map(fn(x) x end).filter(fn(x) true end)
let m: {[String]: Number} = {a: 1}
print m.keys
let t: Table = {a: 1}
print t.size, t.a
fn f(**opts)
  opts.max
end
let r = {count: 1}
print r.count, r.size
print xs.range, xs.len
fn Table.second
  self[2]
end
print xs.second, xs.sum!, [1].first
let u: any = xs
print u.map(fn(x) x end)
";
    // Typed lists, maps and tables, a chain on a declared function's result,
    // a function defined in the file, `!` and a literal; not the options
    // of `**opts`, a record, a constructor (`range`), `len` or an untyped
    // value. The same in both modes: nothing else finds them at runtime.
    let want: Vec<(u32, String)> = [(2, "filter"), (2, "map"), (4, "keys"), (6, "size"), (16, "first"), (16, "second"), (16, "sum")].iter().map(|(l, k)| (*l, k.to_string())).collect();
    assert_eq!(table_fns(src, true), want);
    assert_eq!(table_fns(src, false), want);
    // A member assigned, also by a compound assignment, is the key.
    let place = "let t: Table = {}\nt.count = 1\nt.count += 1\nt.map ??= 2\n";
    assert!(table_fns(place, true).is_empty());
    assert!(analyzer_errors(place).is_empty());
    // The arguments are checked against the function, receiver first.
    assert!(has_code("let xs: [Number] = [1]\nprint xs.take(\"2\")\n", 2, "type-mismatch"));
    assert!(has_code("fn Table.second\n  self[2]\nend\nprint Table.second(5)\n", 4, "type-mismatch"));
    assert!(analyzer_errors("fn Table.second\n  self[2]\nend\nprint Table.second([1, 2]), [1, 2].second\n").is_empty());
}

#[test]
fn table_methods_of_untyped_values_are_linted() {
    let codes = analyzer_codes("fn f(u)\n  u.map(fn(x) x end)\n  u.reverse()\n  u.frobnicate()\n  Table.map(u, fn(x) x end)\nend\n");
    assert_eq!(codes, vec![(2, "table-method-untyped")]);
    // An operator of `Table` has nowhere to live.
    assert!(has_code("fn Table.+(other)\n  self\nend\n", 1, "operator-owner"));
}

#[test]
fn primitive_method_through_its_type() {
    // `String.shout(s)`: the first argument is `self`, checked against the type.
    let decl = "fn String.surround(l: String, r: String): String\n  l <> self <> r\nend\n";
    assert!(analyzer_errors(&format!("{decl}let s: String = String.surround(\"x\", \"<\", \">\")\nprint s\n")).is_empty());
    assert!(has_code(&format!("{decl}print String.surround(5, \"<\", \">\")\n"), 4, "type-mismatch"));
    assert!(has_code(&format!("{decl}print String.surround(\"x\", \"<\")\n"), 4, "missing-argument"));
    assert!(has_code(&format!("{decl}let n: Number = String.surround(\"x\", \"<\", \">\")\nprint n\n"), 4, "type-mismatch"));
    // Read without a call, it is the function.
    assert!(analyzer_errors(&format!("{decl}let f = String.surround\nprint f(\"x\", \"<\", \">\")\n")).is_empty());
}

#[test]
fn declared_names_and_members() {
    let decl = "declare module Foo = :foo\n  coolness_factor: Number as 'coolnessFactor'\n  const limit: Number\n  fn return_largest(a: Number, b: Number): Number as 'ReturnLargest'\n  fn each(list: [Number], callback: Function)\nend\ndeclare fn local_player(): String = :LocalPlayer\n";
    let parse = nl_syntax::parse_decl_file(decl);
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
    let mut files = crate::decl::prelude_files();
    files.push(("game.d.nl".to_string(), parse.syntax()));
    let (decls, diags) = Declarations::load(&files);
    assert!(diags.is_empty());
    let src = "let n: Number = Foo.return_largest(1, 2)\nlet s: String = local_player\nFoo.coolness_factor = 3\nFoo.limit = 4\nFoo.each([1]) do |v| print v end\nFoo.return_largest 1 do end\nlet bad: String = Foo.return_largest(1, 2)\n";
    let p = nl_syntax::parse(src);
    let root = p.syntax();
    let res = resolve_with(&root, &AnyImports, &decls);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    assert_eq!(res.declared_path(decls.lookup("Foo").unwrap()).map(|r| r.global.as_str()), Some("foo"));
    let opts = AnalyzeOptions { static_resolution: true, decls: Some(&decls), lints: false, imports: None, record: false };
    let a = analyze(&root, &res, &opts);
    let index = nl_syntax::LineIndex::new(src);
    let codes: Vec<(u32, &str)> = a.diagnostics.iter().map(|d| (index.line_col(src, d.range.start()).line, d.code)).collect();
    assert!(codes.contains(&(4, "assign-declared-const")), "{codes:?}");
    assert!(codes.contains(&(6, "block-without-function-param")), "{codes:?}");
    // The block is the final positional argument, where `b: Number` is.
    assert!(codes.contains(&(6, "type-mismatch")), "{codes:?}");
    assert!(codes.contains(&(7, "type-mismatch")), "{codes:?}");
    assert_eq!(codes.len(), 4, "{codes:?}");
    // The Lua keys of the renamed members reach the lowering.
    let keys: Vec<String> = a.hints.members_declared.values().map(|h| h.key().to_string()).collect();
    assert!(keys.contains(&"ReturnLargest".to_string()) && keys.contains(&"coolnessFactor".to_string()), "{keys:?}");
}

/// `declare let` takes a capitalized name, as the host names its global:
/// the variable can be assigned, and a `declare const` cannot.
#[test]
fn declared_capitalized_let() {
    let decl = "declare let PLUGIN: Table?\ndeclare let SCHEMA: Number = :Schema\ndeclare const ENV: Table\n";
    let parse = nl_syntax::parse_decl_file(decl);
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
    let mut files = crate::decl::prelude_files();
    files.push(("host.d.nl".to_string(), parse.syntax()));
    let (decls, diags) = Declarations::load(&files);
    assert!(diags.is_empty(), "{diags:?}");
    let src = "PLUGIN = {name: \"x\"}\nSCHEMA = 3\nprint SCHEMA + 1\nENV = {}\n";
    let p = nl_syntax::parse(src);
    let root = p.syntax();
    let res = resolve_with(&root, &AnyImports, &decls);
    let index = nl_syntax::LineIndex::new(src);
    let codes: Vec<(u32, &str)> = res.diagnostics.iter().map(|d| (index.line_col(src, d.range.start()).line, d.code)).collect();
    assert_eq!(codes, vec![(4, "assign-declared-const")]);
}

/// The operators of a declared host type give the analyzer the result
/// types, through a parent too.
#[test]
fn declared_operators_type_results() {
    let decl = "declare type Vec\n  fn +(right: Vec): Vec\n  fn *(right: Number): Vec\n  fn -: Vec\nend\ndeclare type Pos : Vec\nend\ndeclare fn pos(): Pos = :pos\n";
    let parse = nl_syntax::parse_decl_file(decl);
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
    let mut files = crate::decl::prelude_files();
    files.push(("game.d.nl".to_string(), parse.syntax()));
    let (decls, diags) = Declarations::load(&files);
    assert!(diags.is_empty());
    let src = "let a: String = pos() + pos()\nlet b: String = pos() * 2\nlet c: String = -pos()\nlet d: Vec = pos() + pos()\nprint a, b, c, d\n";
    let p = nl_syntax::parse(src);
    let root = p.syntax();
    let res = resolve_with(&root, &AnyImports, &decls);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let opts = AnalyzeOptions { static_resolution: true, decls: Some(&decls), lints: false, imports: None, record: false };
    let a = analyze(&root, &res, &opts);
    let index = nl_syntax::LineIndex::new(src);
    let codes: Vec<(u32, &str)> = a.diagnostics.iter().map(|d| (index.line_col(src, d.range.start()).line, d.code)).collect();
    assert_eq!(codes, vec![(1, "type-mismatch"), (2, "type-mismatch"), (3, "type-mismatch")]);
}

/// `Time` and `Duration` are prelude library types (decl/nl.d.nl): their
/// functions, instance methods, keyed arguments and operators are typed
/// from the declarations, and a method of a known Time is a declared
/// method call.
#[test]
fn time_and_duration_are_declared_types() {
    let src = "let t = Time.at(0)\nlet d = 5.minutes\nlet a: String = t + d\nlet b: Duration = d * 2\nlet c: Number = t.year(utc: true)\nlet e: String = t.since(t)\nlet f = t.hour(utc: 1)\nlet g = t.min(zone: true)\nlet h: Time = Time.now\nlet i: any = t - t\nprint a, b, c, e, f, g, h, i\n";
    let errs: Vec<(u32, &str)> = analyzer_diagnostics(src).into_iter().filter(|d| d.3).map(|d| (d.0, d.1)).collect();
    assert_eq!(errs, vec![(3, "type-mismatch"), (6, "type-mismatch"), (7, "type-mismatch"), (8, "unknown-keyed-arg")]);
    let decls = prelude_declarations();
    let parse = nl_syntax::parse("let t = Time.at(0)\nprint t.epoch\n");
    let root = parse.syntax();
    let res = resolve_with(&root, &AnyImports, &decls);
    let opts = AnalyzeOptions { static_resolution: true, decls: Some(&decls), lints: false, imports: None, record: false };
    let a = analyze(&root, &res, &opts);
    let kinds: Vec<String> = a.hints.members_declared.values().map(|h| format!("{h:?}")).collect();
    assert!(kinds.iter().any(|k| k.starts_with("Function") && k.contains("\"at\"")), "{kinds:?}");
    assert!(kinds.iter().any(|k| k.starts_with("Method") && k.contains("\"epoch\"")), "{kinds:?}");
}

fn resolution(src: &str) -> (nl_syntax::SyntaxNode, Resolution) {
    let parse = nl_syntax::parse(src);
    assert!(!parse.has_errors(), "parse errors: {:?}", parse.diagnostics());
    let root = parse.syntax();
    let res = resolve(&root, &AnyImports);
    (root, res)
}

fn has_error(src: &str, line: u32, needle: &str) -> bool {
    errors(src).iter().any(|(l, m)| *l == line && m.contains(needle))
}

#[test]
fn duplicates_are_errors() {
    assert!(has_error("struct P\n  x: Number\n  x: String\nend\n", 3, "declared twice"));
    assert!(has_error("enum E\n  A\n  A\nend\n", 3, "declared twice"));
    assert!(has_error("fn f(a, b, a)\n  a\nend\n", 1, "declared twice"));
    assert!(has_error("fn f(a, {b, a})\n  a\nend\n", 1, "declared twice"));
    assert!(has_error("fn f(a, *a)\n  a\nend\n", 1, "declared twice"));
    // A default may hold a function with a parameter of the same name.
    assert!(errors("fn f(a, b = fn(a) a end)\n  a\nend\n").is_empty());
}

#[test]
fn a_fn_declaration_cannot_be_assigned() {
    assert!(has_error("fn foo\n  1\nend\nfoo = 5\n", 4, "declared with `fn`"));
    assert!(errors("let foo = fn() 1 end\nfoo = fn() 2 end\n").is_empty());
}

#[test]
fn when_items_bind_the_same_names() {
    let src = "fn f(v)\n  case v\n  when [x, 1], [1, x]\n    x\n  end\nend\n";
    assert!(errors(src).is_empty());
    let (root, res) = resolution(src);
    // Both `x` declarations are one binding.
    let ids: Vec<BindingId> = root
        .descendants()
        .filter(|n| n.kind() == nl_syntax::SyntaxKind::NAME && n.text() == "x")
        .filter_map(|n| res.declaration(n.text_range()))
        .collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], ids[1]);
    assert!(has_error("fn f(v)\n  case v\n  when [x, 1], [1, y]\n    x\n  end\nend\n", 3, "same names"));
    assert!(has_error("fn f(v)\n  case v\n  when 1, [x]\n    x\n  end\nend\n", 3, "same names"));
}

#[test]
fn blocks_on_module_functions() {
    let src = "module M\n  fn plain(x)\n    x\n  end\n  fn each(cb: Function)\n    cb()\n  end\nend\nM.each do\nend\nM.plain 1 do\nend\n";
    let errs = errors(src);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].0, 11);
    // Declared after the call, with `fn M.name`.
    assert!(has_error("module M\nend\nfn use\n  M.later 1 do\n  end\nend\nfn M.later(n)\n  n\nend\n", 4, "Function"));
}

#[test]
fn discarded_values_are_decided_here() {
    // Discarded means "a statement that is not last". The last statement of the
    // file, of a loop body and of a `: nil` function is last.
    let src = "let v = 5\nv\nfor i in 1..2\n  v\n  v\nend\nfn f: nil\n  v\nend\nv\n";
    let (root, res) = resolution(src);
    let discarded: Vec<u32> = root
        .descendants()
        .filter(|n| n.kind() == nl_syntax::SyntaxKind::EXPR_STMT)
        .filter(|n| res.value_discarded(n.text_range()))
        .map(|n| nl_syntax::LineIndex::new(src).line_col(src, n.text_range().start()).line)
        .collect();
    assert_eq!(discarded, vec![2, 4]);
}

#[test]
fn dotted_names_in_patterns() {
    let src = "enum Dir\n  North\nend\nmodule Geo\nend\nlet v = {}\nif let {d: Dir.North, p: Geo.P} = v\n  print p\nend\n";
    assert!(errors(src).is_empty(), "{:?}", errors(src));
    let (root, res) = resolution(src);
    let variants: Vec<String> = root
        .descendants()
        .filter(|n| n.kind() == nl_syntax::SyntaxKind::QUAL_NAME && res.is_enum_variant(n.text_range()))
        .map(|n| n.text().to_string())
        .collect();
    assert_eq!(variants, vec!["Dir.North".to_string()]);
}

#[test]
fn pattern_function_types_make_function_names() {
    let (_, res) = resolution("let {f: Function} = {}\nlet {g: g: Function} = {}\nlet [h: Function] = {}\nlet {k: Number} = {}\n");
    let kind = |name: &str| res.bindings.iter().find(|b| b.name == name).map(|b| b.kind);
    assert_eq!(kind("f"), Some(BindingKind::Function));
    assert_eq!(kind("g"), Some(BindingKind::Function));
    assert_eq!(kind("h"), Some(BindingKind::Function));
    assert_eq!(kind("k"), Some(BindingKind::Variable));
}

#[test]
fn self_of_a_nested_owner() {
    let src = "module Inv\nend\nmodule Inv.Filters\nend\nfn Inv.Filters.late\n  Self\nend\n";
    assert!(errors(src).is_empty(), "{:?}", errors(src));
}

#[test]
fn closures_that_assign() {
    let (_, res) = resolution("let a = 1\nlet b = 2\nfn f\n  a = 3\nend\nb = 4\n");
    let id = |name: &str| res.bindings.iter().position(|b| b.name == name).map(|i| BindingId(i as u32)).unwrap();
    assert!(res.assigned_in_closure(id("a")));
    assert!(!res.assigned_in_closure(id("b")));
}

/// An export has the binding kind its own file
/// gives it, decided once, by the resolver.
#[test]
fn exports_keep_the_kinds_of_their_file() {
    let (_, res) = resolution(
        "pub const f: Function? = fn() 1 end\npub const g = fn() 2 end\npub const h: Function = fn() 3 end\npub fn k(cb: Function)\n  cb\nend\npub type T = Number\npub module M\nend\n",
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let kinds: Vec<(String, BindingKind, DeclKind)> = res.exports().into_iter().map(|e| (e.name, e.kind, e.decl)).collect();
    assert_eq!(
        kinds,
        vec![
            ("f".to_string(), BindingKind::Variable, DeclKind::Const),
            ("g".to_string(), BindingKind::Function, DeclKind::Const),
            ("h".to_string(), BindingKind::Function, DeclKind::Const),
            ("k".to_string(), BindingKind::Function, DeclKind::Fn),
            ("T".to_string(), BindingKind::TypeLevel, DeclKind::TypeAlias),
            ("M".to_string(), BindingKind::TypeLevel, DeclKind::Module),
        ]
    );
    let k = res.exports().into_iter().find(|e| e.name == "k").and_then(|e| e.params);
    assert!(k.is_some_and(|p| p.has_function_param));
}

/// `pub` items in the branches of a file-level `if` are exports: one name
/// in several branches is one export, and it is optional unless a branch
/// that declares it always runs.
#[test]
fn conditional_exports() {
    let src = "let a = true\nif a\n  pub fn both\n    1\n  end\n  pub const only_if = 1\nelse\n  pub fn both\n    2\n  end\nend\nunless a\n  pub module Never\n  end\nend\nprint both, only_if\n";
    let (_, res) = resolution(src);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let got: Vec<(String, BindingKind, bool)> = res.exports().into_iter().map(|e| (e.name, e.kind, e.optional)).collect();
    assert_eq!(
        got,
        vec![
            ("both".to_string(), BindingKind::Function, false),
            ("only_if".to_string(), BindingKind::Variable, true),
            ("Never".to_string(), BindingKind::TypeLevel, true),
        ]
    );
    // The same name twice in one branch is still an error.
    assert_eq!(errors("let a = true\nif a\n  pub fn f\n    1\n  end\n  pub const f = 2\nend\n").len(), 1);
}

/// Under the gmod-client profile, `if CLIENT` always runs and `if SERVER`
/// never does: what the first declares is not optional for an importer.
#[test]
fn conditional_exports_in_the_client_realm() {
    let parse = nl_syntax::parse_decl_file(GMOD_DECL);
    let mut files = crate::decl::prelude_files();
    files.push(("gmod.d.nl".to_string(), parse.syntax()));
    let (decls, _) = Declarations::load(&files);
    let src = "if CLIENT\n  pub fn draw\n    1\n  end\nend\nif SERVER\n  pub fn serve\n    2\n  end\nelse\n  pub fn serve\n    3\n  end\nend\nif SERVER\n  pub const port = 1\nend\n";
    let p = nl_syntax::parse(src);
    let res = resolve_with(&p.syntax(), &AnyImports, &decls);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let optional = |exports: Vec<ExportInfo>| exports.into_iter().map(|e| (e.name, e.optional)).collect::<Vec<_>>();
    assert_eq!(optional(res.exports()), vec![("draw".into(), true), ("serve".into(), false), ("port".into(), true)]);
    assert_eq!(optional(profile::exports(&res, Profile::GmodClient)), vec![("draw".into(), false), ("serve".into(), false), ("port".into(), true)]);
}

/// Import and export errors, on their lines.
#[test]
fn import_and_export_errors() {
    let errs = errors("import { a } from 'x'\nfn a\n  1\nend\n");
    assert_eq!(errs.iter().map(|e| e.0).collect::<Vec<_>>(), vec![2], "{errs:?}");
    let errs = errors("import { a } from 'x'\nlet a = 1\n");
    assert_eq!(errs.iter().map(|e| e.0).collect::<Vec<_>>(), vec![2], "{errs:?}");
    // A local of a function may shadow an import (a lint).
    assert!(errors("import { a } from 'x'\nfn f\n  let a = 1\n  a\nend\n").is_empty());
    // Reopening an imported module is not a clash.
    assert!(errors("import { M } from 'x'\nmodule M\n  fn more\n    1\n  end\nend\n").is_empty());
    let errs = errors("import { a } from 'x'\nimport { a } from 'y'\n");
    assert_eq!(errs.iter().map(|e| e.0).collect::<Vec<_>>(), vec![2], "{errs:?}");
    let errs = errors("import { } from 'x'\n");
    assert_eq!(errs.len(), 1, "{errs:?}");
    let errs = errors("fn g<T>(a)\n  a is T?\nend\nfn h<T>(a: T?, b: [T])\n  a is Number and b is [T]\nend\n");
    assert_eq!(errs.iter().map(|e| e.0).collect::<Vec<_>>(), vec![2], "{errs:?}");
    let errs = errors("pub fn f\n  1\nend\npub const f = 2\n");
    assert_eq!(errs.iter().map(|e| e.0).collect::<Vec<_>>(), vec![4], "{errs:?}");
    let errs = errors("module A\nend\npub module A.B\nend\n");
    assert_eq!(errs.iter().map(|e| e.0).collect::<Vec<_>>(), vec![3], "{errs:?}");
}

/// callonly.rs: which parameters a function only calls, as its export
/// reports them (one bit per written parameter).
#[test]
fn call_only_parameters() {
    let src = "\
pub fn each(list: any, cb: Function)
  for v in list
    cb(v)
  end
end
pub fn apply(x: any, f: Function): any
  f(x)
end
pub fn outer(list: any, cb: Function)
  each(list, cb!)
end
pub fn nested(list: any, cb: Function)
  each list do |v|
    cb(v)
  end
end
pub fn kept(cb: Function)
  let f = cb!
  f()
end
pub fn stored(cb: Function)
  let h = [cb!]
  h.len
end
pub fn returned(cb: Function)
  cb!
end
pub fn optional(cb: Function?)
  if cb
    cb(1)
  end
end
pub fn optional_read(cb: Function?)
  let x = cb
  x
end
pub async fn later(cb: Function)
  cb(1)
end
pub fn to_lua(cb: Function)
  Table.sort([1], cb!)
end
pub fn rec(n: Number, cb: Function)
  if n > 0
    rec(n - 1, cb!)
  end
  cb(n)
end
pub fn mutual_a(cb: Function)
  mutual_b(cb!)
end
pub fn mutual_b(cb: Function)
  mutual_a(cb!)
  cb()
end
pub fn pattern({a}: any, cb: Function)
  cb(a)
end
pub fn untyped(x, cb)
  cb(x)
end
pub fn inner_fn(cb: Function)
  fn helper
    cb(1)
  end
  helper()
end
pub fn piped(x: any, cb: Function)
  x |> cb
end
pub fn inner_literal(list: any, cb: Function)
  let f = fn(v) cb(v) end
  each(list, f!)
end
pub fn async_block(list: any, cb: Function)
  each list async do |v|
    cb(v)
  end
end
";
    let (_, res) = resolution(src);
    let mask = |name: &str| res.exports().into_iter().find(|e| e.name == name).and_then(|e| e.params).map(|p| p.call_only);
    let expected = [
        ("each", 0b10),
        ("apply", 0b10),
        ("outer", 0b10),
        ("nested", 0b10),
        ("kept", 0),
        ("stored", 0),
        ("returned", 0),
        ("optional", 1),
        ("optional_read", 0),
        ("later", 0),
        ("to_lua", 0),
        ("rec", 0b10),
        ("mutual_a", 1),
        ("mutual_b", 1),
        ("pattern", 0b10),
        ("untyped", 0b10),
        ("inner_fn", 0),
        ("piped", 0b10),
        ("inner_literal", 0),
        ("async_block", 0),
    ];
    for (name, bits) in expected {
        assert_eq!(mask(name), Some(bits), "{name}");
    }
}

/// The portable profile reports `Regex`, `load_nl`, `add_path` and
/// `Worker.new` at their uses, and nothing in the full profile; a local
/// that shadows a prelude name, `Worker.run` and `Event` are fine.
#[test]
fn portable_profile() {
    let src = "let r = Regex.new(\"a\")\nload_nl(\"print 1\")\nadd_path(\"lib\")\nlet w = Worker.new(\"w.nl\")\nlet t = Worker.run(|x| -> x, 1)\nEvent.run(\"E\")\nfn f(load_nl)\n  load_nl\nend\nprint(r, w, t, f)\n";
    let parse = nl_syntax::parse(src);
    assert!(!parse.has_errors(), "parse errors: {:?}", parse.diagnostics());
    let root = parse.syntax();
    let res = resolve(&root, &AnyImports);
    let decls = Declarations::default();
    assert!(profile::check(&root, &res, &decls, Profile::Full).is_empty());
    let index = nl_syntax::LineIndex::new(src);
    let found: Vec<(u32, &str)> = profile::check(&root, &res, &decls, Profile::Portable).iter().map(|d| (index.line_col(src, d.range.start()).line, d.code)).collect();
    assert_eq!(found, [(1, "not-in-profile"), (2, "not-in-profile"), (3, "not-in-profile"), (4, "not-in-profile")]);
    // gmod-client keeps the portable profile's rules.
    let found: Vec<(u32, &str)> = profile::check(&root, &res, &decls, Profile::GmodClient).iter().map(|d| (index.line_col(src, d.range.start()).line, d.code)).collect();
    assert_eq!(found, [(1, "not-in-profile"), (2, "not-in-profile"), (3, "not-in-profile"), (4, "not-in-profile")]);
}

/// A small slice of gmod's declarations (decl/glua.d.nl), with realms in
/// its doc comments as the generator writes them.
const GMOD_DECL: &str = "## gmod's library `net`. https://wiki.facepunch.com/gmod/net
declare module Net = :net
  ## Realm: server. https://wiki.facepunch.com/gmod/net.Broadcast
  fn broadcast() as 'Broadcast'
  ## Realm: client. https://wiki.facepunch.com/gmod/net.SendToServer
  fn send_to_server() as 'SendToServer'
  ## Realm: shared. https://wiki.facepunch.com/gmod/net.Start
  fn start(name: String) as 'Start'
end
## gmod's class `Entity`. https://wiki.facepunch.com/gmod/Entity
declare type Entity
  ## Realm: shared. https://wiki.facepunch.com/gmod/Entity:GetPos
  fn get_pos(self): Number as 'GetPos'
  ## Realm: server. https://wiki.facepunch.com/gmod/Entity:Remove
  fn remove(self) as 'Remove'
end
## gmod's class `Player`. https://wiki.facepunch.com/gmod/Player
declare type Player : Entity
  ## Realm: server. https://wiki.facepunch.com/gmod/Player:Kick
  fn kick(self, reason: String?) as 'Kick'
  ## Realm: client. https://wiki.facepunch.com/gmod/Player:ShouldDrawLocalPlayer
  fn should_draw_local_player(self): Bool as 'ShouldDrawLocalPlayer'
end
## gmod's table of hooks `GM`. https://wiki.facepunch.com/gmod/GM_Hooks
declare type GM
  ## Realm: server. https://wiki.facepunch.com/gmod/GM:PlayerSpawn
  fn player_spawn(self, ply: Player) as 'PlayerSpawn'
end
## Realm: client. https://wiki.facepunch.com/gmod/Global.LocalPlayer
declare fn local_player(): Player = :LocalPlayer
## Realm: server. https://wiki.facepunch.com/gmod/Global.BroadcastLua
declare fn broadcast_lua(code: String) = :BroadcastLua
## Realm: menu. https://wiki.facepunch.com/gmod/Global.JoinServer
declare fn join_server(address: String) = :JoinServer
declare const GAMEMODE: GM
## Realm: shared. https://wiki.facepunch.com/gmod/Global_Variables
declare const SERVER: Bool
## Realm: shared. https://wiki.facepunch.com/gmod/Global_Variables
declare const CLIENT: Bool
";

/// The errors of `profile::check` and `profile::check_members` for a file
/// compiled with [`GMOD_DECL`] in the gmod-client profile, as (line, code).
fn gmod_client_errors(src: &str) -> Vec<(u32, &'static str, String)> {
    let parse = nl_syntax::parse_decl_file(GMOD_DECL);
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
    let mut files = crate::decl::prelude_files();
    files.push(("gmod.d.nl".to_string(), parse.syntax()));
    let (decls, diags) = Declarations::load(&files);
    assert!(diags.is_empty(), "{diags:?}");
    let p = nl_syntax::parse(src);
    assert!(!p.has_errors(), "{:?}", p.diagnostics());
    let root = p.syntax();
    let res = resolve_with(&root, &AnyImports, &decls);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let mut diags = profile::check(&root, &res, &decls, Profile::GmodClient);
    let opts = AnalyzeOptions { static_resolution: true, decls: Some(&decls), lints: false, imports: None, record: true };
    let a = analyze(&root, &res, &opts);
    assert!(!a.has_errors(), "{:?}", a.diagnostics);
    diags.extend(profile::check_members(&root, &res, a.info.as_ref().unwrap(), &decls, Profile::GmodClient));
    // Nothing outside gmod.
    assert!(profile::check(&root, &res, &decls, Profile::Portable).iter().all(|d| d.code == profile::CODE));
    let index = nl_syntax::LineIndex::new(src);
    let mut out: Vec<(u32, &'static str, String)> = diags.iter().filter(|d| d.error).map(|d| (index.line_col(src, d.range.start()).line, d.code, d.message.clone())).collect();
    out.sort_by_key(|(l, ..)| *l);
    out
}

fn lines_codes(errors: &[(u32, &'static str, String)]) -> Vec<(u32, &'static str)> {
    errors.iter().map(|(l, c, _)| (*l, *c)).collect()
}

/// What gmod's Lua state removes from LuaJIT's libraries, through prelude
/// names, Lua paths and `require`; what it keeps is fine.
#[test]
fn gmod_client_environment() {
    let src = "IO.write(\"x\")
let f = loadstring(\"return 1\")
let t = OS.time()
OS.getenv(\"HOME\")
:os.exit(1)
let c = OS.clock() + :os.clock()
Debug.setlocal(1, 1, 1)
let tb = Debug.traceback()
let p = Package.path
let l = Package.loaded
require(\"ffi\")
require \"table.new\"
:require(\"string.buffer\")
require(\"mymodule\")
let y = Coroutine.isyieldable()
let r = Coroutine.running()
:io.open(\"f\")
dofile(\"x.lua\")
print(f, t, c, tb, p, l, y, r)
";
    let errors = gmod_client_errors(src);
    assert_eq!(
        lines_codes(&errors),
        [(1, "not-in-gmod"), (2, "not-in-gmod"), (4, "not-in-gmod"), (5, "not-in-gmod"), (7, "not-in-gmod"), (9, "not-in-gmod"), (11, "not-in-gmod"), (12, "not-in-gmod"), (13, "not-in-gmod"), (15, "not-in-gmod"), (17, "not-in-gmod"), (18, "not-in-gmod")],
        "{errors:#?}"
    );
    assert!(errors[2].2.contains("`OS.getenv`") && errors[2].2.contains("`clock`, `date`, `difftime` and `time`"), "{}", errors[2].2);
    assert!(errors[9].2.contains("LuaJIT 2.0.4"), "{}", errors[9].2);
}

/// The realm of declared functions, members of modules and types (through
/// a parent), and Lua paths; the members of a table of hooks, and what the
/// client realm has, are fine.
#[test]
fn gmod_client_realms() {
    let src = "Net.start(\"msg\")
Net.broadcast()
Net.send_to_server()
let me = local_player()
me.kick(\"bye\")
me.remove()
let pos = me.get_pos()
let d = me.should_draw_local_player()
broadcast_lua(\"print(1)\")
join_server(\"127.0.0.1\")
:net.Broadcast()
:net.SendToServer()
GAMEMODE.player_spawn(me)
print(pos, d)
";
    let errors = gmod_client_errors(src);
    assert_eq!(
        lines_codes(&errors),
        [(2, "wrong-realm"), (5, "wrong-realm"), (6, "wrong-realm"), (9, "wrong-realm"), (10, "wrong-realm"), (11, "wrong-realm")],
        "{errors:#?}"
    );
    let kick = &errors[1].2;
    assert!(kick.contains("`Player.kick` exists only on the server") && kick.contains("`if SERVER`") && kick.contains("https://wiki.facepunch.com/gmod/Player:Kick"), "{kick}");
    // A member found on a parent is named on the receiver's type.
    assert!(errors[2].2.contains("`Player.remove`"), "{}", errors[2].2);
    assert!(errors[4].2.contains("only in the main menu"), "{}", errors[4].2);
}

/// Code that the realm constants leave dead in the client realm is not
/// checked: `if SERVER`, `elseif`/`else` after a branch always taken,
/// `unless CLIENT`, modifiers, `not`, `and`, `or`. A local constant named
/// `SERVER` decides nothing.
#[test]
fn gmod_client_dead_code() {
    let src = "if SERVER
  Net.broadcast()
  IO.write(\"x\")
elseif CLIENT
  Net.send_to_server()
else
  Net.broadcast()
end
if CLIENT
  print(1)
else
  Net.broadcast()
end
unless CLIENT
  Net.broadcast()
end
Net.broadcast() if SERVER
Net.broadcast() unless CLIENT
Net.broadcast() if SERVER and Net.start(\"x\")
Net.broadcast() if not (CLIENT or false)
if :SERVER
  Net.broadcast()
end
Net.broadcast() if CLIENT
fn f
  const SERVER = true
  Net.broadcast() if SERVER
end
Net.broadcast() if Net.start(\"y\") and SERVER
print(f)
";
    let errors = gmod_client_errors(src);
    assert_eq!(lines_codes(&errors), [(24, "wrong-realm"), (27, "wrong-realm"), (29, "wrong-realm")], "{errors:#?}");
}

/// The host types of declaration files come with their parents, every
/// parent before its children.
#[test]
fn host_types_parents_first() {
    let src = "declare type Player : Entity\nend\ndeclare type Weapon : Entity\nend\ndeclare type Entity\nend\ndeclare type Lone\nend\ndeclare struct Color\nend\n";
    let parse = nl_syntax::parse_decl_file(src);
    assert!(!parse.has_errors(), "{:?}", parse.diagnostics());
    let (decls, diags) = Declarations::load(&[("t.d.nl".to_string(), parse.syntax())]);
    assert!(diags.is_empty(), "{diags:?}");
    let got = decls.host_types();
    let names: Vec<(&str, Option<&str>)> = got.iter().map(|(n, p)| (n.as_str(), p.as_deref())).collect();
    assert_eq!(names, [("Entity", None), ("Lone", None), ("Player", Some("Entity")), ("Weapon", Some("Entity"))]);
}
