//! The hidden function of every `spawn do` block is made and
//! registered when its file loads (lift.rs), so that a worker that loads
//! the file finds it by name. Every block of these programs either becomes
//! an `rt.register` outside the code that runs the `spawn`, or the file
//! fails to compile with a `spawn-block` diagnostic; none is left written
//! where the `spawn` stands.

use nl_ir::ir::{Block, Expr, Function, Place, StmtKind, TableEntry};
use nl_ir::{LowerOptions, Localize, lower};

#[derive(Default)]
struct Count {
    /// `spawn do` blocks whose function is still written at the `spawn`.
    inline: usize,
    /// `rt.register` of a `<spawn do …>` name.
    registered: usize,
    /// `spawn do` blocks.
    spawns: usize,
}

fn walk_fn(f: &Function, c: &mut Count) {
    walk_block(&f.body, c);
}

fn walk_block(b: &Block, c: &mut Count) {
    for s in &b.stmts {
        match &s.kind {
            StmtKind::Local { values, .. } => values.iter().for_each(|e| walk(e, c)),
            StmtKind::Assign { targets, values } => {
                for t in targets {
                    if let Place::Index(o, k) = t {
                        walk(o, c);
                        walk(k, c);
                    }
                }
                values.iter().for_each(|e| walk(e, c));
            }
            StmtKind::Expr(e) => walk(e, c),
            StmtKind::Do(b) => walk_block(b, c),
            StmtKind::If { cond, then, otherwise } => {
                walk(cond, c);
                walk_block(then, c);
                if let Some(o) = otherwise {
                    walk_block(o, c);
                }
            }
            StmtKind::While { cond, body } => {
                walk(cond, c);
                walk_block(body, c);
            }
            StmtKind::NumericFor { start, limit, step, body, .. } => {
                walk(start, c);
                walk(limit, c);
                if let Some(s) = step {
                    walk(s, c);
                }
                walk_block(body, c);
            }
            StmtKind::GenericFor { iter, body, .. } => {
                iter.iter().for_each(|e| walk(e, c));
                walk_block(body, c);
            }
            StmtKind::Return(v) => v.iter().for_each(|e| walk(e, c)),
            StmtKind::Hoist { defs, rest } => {
                walk_block(defs, c);
                walk_block(rest, c);
            }
            StmtKind::Break | StmtKind::Goto(_) | StmtKind::Label(_) => {}
        }
    }
}

fn is_spawn_name(e: Option<&Expr>) -> bool {
    matches!(e, Some(Expr::Str(s)) if s.starts_with(b"<spawn do at "))
}

fn walk(e: &Expr, c: &mut Count) {
    match e {
        Expr::Call(f, args) => {
            if matches!(f.as_ref(), Expr::Intrinsic("spawn")) {
                if let Some(Expr::Table(info)) = args.first() {
                    let named = matches!(info.get(1), Some(TableEntry::Item(n)) if is_spawn_name(Some(n)));
                    if named {
                        c.spawns += 1;
                        if matches!(args.get(1), Some(Expr::Function(_))) {
                            c.inline += 1;
                        }
                    }
                }
            }
            if matches!(f.as_ref(), Expr::Intrinsic("register")) && is_spawn_name(args.get(1)) {
                c.registered += 1;
            }
            walk(f, c);
            args.iter().for_each(|a| walk(a, c));
        }
        Expr::Function(f) => walk_fn(f, c),
        Expr::Index(a, b) | Expr::Binary(_, a, b) => {
            walk(a, c);
            walk(b, c);
        }
        Expr::MethodCall(o, _, args) => {
            walk(o, c);
            args.iter().for_each(|a| walk(a, c));
        }
        Expr::Unary(_, a) | Expr::Paren(a) => walk(a, c),
        Expr::Table(entries) => {
            for en in entries {
                match en {
                    TableEntry::Item(x) => walk(x, c),
                    TableEntry::Pair(k, x) => {
                        walk(k, c);
                        walk(x, c);
                    }
                }
            }
        }
        Expr::MemberCall { receiver, key, args } => {
            walk(receiver, c);
            walk(key, c);
            args.iter().for_each(|a| walk(a, c));
        }
        Expr::MemberApply { func, receiver, args } => {
            walk(func, c);
            walk(receiver, c);
            args.iter().for_each(|a| walk(a, c));
        }
        Expr::AutoGet { receiver, key } => {
            walk(receiver, c);
            walk(key, c);
        }
        _ => {}
    }
}

/// The counts for a program, and the codes of its lowering diagnostics.
fn lower_counts(src: &str) -> (Count, Vec<&'static str>) {
    let parse = nl_syntax::parse(src);
    assert!(!parse.has_errors(), "the test program parses");
    let root = parse.syntax();
    let res = nl_sema::resolve(&root, &nl_sema::NoImports);
    assert!(!res.has_errors(), "the test program resolves: {:?}", res.diagnostics);
    let opts = LowerOptions {
        checks: true,
        chunk_name: "@t.nl".to_string(),
        file_label: "t.nl".to_string(),
        hints: nl_sema::Hints::default(),
        level: 0,
        localize: Localize::Std,
        library_fields: Default::default(),
        imported_globals: Vec::new(),
        profile: nl_sema::Profile::Full,
    };
    let out = lower(&root, src, &res, &opts);
    let mut c = Count::default();
    walk_fn(&out.chunk.main, &mut c);
    (c, out.diagnostics.iter().map(|d| d.code).collect())
}

const LIFTED: &[&str] = &[
    // At file level, in a function.
    "fn f(n)\n  spawn do\n    n + 1\n  end\nend\n",
    // In a module function that calls a sibling function and builds a
    // struct of the module.
    "module Geo\n  struct Box\n    w: Number\n  end\n  fn grow(n)\n    n + 1\n  end\n  fn boxed(n)\n    spawn do\n      Box { w: grow(n) }\n    end\n  end\nend\n",
    // A struct and a module declared in the body of a top-level `try`,
    // which runs once when the file loads.
    "try\n  struct Box\n    v: Number\n  end\n  fn run\n    spawn do\n      Box { v: 2 }\n    end\n  end\ncatch e\n  print e\nend\n",
    "try\n  module M\n    fn sib(n: Number): Number\n      n + 1\n    end\n    fn run(n: Number)\n      spawn do\n        sib(n)\n      end\n    end\n  end\nfinally\n  print 1\nend\n",
    // In its `finally`, and in a `try` nested in one.
    "try\n  print 1\nfinally\n  struct Box\n    v: Number\n  end\n  fn run\n    spawn do\n      Box { v: 2 }\n    end\n  end\nend\n",
    "try\n  try\n    struct Box\n      v: Number\n    end\n    fn run\n      spawn do\n        Box { v: 2 }\n      end\n    end\n  finally\n    print 1\n  end\ncatch e\n  print e\nend\n",
    // In a loop, a method, a generic function; nested blocks.
    "fn f\n  for i in 1..3\n    spawn do\n      i * 10\n    end\n  end\nend\n",
    "struct C\n  step: Number\nend\nfn C.ahead(n)\n  let s = @step\n  spawn do\n    n + s\n  end\nend\n",
    "fn twice<T>(v: T)\n  spawn do\n    [v, v]\n  end\nend\n",
    "fn f\n  spawn do\n    let inner = spawn do\n      1\n    end\n    inner.await\n  end\nend\n",
    // At the top level itself.
    "let t = spawn do\n  1\nend\n",
];

#[test]
fn every_spawn_do_block_is_registered_when_its_file_loads() {
    for src in LIFTED {
        let (c, diags) = lower_counts(src);
        assert!(diags.is_empty(), "{src}: {diags:?}");
        assert!(c.spawns > 0, "{src}: no spawn do found");
        assert_eq!(c.inline, 0, "{src}: a block is still made where its spawn stands");
        assert_eq!(c.registered, c.spawns, "{src}: every block is registered");
    }
}

/// The messages of a program's lowering diagnostics.
fn lower_messages(src: &str) -> Vec<String> {
    let parse = nl_syntax::parse(src);
    let root = parse.syntax();
    let res = nl_sema::resolve(&root, &nl_sema::NoImports);
    let opts = LowerOptions {
        checks: true,
        chunk_name: "@t.nl".to_string(),
        file_label: "t.nl".to_string(),
        hints: nl_sema::Hints::default(),
        level: 0,
        localize: Localize::Std,
        library_fields: Default::default(),
        imported_globals: Vec::new(),
        profile: nl_sema::Profile::Full,
    };
    lower(&root, src, &res, &opts).diagnostics.iter().map(|d| d.message.clone()).collect()
}

#[test]
fn a_file_over_the_upvalue_budget_still_registers_its_blocks() {
    // A function that reads 56 file-level locals has no upvalue left for
    // the table of lifted blocks: limits.rs moves the file's locals into a
    // table of their own instead of the block staying unregistered.
    for n in [55, 56, 61] {
        let mut src = String::new();
        let mut sum = String::from("0");
        for i in 1..=n {
            src.push_str(&format!("let v{i} = {i}\n"));
            sum.push_str(&format!(" + v{i}"));
        }
        src.push_str(&format!("fn f\n  let s = {sum}\n  let t = spawn do\n    1\n  end\n  s\nend\nprint f() unless Worker.parent\n"));
        let (c, diags) = lower_counts(&src);
        assert!(diags.is_empty(), "{n} locals: {diags:?}");
        assert_eq!(c.registered, c.spawns, "{n} locals: every block is registered");
        assert_eq!(c.inline, 0, "{n} locals: a block is still made where its spawn stands");
    }
}

#[test]
fn a_declaration_in_a_top_level_branch_is_a_compile_error() {
    // A worker that loads the file may not take the branch (the
    // `unless Worker.parent` guard): the block would be missing there.
    let cases = [
        "if true\n  struct Box\n    v: Number\n  end\n  fn run\n    spawn do\n      Box { v: 2 }\n    end\n  end\nend\n",
        "unless Worker.parent\n  struct Job\n    n: Number\n  end\n  let t = spawn do\n    Job { n: 1 }.n\n  end\nend\n",
        "unless Worker.parent\n  module Jobs\n    fn one: Number\n      1\n    end\n    fn run\n      spawn do\n        one()\n      end\n    end\n  end\nend\n",
        "try\n  raise \"x\"\ncatch e\n  struct Box\n    v: Number\n  end\n  fn run\n    spawn do\n      Box { v: 2 }\n    end\n  end\nend\n",
        "if true\n  try\n    struct Box\n      v: Number\n    end\n    fn run\n      spawn do\n        Box { v: 2 }\n      end\n    end\n  catch e\n    print e\n  end\nend\n",
    ];
    for src in cases {
        let (_, diags) = lower_counts(src);
        assert_eq!(diags, ["spawn-block"], "{src}");
        let messages = lower_messages(src);
        assert!(messages.iter().any(|m| m.contains("in a branch of a top-level conditional")), "{src}: {messages:?}");
    }
}

#[test]
fn the_error_names_what_the_block_reads() {
    // A sibling function of a module declared in a loop: the message names
    // `sib`, which the block's code names, not the module passed as its
    // `self`.
    let src = "for i in [1]\n  module M\n    fn sib(n: Number): Number\n      n + 1\n    end\n    fn run(n: Number)\n      spawn do\n        sib(n)\n      end\n    end\n  end\nend\n";
    let messages = lower_messages(src);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages.iter().all(|m| m.contains("reads `sib`")), "{messages:?}");
}

#[test]
fn a_block_that_cannot_be_made_at_load_is_a_compile_error() {
    // A struct declared in a top-level loop: the block would have to be
    // made once per iteration.
    let src = "for i in 1..2\n  struct Box\n    v: Number\n  end\n  let t = spawn do\n    Box { v: 1 }\n  end\nend\n";
    let (c, diags) = lower_counts(src);
    assert!(diags.contains(&"spawn-block"), "{diags:?}");
    assert!(c.inline <= c.spawns);
}
