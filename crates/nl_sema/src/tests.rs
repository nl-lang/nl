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
    for name in ["Number", "String", "Table", "IO", "OS", "Event", "Worker", "Task", "Regex", "print", "raise", "File"] {
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
