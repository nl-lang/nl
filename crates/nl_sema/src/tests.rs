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

/// The built-in declarations and those of the conformance suite
/// (`tests/run/decl/`), which its programs are compiled with.
fn suite_declarations() -> Declarations {
    let mut files = prelude_files();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(run_dir().join("decl")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(".d.nl")).collect();
    paths.sort();
    for p in paths {
        let src = std::fs::read_to_string(&p).unwrap();
        files.push((p.display().to_string(), nl_syntax::parse_decl_file(&src).syntax()));
    }
    Declarations::load(&files).0
}

/// The resolver's errors on a program of the conformance suite, with its
/// declarations.
fn suite_errors(src: &str) -> Vec<(u32, String)> {
    let parse = nl_syntax::parse(src);
    assert!(!parse.has_errors(), "parse errors: {:?}", parse.diagnostics());
    let decls = suite_declarations();
    let res = resolve_with(&parse.syntax(), &AnyImports, &decls);
    let index = nl_syntax::LineIndex::new(src);
    res.diagnostics.iter().filter(|d| d.error && d.code != "not-exported").map(|d| (index.line_col(src, d.range.start()).line, d.message.clone())).collect()
}

/// The files of tests/run, for the analyzer: an import path is found
/// beside the importing file, then in tests/run (where the entry programs
/// are), then in tests/run/reject (where the rejected ones are); exports
/// are what the file's resolver gives, and the primitive methods are
/// followed through the files' imports, as the driver does.
struct RunImports;

impl RunImports {
    fn find(from: Option<&str>, path: &str) -> Option<String> {
        let near = from.and_then(|f| Path::new(f).parent().map(|d| d.join(format!("{path}.nl"))));
        let top = [run_dir().join(format!("{path}.nl")), run_dir().join("reject").join(format!("{path}.nl"))];
        near.into_iter().chain(top).find(|c| c.is_file()).map(|c| c.to_string_lossy().into_owned())
    }
}

impl ImportProvider for RunImports {
    fn exports(&self, path: &str) -> Result<Vec<ExportInfo>, String> {
        let name = Self::find(None, path).ok_or_else(|| format!("cannot find '{path}'"))?;
        let text = std::fs::read_to_string(&name).map_err(|e| e.to_string())?;
        Ok(resolve(&nl_syntax::parse(&text).syntax(), &NoImports).exports())
    }

    fn primitive_methods(&self, path: &str) -> Vec<PrimMethod> {
        let scan = |name: &str| Some(scan_file(&nl_syntax::parse(&std::fs::read_to_string(name).ok()?).syntax()));
        primitive_methods_through(None, path, &Self::find, &scan)
    }
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
/// none (the pipeline's order), with the built-in declarations; imports
/// are the files of tests/run ([`RunImports`]).
fn analyzer_diagnostics(src: &str) -> Vec<(u32, &'static str, String, bool)> {
    analyzer_diagnostics_with(src, &prelude_declarations())
}

fn analyzer_diagnostics_with(src: &str, decls: &Declarations) -> Vec<(u32, &'static str, String, bool)> {
    let parse = nl_syntax::parse(src);
    assert!(!parse.has_errors(), "parse errors: {:?}", parse.diagnostics());
    let root = parse.syntax();
    let res = resolve_with(&root, &RunImports, decls);
    let index = nl_syntax::LineIndex::new(src);
    let line = |r: nl_syntax::TextRange| index.line_col(src, r.start()).line;
    let mut out: Vec<(u32, &'static str, String, bool)> = res.diagnostics.iter().filter(|d| d.code != "not-exported").map(|d| (line(d.range), d.code, d.message.clone(), d.error)).collect();
    if !res.has_errors() {
        let opts = AnalyzeOptions { static_resolution: true, decls: Some(decls), lints: true, imports: Some(&RunImports), record: false };
        let analysis = analyze(&root, &res, &opts);
        out.extend(analysis.diagnostics.iter().map(|d| (line(d.range), d.code, d.message.clone(), d.error)));
    }
    out
}

/// The analyzer's errors on a program of the conformance suite, with its
/// declarations.
fn suite_analyzer_errors(src: &str) -> Vec<(u32, String)> {
    analyzer_diagnostics_with(src, &suite_declarations()).into_iter().filter(|d| d.3).map(|d| (d.0, d.2)).collect()
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
        let errs = suite_errors(&src);
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
        let errs = suite_analyzer_errors(&src);
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
        let errs = suite_analyzer_errors(&src);
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
    // A member assigned, also by a compound assignment, names the function
    // as a read does, so the store is refused; a key is written in brackets.
    let place = "let t: Table = {}\nt.count = 1\nt.count += 1\nt.map ??= 2\n";
    assert!(table_fns(place, true).is_empty());
    let refused: Vec<u32> = analyzer_codes(place).into_iter().filter(|(_, c)| *c == "table-method-assign").map(|(l, _)| l).collect();
    assert_eq!(refused, vec![2, 3, 4]);
    assert!(analyzer_errors("let t: Table = {}\nt[\"count\"] = 1\nt[\"count\"] += 1\nprint t[\"count\"]\n").is_empty());
    // The arguments are checked against the function, receiver first.
    assert!(has_code("let xs: [Number] = [1]\nprint xs.take(\"2\")\n", 2, "type-mismatch"));
    assert!(has_code("fn Table.second\n  self[2]\nend\nprint Table.second(5)\n", 4, "type-mismatch"));
    assert!(analyzer_errors("fn Table.second\n  self[2]\nend\nprint Table.second([1, 2]), [1, 2].second\n").is_empty());
}

/// A member of a receiver typed `[T]`, `{[K]: V}` or `Table` that names a
/// function of `Table` is that function wherever the type comes from: an
/// index of a map, a union whose table parts agree. When the parts of a
/// union disagree, or a list member names no function of `Table`, nothing
/// would work at runtime (a table has no members but its keys), so it is an
/// error instead of a `nil` read.
#[test]
fn unresolved_table_members() {
    // `m["a"]` and `m.b` are the map's values: a record literal stored in
    // a variable declared a map keeps the map type.
    let src = "let m: {[String]: [Number]} = {a: [1, 2]}\nprint m[\"a\"].sum, m.a.sum, m.b.sum\n";
    assert_eq!(table_fns(src, true), vec![(2, "sum".to_string()); 3]);
    assert!(analyzer_errors(src).is_empty());
    // A union with a part that has no such function, or another one.
    assert!(has_code("fn r(v: [Number] | String) v.reverse end\nprint r([1])\n", 1, "ambiguous-member"));
    assert!(has_code("fn r(v: [Number] | String) v.reverse() end\nprint r([1])\n", 1, "ambiguous-member"));
    assert!(has_code("struct P\n  x: Number\nend\nfn r(v: [Number] | P) v.sum end\nprint r([1])\n", 4, "ambiguous-member"));
    // Narrowed, or table parts that agree: resolved.
    assert!(analyzer_errors("fn r(v: [Number] | String)\n  if v is String\n    v.reverse\n  else\n    v.reverse\n  end\nend\nprint r([1])\n").is_empty());
    assert!(analyzer_errors("fn r(v: [Number] | {[String]: Number}) v.size end\nprint r([1])\n").is_empty());
    // A name no function of `Table` has, on a list: reads, calls, `!`,
    // inferred lists; a function of `Table` that makes a table.
    assert!(has_code("let xs: [Number] = [1]\nprint xs.second\n", 2, "unknown-table-member"));
    assert!(has_code("let xs: [Number] = [1]\nprint xs.second(1)\n", 2, "unknown-table-member"));
    assert!(has_code("let xs: [Number] = [1]\nprint xs.second!\n", 2, "unknown-table-member"));
    assert!(has_code("let xs = [1, 2]\nprint xs.second\n", 2, "unknown-table-member"));
    assert!(has_code("fn f(xs: [Number]?) xs?.second end\nprint f(nil)\n", 1, "unknown-table-member"));
    assert!(has_code("let xs: [Number] = [1]\nprint xs.range\n", 2, "unknown-table-member"));
    // `n` of a rest table, whose count is not a key of it.
    assert!(has_code("fn f(*a: [Number]) a.n end\nprint f(1)\n", 1, "unknown-table-member"));
    // Defined in the file, it is known.
    assert!(analyzer_errors("fn Table.second\n  self[2]\nend\nlet xs: [Number] = [1]\nprint xs.second\n").is_empty());
    // On a `Table` or a map another name is a key (`n` of what
    // `Table.pack` makes); the options of `**opts` and a record's fields
    // are their keys.
    let keys = "\
let t: Table = {}
let m: {[String]: Number} = {}
print t.second, m.second
fn f(*a) Table.pack(*a).n end
fn g(**opts) opts.sum end
let r = {sum: 1}
print f(1), g(sum: 2), r.sum
";
    assert!(analyzer_errors(keys).is_empty());
}

/// An assignment to a member of a table-typed receiver that names a
/// function of `Table` is refused, as a read of it is the function.
#[test]
fn table_method_assignments() {
    assert!(has_code("let m: {[String]: Number} = {}\nm.count = 1\n", 2, "table-method-assign"));
    assert!(has_code("let xs: [Number] = []\nxs.map = 1\n", 2, "table-method-assign"));
    assert!(has_code("let c: Table = {}\nc.count ??= 1\n", 2, "table-method-assign"));
    assert!(has_code("let xs: [Number]? = []\nxs?.sum = 1\n", 2, "table-method-assign"));
    // A record's field, a `**opts` option, a key that names no function.
    let fine = "\
let r = {count: 1}
r.count = 2
r.count += 1
fn f(**opts)
  opts.count ??= 1
  opts.count
end
let t: Table = {}
t.color = 1
t.color += 1
print r.count, f(), t.color
";
    assert!(analyzer_errors(fine).is_empty());
}

/// A variable declared `Table` or a map keeps its declared type when it is
/// given a list, record or map literal: the declaration says how the table
/// is used, so its keys are keys (`t.foo`), not list members.
#[test]
fn declared_table_type_wins_over_a_literal() {
    let src = "\
let t: Table = [1, 2]
t.foo = 1
print t.foo, t.count
let r: Table = {a: 1}
r.foo = 2
print r.foo
let maybe: Table? = [1]
maybe.foo = 1
print maybe.foo
";
    assert_eq!(analyzer_errors(src), []);
    // An empty list given to a `Table` is a `Table` in the messages too;
    // undeclared, it is a list (not `[never]`).
    let msgs: Vec<String> = analyzer_diagnostics("let w: Table = []\nw.count = 1\nlet u = []\nu.count = 0\n").into_iter().filter(|d| d.1 == "table-method-assign").map(|d| d.2).collect();
    assert_eq!(msgs.len(), 2, "{msgs:?}");
    assert!(msgs[0].contains("on a value typed `Table`; write w[\"count\"]"), "{msgs:?}");
    assert!(msgs[1].contains("on a list; write u[\"count\"]"), "{msgs:?}");
    assert!(!msgs.iter().any(|m| m.contains("never")), "{msgs:?}");
}

/// A receiver too long to repeat is not given a made-up name in the
/// advice: the brackets alone.
#[test]
fn table_member_advice_for_long_receivers() {
    let src = "\
let lists_of_numbers_by_name: {[String]: [Number]} = {}
lists_of_numbers_by_name[\"a\"] = [1]
lists_of_numbers_by_name[\"a\"].count = 1
print lists_of_numbers_by_name[\"a\"].foo
";
    let msgs: Vec<(u32, &str, String)> = analyzer_diagnostics(src).into_iter().filter(|d| d.3).map(|d| (d.0, d.1, d.2)).collect();
    assert_eq!(msgs.iter().map(|m| (m.0, m.1)).collect::<Vec<_>>(), [(3, "table-method-assign"), (4, "unknown-table-member")], "{msgs:?}");
    assert!(msgs[0].2.contains("index the key with brackets, `[\"count\"]`, to use it as a key"), "{msgs:?}");
    assert!(msgs[1].2.contains("read the key with brackets, `[\"foo\"]`"), "{msgs:?}");
    assert!(!msgs.iter().any(|m| m.2.contains("x[")), "{msgs:?}");
}

/// `fn Number.round` and the like: a name the library already has (or an
/// imported file defines) is refused at load time, so it is an error here.
#[test]
fn library_methods_are_not_redefined() {
    assert!(has_code("fn Number.round\n  1\nend\n", 1, "library-method-redefined"));
    assert!(has_code("fn Number.floor\n  1\nend\n", 1, "library-method-redefined"));
    assert!(has_code("fn Number.minutes\n  1\nend\n", 1, "library-method-redefined"));
    assert!(has_code("fn Number.pi\n  1\nend\n", 1, "library-method-redefined"));
    assert!(has_code("fn String.wrap\n  1\nend\n", 1, "library-method-redefined"));
    assert!(has_code("fn String.upcase\n  1\nend\n", 1, "library-method-redefined"));
    assert!(has_code("fn Table.map\n  1\nend\n", 1, "library-method-redefined"));
    assert!(has_code("interface Loud\n  fn upcase\nend\ninterface Loud for String\n  fn upcase\n    self\n  end\nend\n", 5, "library-method-redefined"));
    // New names, and operators, are fine.
    assert!(analyzer_errors("fn Number.is_cool\n  self == 7\nend\nfn String.shout\n  self\nend\nfn Table.second\n  self[2]\nend\nfn Bool.flip\n  !self\nend\nfn String.+(o)\n  self\nend\n").is_empty());
}

/// The hints of a source in a mode, and the lines of the given ranges.
fn hints_of(src: &str, static_resolution: bool) -> Hints {
    let parse = nl_syntax::parse(src);
    assert!(!parse.has_errors(), "parse errors: {:?}", parse.diagnostics());
    let decls = prelude_declarations();
    let root = parse.syntax();
    let res = resolve_with(&root, &AnyImports, &decls);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let opts = AnalyzeOptions { static_resolution, decls: Some(&decls), lints: true, imports: None, record: false };
    let analysis = analyze(&root, &res, &opts);
    if static_resolution { analysis.hints } else { analysis.hints.without_static_resolution() }
}

fn lines_of<'a>(src: &str, ranges: impl Iterator<Item = &'a nl_syntax::TextRange>) -> Vec<u32> {
    let index = nl_syntax::LineIndex::new(src);
    let mut out: Vec<u32> = ranges.map(|r| index.line_col(src, r.start()).line).collect();
    out.sort();
    out.dedup();
    out
}

/// Reads whose receiver is certain to be a table (`Hints::table_reads`):
/// declared tables, lists, maps and records, not an untyped value or a
/// string; none without static resolution.
#[test]
fn table_reads_of_certain_tables() {
    let src = "\
fn f(t: Table, xs: [Number], m: {[String]: Number}, r: {a: Number}, o: Table?, u: any, s: String)
  print t.a, xs[1], xs.len, m[\"k\"], m.k, r.a
  print o?.a, u.a, u[1], s.len, s[1]
  print t.a!
end
";
    assert_eq!(lines_of(src, hints_of(src, true).table_reads.iter()), vec![2, 3, 4]);
    let h = hints_of(src, true);
    // Six reads on line 2, `o?.a` (under its guard `o` is a table) on
    // line 3, `t.a!` on line 4.
    assert_eq!(h.table_reads.len(), 8, "{:?}", h.table_reads);
    assert!(hints_of(src, false).table_reads.is_empty());
}

/// A return of a call of an NL function declared to return a type whose
/// check implies the caller's is proven; an inferred return type, a
/// record that checks its fields more strictly, or a reassigned function
/// is not.
#[test]
fn returned_checked_calls_are_proven() {
    let src = "\
fn half(x: Number): Number
  x / 2
end
fn loose(x)
  x
end
fn any_x(v: any): {x: any}
  v
end
fn a(x: Number): Number
  half(x)
end
fn b(x: Number): Number | String
  return half(x)
end
fn c(x: Number): Number
  loose(x)
end
fn d(v: any): {x: Number}
  any_x(v)
end
fn e(v: any): {x: any}
  any_x(v)
end
";
    // The tail of `a`, the return of `b`, the tail of `e` (the same type);
    // `half`'s own `x / 2` is proven too.
    let proven = lines_of(src, hints_of(src, true).proven.iter());
    assert!(proven.contains(&11) && proven.contains(&14) && proven.contains(&23), "{proven:?}");
    assert!(!proven.contains(&17) && !proven.contains(&20), "{proven:?}");
    assert!(hints_of(src, false).proven.is_empty());
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

/// Declarations for the tests of `fn Decl.name`: a module, a Lua class, a
/// host type with a parent, a function and a type nested in a module.
fn extension_declarations() -> Declarations {
    let decl = "declare module Util = :util\n  fn width(s: String): Number\nend\ndeclare struct Color = :Color\n  r: Number\nend\ndeclare type Entity\n  fn get_class(self): String as 'GetClass'\nend\ndeclare type Player : Entity\n  fn nick(self): String as 'Nick'\nend\ndeclare fn me(): Player = :LocalPlayer\ndeclare module Nest = :nest\n  type Inner\n  end\nend\n";
    let parse = nl_syntax::parse_decl_file(decl);
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
    let mut files = crate::decl::prelude_files();
    files.push(("game.d.nl".to_string(), parse.syntax()));
    let (decls, diags) = Declarations::load(&files);
    assert!(diags.is_empty(), "{diags:?}");
    decls
}

/// The resolver's and the analyzer's codes, by line, with
/// [`extension_declarations`] and the imports given.
fn extension_codes(src: &str, imports: &dyn ImportProvider) -> Vec<(u32, &'static str)> {
    let decls = extension_declarations();
    let parse = nl_syntax::parse(src);
    assert!(!parse.has_errors(), "parse errors: {:?}", parse.diagnostics());
    let root = parse.syntax();
    let res = resolve_with(&root, imports, &decls);
    let index = nl_syntax::LineIndex::new(src);
    let line = |r: nl_syntax::TextRange| index.line_col(src, r.start()).line;
    let mut out: Vec<(u32, &'static str)> = res.diagnostics.iter().map(|d| (line(d.range), d.code)).collect();
    if !res.has_errors() {
        let opts = AnalyzeOptions { static_resolution: true, decls: Some(&decls), lints: false, imports: None, record: false };
        out.extend(analyze(&root, &res, &opts).diagnostics.iter().map(|d| (line(d.range), d.code)));
    }
    out
}

/// `fn Util.name` adds a plain function to a declared module (no `self`),
/// `fn Player.name` a method to a declared type; the analyzer knows both,
/// with their signatures.
#[test]
fn extending_declared_modules_and_types() {
    let src = "fn Util.twice(s: String): String\n  s <> s\nend\nfn Player.tag(prefix: String): String\n  prefix <> self.nick\nend\nfn Color.bright: Number\n  self.r * 2\nend\nlet a: String = Util.twice(\"x\")\nlet b: String = me().tag(\"@\")\nprint a, b\n";
    assert_eq!(extension_codes(src, &AnyImports), []);
    // The signatures are checked at calls.
    assert_eq!(extension_codes("fn Util.twice(s: String): String\n  s\nend\nprint Util.twice(5)\n", &AnyImports), [(4, "type-mismatch")]);
    assert_eq!(extension_codes("fn Player.tag(p: String): String\n  p\nend\nlet n: Number = me().tag(\"@\")\nprint n\n", &AnyImports), [(4, "type-mismatch")]);
    // A function of a module has no `self`.
    assert!(extension_codes("fn Util.bad\n  self\nend\n", &AnyImports).iter().any(|(l, _)| *l == 2));
    // The same file may define a name again: it replaces the first.
    assert_eq!(extension_codes("fn Util.f\n  1\nend\nfn Util.f\n  2\nend\n", &AnyImports), []);
}

#[test]
fn extending_declared_names_is_checked() {
    // A member of the declaration, or of a parent of a host type.
    assert_eq!(extension_codes("fn Util.width(s)\n  1\nend\n", &AnyImports), [(1, "declared-member-redefined")]);
    assert_eq!(extension_codes("fn Player.get_class\n  \"x\"\nend\n", &AnyImports), [(1, "declared-member-redefined")]);
    // No metatable: a type nested in a declaration.
    assert_eq!(extension_codes("fn Nest.Inner.f\nend\n", &AnyImports), [(1, "declared-owner")]);
    // Not a module or a type; operators.
    assert_eq!(extension_codes("fn Nest.Missing.f\nend\n", &AnyImports), [(1, "declared-owner")]);
    assert_eq!(extension_codes("fn Player.+(o)\n  o\nend\n", &AnyImports), [(1, "operator-owner")]);
    assert_eq!(extension_codes("fn Util.len\n  1\nend\n", &AnyImports), [(1, "operator-owner")]);
}

/// An import that brings the functions its file adds to declared types.
struct ExtendingImport;

impl ImportProvider for ExtendingImport {
    fn exports(&self, _path: &str) -> Result<Vec<ExportInfo>, String> {
        Ok(vec![ExportInfo { name: "ready".into(), kind: BindingKind::Variable, params: None, decl: DeclKind::Const, optional: false }])
    }
    fn extensions(&self, _path: &str) -> Vec<ExtensionInfo> {
        vec![ExtensionInfo { owner: "Player".into(), name: "tag".into(), method: true, params: vec![("prefix".into(), false)], rest: false, keyed: false, is_async: false, origin: None }]
    }
}

/// What an imported file adds to a declared type is known in the
/// importing file, and passed on to the files that import it; defining
/// it again is an error.
#[test]
fn imported_extensions() {
    // The member is resolved from the declared type, as the declaration's
    // own are: a method call under its name.
    let src = "import { ready } from 'tags'\nprint ready, me().tag(\"@\")\n";
    assert_eq!(extension_codes(src, &ExtendingImport), []);
    let decls = extension_declarations();
    let parse = nl_syntax::parse(src);
    let root = parse.syntax();
    let res = resolve_with(&root, &ExtendingImport, &decls);
    let analysis = analyze(&root, &res, &AnalyzeOptions { decls: Some(&decls), ..Default::default() });
    let hinted = root.descendants().filter_map(|n| analysis.hints.member(n.text_range()).cloned()).collect::<Vec<_>>();
    assert!(hinted.contains(&MemberHint::Method { key: "tag".into() }), "{hinted:?}");
    let parse = nl_syntax::parse("import { ready } from 'tags'\nfn Util.f\nend\nprint ready\n");
    let res = resolve_with(&parse.syntax(), &ExtendingImport, &decls);
    let got: Vec<(String, String, Option<String>)> = res.extensions().into_iter().map(|e| (e.owner, e.name, e.origin)).collect();
    assert_eq!(got, [("Player".into(), "tag".into(), Some("tags".into())), ("Util".into(), "f".into(), None)]);
    let again = "import { ready } from 'tags'\nfn Player.tag(p)\n  p\nend\nprint ready\n";
    assert_eq!(extension_codes(again, &ExtendingImport), [(2, "declared-member-redefined")]);
}

/// A `spawn do` block that reads the count of an enclosing function's
/// rest parameter (`args.len`, `*args`) is refused, naming the parameter;
/// its items, and a rest parameter of a function inside the block, are not.
#[test]
fn spawn_block_rest_count() {
    let codes = |body: &str| analyzer_codes(&format!("fn f(*args)\n  let t = spawn do\n    {body}\n  end\n  t\nend\nprint f(1)\n"));
    assert!(codes("args.len").contains(&(3, "spawn-rest-count")));
    assert!(codes(":print(*args)").contains(&(3, "spawn-rest-count")));
    assert!(codes("[0, *args].len").contains(&(3, "spawn-rest-count")));
    assert!(!codes("args[1]").iter().any(|(_, c)| *c == "spawn-rest-count"));
    assert!(!codes("(fn(*b) b.len end)(*[1])").iter().any(|(_, c)| *c == "spawn-rest-count"));
    let errs = analyzer_errors("fn f(*rest)\n  let t = spawn do\n    rest.len\n  end\n  t\nend\nprint f(1)\n");
    assert!(errs.iter().any(|(_, m)| m.contains("rest parameter `rest`")), "{errs:?}");
}

/// `@m!` in a method is `self.m!`: a method of the struct comes bound,
/// with a hint for the lowering and a type that takes no `self`; a field
/// is its value.
#[test]
fn at_bang_binds_methods() {
    let src = "struct Box\n  v: Number\n  cb: Function?\nend\nfn Box.get(x: Number): String\n  \"#{@v}:#{x}\"\nend\nfn Box.both\n  let f = @get!\n  let c = @cb!\n  print c\n  f(1)\nend\nlet b = Box { v: 1 }\nprint b.both\n";
    assert!(analyzer_errors(src).is_empty(), "{:?}", analyzer_errors(src));
    let parse = nl_syntax::parse(src);
    let root = parse.syntax();
    let res = resolve(&root, &AnyImports);
    let analysis = analyze(&root, &res, &AnalyzeOptions { static_resolution: true, ..Default::default() });
    let hinted: Vec<MemberHint> = root.descendants().filter(|n| n.kind() == nl_syntax::SyntaxKind::AT_EXPR).filter_map(|n| analysis.hints.member(n.text_range()).cloned()).collect();
    assert_eq!(hinted, [MemberHint::Method { key: "get".into() }]);
    assert!(has_code(&src.replace("f(1)", "f(\"x\")"), 12, "type-mismatch"));
}

/// `global fn`, `global module` and `global struct` are exports, and no
/// lint calls them a use of a global.
#[test]
fn global_items_are_exported() {
    let src = "global fn class_(n)\n  n\nend\nglobal module Registry\nend\nglobal struct Point\n  x: Number\nend\n";
    let parse = nl_syntax::parse(src);
    assert!(!parse.has_errors(), "{:?}", parse.diagnostics());
    let res = resolve(&parse.syntax(), &AnyImports);
    let names: Vec<(String, DeclKind)> = res.exports().into_iter().map(|e| (e.name, e.decl)).collect();
    assert_eq!(names, [("class_".into(), DeclKind::Fn), ("Registry".into(), DeclKind::Module), ("Point".into(), DeclKind::Struct)]);
    assert_eq!(analyzer_codes(src), []);
    // Where `pub` cannot stand, `global` cannot either.
    assert!(nl_syntax::parse("fn f\n  global fn g\n  end\nend\n").has_errors());
    assert!(nl_syntax::parse("struct S\nend\nglobal fn S.m\nend\n").has_errors());
}

/// `M.f!` of a module and `obj.m!` of a struct are bound: their type is
/// a function without `self`.
#[test]
fn bang_of_a_method_is_bound() {
    let src = "struct P\n  n: Number = 0\nend\nfn P.add(k: Number): Number\n  @n + k\nend\nlet p = P {}\nlet f = p.add!\nlet n: Number = f(1)\nprint n\nprint f(\"x\")\n";
    assert_eq!(analyzer_codes(src), [(11, "type-mismatch")]);
}

// ---- user feedback: autocalls, `when` names, declarations, imports --------

/// The resolver's and the analyzer's codes, by line, for a program with a
/// declaration file beside the built-in ones, and the analysis (lints on).
fn with_decl(decl: &str, src: &str) -> (Vec<(u32, &'static str)>, Analysis) {
    let parse = nl_syntax::parse_decl_file(decl);
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
    let mut files = prelude_files();
    files.push(("host.d.nl".to_string(), parse.syntax()));
    let (decls, diags) = Declarations::load(&files);
    assert!(diags.is_empty(), "{diags:?}");
    let p = nl_syntax::parse(src);
    assert!(!p.has_errors(), "{:?}", p.diagnostics());
    let root = p.syntax();
    let res = resolve_with(&root, &AnyImports, &decls);
    let opts = AnalyzeOptions { static_resolution: true, decls: Some(&decls), lints: true, imports: None, record: false };
    let a = analyze(&root, &res, &opts);
    let index = nl_syntax::LineIndex::new(src);
    let line = |r: nl_syntax::TextRange| index.line_col(src, r.start()).line;
    let codes = res.diagnostics.iter().chain(a.diagnostics.iter()).filter(|d| d.code != "not-exported").map(|d| (line(d.range), d.code)).collect();
    (codes, a)
}

fn declared_hints(a: &Analysis) -> Vec<String> {
    a.hints.members_declared.values().map(|h| format!("{h:?}")).collect()
}

/// A bare function-kind name passed as an argument is called with no
/// arguments: a lint when the function has a required parameter, nothing
/// for one without, and the warning `fn-kind-argument` alone where the
/// parameter takes a function.
#[test]
fn autocall_argument_lint() {
    assert!(has_code("fn f(x)\n  x\nend\nprint typeof(f)\n", 4, "autocall-argument"));
    assert!(has_code("fn f(x: Number): Number\n  x\nend\nprint(k: f)\n", 4, "autocall-argument"));
    let quiet = ["fn now\n  1\nend\nprint now\n", "fn f(x = 1)\n  x\nend\nprint f\n", "fn f(x: Number?)\n  x\nend\nprint f\n", "fn f(x)\n  x\nend\nprint typeof(f!)\n"];
    for src in quiet {
        assert!(analyzer_codes(src).iter().all(|(_, c)| *c != "autocall-argument"), "{src}: {:?}", analyzer_codes(src));
    }
    let codes = analyzer_codes("fn reg(cb: Function)\n  cb\nend\nfn cb(x)\n  x\nend\nreg cb\n");
    assert_eq!(codes.iter().filter(|(l, _)| *l == 7).map(|(_, c)| *c).collect::<Vec<_>>(), ["fn-kind-argument"], "{codes:?}");
    // A declared function is a bare call too.
    let (codes, _) = with_decl("declare fn get_name(id: Number): String = :GetName\n", "print get_name\n");
    assert_eq!(codes, [(1, "autocall-argument")]);
}

/// `when Name` with a struct, enum, interface or library type compares
/// with the type's table: an error, whose fix is `when is Name`.
#[test]
fn when_type_name_is_an_error() {
    let case = |item: &str| format!("struct S\n  x: Number\nend\nenum E\n  A\nend\nfn f(v)\n  case v\n  when {item}\n    1\n  else\n    2\n  end\nend\nprint f(1)\n");
    for item in ["S", "E", "String", "Exception"] {
        assert!(has_code(&case(item), 9, "when-type-compare"), "{item}: {:?}", analyzer_codes(&case(item)));
    }
    for item in ["is S", "S {x}", "E.A", "is String", "\"S\"", "1"] {
        assert!(analyzer_errors(&case(item)).is_empty(), "{item}: {:?}", analyzer_errors(&case(item)));
    }
}

/// A module nested in a declared module is declared too: its methods are
/// Lua method calls, its functions plain ones, each under its Lua key, and
/// the nested module's own `as` key is the key it is read with.
#[test]
fn nested_declared_modules() {
    let decl = "declare module Top = :Top\n  module Sub\n    fn meth(self, x: String): String\n    fn plain(x: String): String as 'Plain'\n  end\n  module Inner as 'InnerLua'\n    fn hello(): String as 'Hello'\n  end\nend\n";
    let (codes, a) = with_decl(decl, "print Top.Sub.meth(\"a\")\nprint Top.Sub.plain(\"b\")\nprint Top.Inner.hello()\n");
    assert!(codes.is_empty(), "{codes:?}");
    let hints = declared_hints(&a);
    for want in ["Method { key: \"meth\" }", "Function { key: \"Plain\" }", "Field { key: \"InnerLua\" }", "Function { key: \"Hello\" }"] {
        assert!(hints.iter().any(|h| h == want), "{want} not in {hints:?}");
    }
    // The key is written with `as`; a Lua path is for the top level.
    assert!(nl_syntax::parse_decl_file("declare module T = :T\n  module S = :s\n  end\nend\n").has_errors());
}

/// A member of an optional declared type is the type's member, under its
/// Lua key, and a declared `const` narrows in a test (a declared `let`
/// that the file assigns does not).
#[test]
fn optional_declared_types_and_narrowing() {
    let decl = "declare type Panel\n  fn remove(self) as 'Remove'\nend\ndeclare fn find(): Panel? = :Find\ndeclare const PACKAGE: Panel?\ndeclare let CURRENT: Panel?\n";
    let (codes, a) = with_decl(decl, "let p = find()\np.remove() if p\nprint p.remove\n");
    assert!(codes.is_empty(), "{codes:?}");
    assert!(declared_hints(&a).iter().any(|h| h == "Method { key: \"Remove\" }"), "{:?}", declared_hints(&a));
    let src = "fn need(p: Panel)\n  p.remove\nend\nif PACKAGE\n  need(PACKAGE)\nend\nneed(PACKAGE) if PACKAGE != nil\nneed(PACKAGE)\nif CURRENT\n  need(CURRENT)\nend\nCURRENT = nil\n";
    let (codes, _) = with_decl(decl, src);
    assert_eq!(codes, [(8, "type-mismatch"), (10, "type-mismatch")]);
}

/// A member declared twice in a declaration is an error; `fn -` and
/// `fn -(right)` are two operators.
#[test]
fn duplicate_declared_members() {
    let src = "declare module Dup = :Dup\n  fn a(x: String)\n  fn a(self, y: Number): Number\n  module In\n    b: Number\n    fn b(): Number\n  end\nend\ndeclare type V\n  fn -: V\n  fn -(r: V): V\n  fn +(r: V): V\n  fn +(r: Number): V\nend\n";
    let parse = nl_syntax::parse_decl_file(src);
    assert!(!parse.has_errors(), "{:?}", parse.diagnostics());
    let (_, diags) = Declarations::load(&[("d.d.nl".to_string(), parse.syntax())]);
    let index = nl_syntax::LineIndex::new(src);
    let got: Vec<(u32, &str)> = diags.iter().map(|(_, d)| (index.line_col(src, d.range.start()).line, d.code)).collect();
    assert_eq!(got, [(3, "duplicate-member"), (6, "duplicate-member"), (13, "duplicate-member")]);
}

/// A local named like a declared value or module, or an import, hides
/// it: a lint. A prelude name has its own rules and is not reported here,
/// nor is a declared function (a host declares many lowercase ones, as
/// gmod does `player` and `color`), nor a parameter unless it hides a
/// declared value.
#[test]
fn shadowing_declared_and_imported_names() {
    let (codes, _) = with_decl("declare const t: Table = :t\n", "fn f(i)\n  let t = i\n  t\nend\nlet print_it = 1\nprint f(print_it)\n");
    assert_eq!(codes, [(2, "shadows-declared")]);
    let host = "\
declare fn player(i: Number): Table = :Player
declare fn color(r: Number, g: Number, b: Number): Table = :Color
declare fn entity(i: Number): Table = :Entity
declare const vector_origin: Table = :vector_origin
";
    let src = "\
fn describe(player, color, entity)
  let sound = player
  \"#{player} #{color} #{entity} #{sound}\"
end
fn moved(vector_origin)
  let player = vector_origin
  player
end
fn kept(v)
  let vector_origin = v
  vector_origin
end
print describe(1, 2, 3), moved(4), kept(5)
";
    let (codes, _) = with_decl(host, src);
    let shadows: Vec<u32> = codes.iter().filter(|(_, c)| *c == "shadows-declared").map(|(l, _)| *l).collect();
    assert_eq!(shadows, [5, 10], "{codes:?}");
    assert!(has_code("import { helper } from 'x'\nfn f\n  let helper = 2\n  helper\nend\nprint f\n", 3, "shadows-declared"));
}

/// A definition named like a declared module (gmod's `team` library is
/// `Team`, and a script's `enum Team` is its own) does not shadow it in
/// the lint's sense; a `const` of that name still does.
#[test]
fn definitions_named_like_a_declared_module() {
    let host = "declare module Team = :team\n  fn get_name(i: Number): String as 'GetName'\nend\n";
    let src = "\
enum Team
  Red
  Blue
end
fn f
  struct Player
    team: Team
  end
  interface Named
    fn name: String
  end
  Player { team: Team.Red }
end
print f
";
    let (codes, _) = with_decl(host, src);
    assert!(!codes.iter().any(|(_, c)| *c == "shadows-declared"), "{codes:?}");
    for kind in ["struct Team\n  a: Number\nend\nprint Team { a: 1 }\n", "module Team\n  fn a\n    1\n  end\nend\nprint Team.a\n", "interface Team\n  fn a: Number\nend\n"] {
        let (codes, _) = with_decl(host, kind);
        assert!(!codes.iter().any(|(_, c)| *c == "shadows-declared"), "{kind}: {codes:?}");
    }
    let (codes, _) = with_decl(host, "const Team = 1\nprint Team\n");
    assert!(codes.contains(&(1, "shadows-declared")), "{codes:?}");
}

/// Methods and operators that an imported file defines on primitives are
/// known to the importer, from what `scan_file` reads.
#[test]
fn primitive_methods_of_imports() {
    let scanned = scan_file(&nl_syntax::parse("import { a } from 'b'\nfn String.+(r: String): String\n  r\nend\nfn Number.half: Number\n  self / 2\nend\ninterface I for Bool\n  fn flip: Bool\n    not self\n  end\nend\nprint a\n").syntax());
    assert_eq!(scanned.imports, ["b"]);
    let got: Vec<(&str, &str, Option<&str>)> = scanned.methods.iter().map(|m| (m.owner, m.name.as_str(), m.ret)).collect();
    assert_eq!(got, [("String", "+", Some("String")), ("Number", "half", Some("Number")), ("Bool", "flip", Some("Bool"))]);

    struct Ops;
    impl ImportProvider for Ops {
        fn exports(&self, _path: &str) -> Result<Vec<ExportInfo>, String> {
            Ok(Vec::new())
        }
        fn primitive_methods(&self, _path: &str) -> Vec<PrimMethod> {
            vec![PrimMethod { owner: "String", name: "+".into(), ret: Some("String") }]
        }
    }
    let src = "import { x } from 'ops'\nlet s: Number = \"a\" + \"b\"\nprint s, x\n";
    let root = nl_syntax::parse(src).syntax();
    let decls = prelude_declarations();
    let res = resolve_with(&root, &Ops, &decls);
    let codes = |imports: Option<&dyn ImportProvider>| {
        let opts = AnalyzeOptions { static_resolution: true, decls: Some(&decls), lints: false, imports, record: false };
        analyze(&root, &res, &opts).diagnostics.iter().map(|d| d.code).collect::<Vec<_>>()
    };
    assert_eq!(codes(None), ["bad-operand"]);
    // Known, with its return type.
    assert_eq!(codes(Some(&Ops)), ["type-mismatch"]);
    // Through a chain of imports, each file once, a cycle included.
    let files: std::collections::HashMap<&str, &str> =
        [("a", "import { b } from 'b'\nfn String.one\n  1\nend\n"), ("b", "import { a } from 'a'\nimport { c } from 'c'\n"), ("c", "fn Table.two\n  2\nend\n")].into_iter().collect();
    let found = primitive_methods_through(Some("main"), "a", &|_, p| files.contains_key(p).then(|| p.to_string()), &|name| Some(scan_file(&nl_syntax::parse(files[name]).syntax())));
    let names: Vec<&str> = found.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["one", "two"]);
}

