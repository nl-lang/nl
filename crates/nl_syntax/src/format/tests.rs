//! Unit tests of the formatter: input to expected output, one case per
//! form of the grammar, plus the indentation, comment and `--fix`
//! rules. Every case also checks the three invariants: the output parses
//! to the same tree (trivia aside) with the same diagnostics, and
//! formatting it again changes nothing.

use std::time::{Duration, Instant};

use crate::{DumpOptions, FormatOptions, Formatted, SyntaxKind, decode_string, dump_tree, format, format_decl, parse, parse_decl_file};

/// A tree dump without its byte offsets: kinds and token texts only.
pub(crate) fn strip_offsets(dump: &str) -> String {
    let mut out = String::new();
    for line in dump.lines() {
        match line.find('@') {
            Some(at) => {
                let rest = &line[at..];
                let end = rest.find(' ').map_or(line.len(), |i| at + i);
                out.push_str(&line[..at]);
                out.push_str(&line[end..]);
            }
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// The trivia-free shape of a source's tree.
pub(crate) fn shape(src: &str, decl: bool) -> String {
    let p = if decl { parse_decl_file(src) } else { parse(src) };
    strip_offsets(&dump_tree(&p.syntax(), DumpOptions { skip_trivia: true }))
}

fn messages(src: &str, decl: bool) -> Vec<String> {
    let p = if decl { parse_decl_file(src) } else { parse(src) };
    p.diagnostics().iter().map(|d| d.message.clone()).collect()
}

fn run(src: &str, decl: bool, fix: bool) -> Formatted {
    let options = FormatOptions { fix };
    let r = if decl { format_decl(src, options) } else { format(src, options) };
    r.unwrap_or_else(|d| panic!("{src:?} did not format: {d:?}"))
}

fn check_any(src: &str, expected: &str, decl: bool) {
    let out = run(src, decl, false);
    assert_eq!(out.text, expected, "formatting {src:?}");
    assert_eq!(run(expected, decl, false).text, expected, "not idempotent: {expected:?}");
    assert_eq!(shape(src, decl), shape(expected, decl), "the tree changed for {src:?}");
    assert_eq!(messages(src, decl), messages(expected, decl), "the diagnostics changed for {src:?}");
}

/// Formats `src` and expects `expected`.
fn check(src: &str, expected: &str) {
    check_any(src, expected, false);
}

/// `src` is already formatted.
fn same(src: &str) {
    check(src, src);
}

// ---- Blank lines, trailing whitespace, line ends ------------------------------

#[test]
fn empty_and_blank_files() {
    assert_eq!(run("", false, false).text, "");
    assert_eq!(run("\n\n", false, false).text, "");
    assert_eq!(run("   \n", false, false).text, "");
}

#[test]
fn final_newline_and_trailing_whitespace() {
    check("x", "x\n");
    check("x  \n\n\n", "x\n");
    check("x\t\ny \n", "x\ny\n");
    check("\n\nx\n", "x\n");
}

#[test]
fn blank_lines_collapse() {
    check("a\n\n\n\nb\n", "a\n\nb\n");
    check("fn f\n\n\n  x\n\n\nend\n", "fn f\n\n  x\n\nend\n");
}

#[test]
fn line_ends_are_kept() {
    check("let x =  1\r\nprint x\r\n", "let x = 1\r\nprint x\r\n");
    check("let x =  1\rprint x", "let x = 1\rprint x\r");
}

#[test]
fn byte_order_mark_is_kept() {
    check("\u{FEFF}let  x = 1\n", "\u{FEFF}let x = 1\n");
    // The mark is not a column: comment lines after it stay where they are.
    same("\u{FEFF}# a\n# b\nlet x = 1\n");
    check("\u{FEFF}let  x = 1 # a\n           # b\n", "\u{FEFF}let x = 1 # a\n          # b\n");
    same("\u{FEFF}let x = 1 # a\n          # b\n");
}

#[test]
fn large_brackets_format_in_linear_time() {
    // A 10k-line list and a 10k-pair table with a comment before each pair:
    // no line start may rescan its bracket.
    let items: Vec<String> = (0..10_000).map(|i| format!("  {i},")).collect();
    let list = format!("let a = [\n{}\n]\n", items.join("\n"));
    let pairs: Vec<String> = (0..10_000).map(|i| format!("  # pair {i}\n  k{i}: {i}, # c")).collect();
    let table = format!("let t = {{\n{}\n}}\n", pairs.join("\n"));
    for src in [list, table] {
        let start = Instant::now();
        let out = run(&src, false, false);
        let elapsed = start.elapsed();
        assert_eq!(out.text, src);
        assert!(elapsed < Duration::from_secs(1), "formatting {} lines took {elapsed:?}", src.lines().count());
    }
}

// ---- Blocks and statements ---------------------------------------------------

#[test]
fn statements_and_semicolons() {
    check("a ;b;  c\n", "a; b; c\n");
    check("if a; b; else; c; end\n", "if a; b; else; c; end\n");
    check("a,b\n", "a, b\n");
}

#[test]
fn nested_blocks_indent_two_spaces() {
    check(
        "fn outer\nif a\nwhile b\nx\nend\nelse\ny\nend\nend\n",
        "fn outer\n  if a\n    while b\n      x\n    end\n  else\n    y\n  end\nend\n",
    );
    check("fn f\n\t\tx\n\tend\n", "fn f\n  x\nend\n");
    check("    fn f\n        x\n    end\n", "fn f\n  x\nend\n");
}

#[test]
fn do_block() {
    check("do\nlet a = 1\nend\n", "do\n  let a = 1\nend\n");
}

// ---- Declarations ------------------------------------------------------------

#[test]
fn fn_declarations() {
    check("fn foo\nend\n", "fn foo\nend\n");
    check("fn  foo ( a , b=1 ):Number\n  a+b\nend\n", "fn foo(a, b = 1): Number\n  a + b\nend\n");
    check("pub async fn load( id )\nend\n", "pub async fn load(id)\nend\n");
    check("fn Foo . method\nend\n", "fn Foo.method\nend\n");
    check("fn Foo.create( a : Number ) : Self\n  Self{a: a}\nend\n", "fn Foo.create(a: Number): Self\n  Self { a: a }\nend\n");
    same("fn f<T>(a: T): T\n  a\nend\n");
    check("fn f <T1,T2> (a: T1, b: T2)\nend\n", "fn f<T1, T2>(a: T1, b: T2)\nend\n");
    check("fn foo: Function\n  a!\nend\n", "fn foo: Function\n  a!\nend\n");
}

#[test]
fn operator_methods() {
    same("fn Foo.+(right)\nend\n");
    same("fn Foo.-\nend\n");
    check("fn Foo . [] (key)\nend\n", "fn Foo.[](key)\nend\n");
    same("fn Foo.[]=(key, value)\nend\n");
    same("fn Foo.()(*args)\nend\n");
    same("fn Foo.<>(right)\nend\n");
    same("fn Foo.<=(right)\nend\n");
}

#[test]
fn parameters() {
    check("fn foo(a: any, *args: any)\nend\n", "fn foo(a: any, *args: any)\nend\n");
    check("fn foo(a, * args)\nend\n", "fn foo(a, *args)\nend\n");
    same("fn foo(a, **opts: Table)\nend\n");
    check("fn foo( { a , b } as tbl : Table )\nend\n", "fn foo({a, b} as tbl: Table)\nend\n");
    check("fn foo ({key: value: Number})\nend\n", "fn foo({key: value: Number})\nend\n");
    same("fn greet(name, greeting = \"hello\")\n  print \"#{greeting}, #{name}\"\nend\n");
}

#[test]
fn let_const_global() {
    check("let  a", "let a\n");
    check("let a:Number", "let a: Number\n");
    check("let a , b = 1 , 2\n", "let a, b = 1, 2\n");
    check("const  MAX=10\n", "const MAX = 10\n");
    check("pub const limit=10\n", "pub const limit = 10\n");
    check("global  C\nglobal D : Number = 1\n", "global C\nglobal D: Number = 1\n");
    check("let {a, b} = tbl\nlet [a, b] = tbl\n", "let {a, b} = tbl\nlet [a, b] = tbl\n");
}

#[test]
fn struct_declarations() {
    check("struct Foo\na: Number\nb: String = \"x\"\nend\n", "struct Foo\n  a: Number\n  b: String = \"x\"\nend\n");
    same("struct Bar : Foo\n  c: Table\nend\n");
    check("pub struct Box<T>\n  value : T\nend\n", "pub struct Box<T>\n  value: T\nend\n");
    check("struct Empty end\n", "struct Empty end\n");
}

#[test]
fn interfaces() {
    check(
        "interface Foo\nfn a\nfn b(var)\nfn c(var: Number): nil\nd: String\nend\n",
        "interface Foo\n  fn a\n  fn b(var)\n  fn c(var: Number): nil\n  d: String\nend\n",
    );
    check(
        "interface Foo for SomeStruct\nfn a\nend\n\nfn c(var: Number): nil\nnil\nend\nend\n",
        "interface Foo for SomeStruct\n  fn a\n  end\n\n  fn c(var: Number): nil\n    nil\n  end\nend\n",
    );
    same("interface To<Number> for Point\n  fn to: Number\n    0\n  end\nend\n");
    same("interface From<T>\n  fn from(val: T): Self\nend\n");
}

#[test]
fn enums() {
    check("enum Direction\nNorth\nEast\nend\n", "enum Direction\n  North\n  East\nend\n");
    check("enum Team\nRed=1\nBlue = -2\nend\n", "enum Team\n  Red = 1\n  Blue = -2\nend\n");
}

#[test]
fn imports() {
    check("import  foo  from  'filename'\n", "import foo from 'filename'\n");
    check("import {foo,bar} from 'filename'\n", "import { foo, bar } from 'filename'\n");
    check("import { foo, bar } from 'filename'\n", "import { foo, bar } from 'filename'\n");
    check("import from 'filename'  as File\n", "import from 'filename' as File\n");
}

#[test]
fn type_declarations() {
    check("type Foo=Number\n", "type Foo = Number\n");
    check("type Composite = Number|String\n", "type Composite = Number | String\n");
    check("type Neg = ~ Number\n", "type Neg = ~Number\n");
    check("type Point = { x : Number , y : Number }\n", "type Point = {x: Number, y: Number}\n");
    check("pub type Pair<T> = [ T ]\n", "pub type Pair<T> = [T]\n");
}

#[test]
fn modules() {
    check("module Foo\nfn some_func\nend\nend\n", "module Foo\n  fn some_func\n  end\nend\n");
    same("module Foo.Bar\n  fn f\n  end\nend\n");
    same("pub module Foo end\n");
}

// ---- Control flow ------------------------------------------------------------

#[test]
fn if_forms() {
    check(
        "if a\nx\nelseif b\ny\nelse\nz\nend\n",
        "if a\n  x\nelseif b\n  y\nelse\n  z\nend\n",
    );
    check("if let player = find(name)\n  x\nend\n", "if let player = find(name)\n  x\nend\n");
    check("if let {role: \"admin\", name} = user\nx\nelse\ny\nend\n", "if let {role: \"admin\", name} = user\n  x\nelse\n  y\nend\n");
    check("let label = if 1 > 2\n\"bigger\"\nelse\n\"smaller\"\nend\n", "let label = if 1 > 2\n  \"bigger\"\nelse\n  \"smaller\"\nend\n");
    check("unless ok\n\"refused\"\nelse\nx\nend\n", "unless ok\n  \"refused\"\nelse\n  x\nend\n");
}

#[test]
fn case_and_cond() {
    check(
        "case msg\nwhen 3..9\n\"range\"\nwhen is String\n\"string\"\nwhen {kind: \"move\", to: dest}\n\"move to #{dest}\"\nwhen Circle {r} if r>10\n\"big\"\nwhen [x, 456], Direction.North\n\"pair\"\nelse\n\"other\"\nend\n",
        "case msg\nwhen 3..9\n  \"range\"\nwhen is String\n  \"string\"\nwhen {kind: \"move\", to: dest}\n  \"move to #{dest}\"\nwhen Circle {r} if r > 10\n  \"big\"\nwhen [x, 456], Direction.North\n  \"pair\"\nelse\n  \"other\"\nend\n",
    );
    check("cond\nwhen a>b\nx\nwhen a<b\ny\nelse\nz\nend\n", "cond\nwhen a > b\n  x\nwhen a < b\n  y\nelse\n  z\nend\n");
    check("let word = case 2\nwhen 1\n\"one\"\nend\n", "let word = case 2\nwhen 1\n  \"one\"\nend\n");
}

#[test]
fn loops() {
    check("while condition\nx\nend\n", "while condition\n  x\nend\n");
    check("until condition\nx\nend\n", "until condition\n  x\nend\n");
    check("loop\nbreak if done\nend\n", "loop\n  break if done\nend\n");
    check("for i in 1 .. 10\nend\n", "for i in 1..10\nend\n");
    check("for i in 1..10  step  2\ncontinue\nend\n", "for i in 1..10 step 2\n  continue\nend\n");
    check("for k,v in table\nend\n", "for k, v in table\nend\n");
    check("for m in date.scan text\nend\n", "for m in date.scan text\nend\n");
}

#[test]
fn try_forms() {
    check(
        "try\nfoo\ncatch err:MyException\nx\ncatch err\ny\nfinally\nz\nend\n",
        "try\n  foo\ncatch err: MyException\n  x\ncatch err\n  y\nfinally\n  z\nend\n",
    );
    check("let safe = try\nraise \"x\"\ncatch\n\"recovered\"\nend\n", "let safe = try\n  raise \"x\"\ncatch\n  \"recovered\"\nend\n");
}

#[test]
fn return_break_continue() {
    check("fn f\nreturn\nend\n", "fn f\n  return\nend\n");
    check("fn f\nreturn a,b\nend\n", "fn f\n  return a, b\nend\n");
    same("fn f\n  return amount / 2 if target.armored\nend\n");
    same("fn f\n  return if done\nend\n");
}

#[test]
fn statement_modifiers() {
    check("some_function() if  condition\n", "some_function() if condition\n");
    same("save unless dirty\n");
    same("step() while busy\n");
    same("x += 1 until x > 3\n");
    same("f x do\n  y\nend if z\n");
}

// ---- Assignment --------------------------------------------------------------

#[test]
fn assignments() {
    check("a=b\n", "a = b\n");
    check("a+=b\na -= b\na //= 2\na **= 2\na ||= \"owo\"\na &&= 1\n", "a += b\na -= b\na //= 2\na **= 2\na ||= \"owo\"\na &&= 1\n");
    check("x,y=y,x\n", "x, y = y, x\n");
    check("t.a=1\nt[\"k\"]=2\n@f = 3\n", "t.a = 1\nt[\"k\"] = 2\n@f = 3\n");
    same(":config.maxPlayers = 8\n");
}

// ---- Expressions -------------------------------------------------------------

#[test]
fn binary_operators() {
    check("let r = a+b*c-d/e//f%g**h\n", "let r = a + b * c - d / e // f % g ** h\n");
    check("let r = a<>b\n", "let r = a <> b\n");
    check("let r = a|b&c^d<<1>>2>>>3\n", "let r = a | b & c ^ d << 1 >> 2 >>> 3\n");
    check("let r = a==b&&a!=b||a<b or a<=b and a>b\n", "let r = a == b && a != b || a < b or a <= b and a > b\n");
    check("let r = a>=b\n", "let r = a >= b\n");
    same("let r = (a or b) and c\n");
}

#[test]
fn unary_operators() {
    check("let r = - a\nlet s = ~ a\nlet t = ! a\nlet u = not  a\n", "let r = -a\nlet s = ~a\nlet t = !a\nlet u = not a\n");
    same("let r = -5.abs\n");
    same("let r = -2 ** 2\n");
    same("let r = a - -b\n");
    check("let r = -  -a\nlet s = ~~a\nlet t = !!a\n", "let r = - -a\nlet s = ~~a\nlet t = !!a\n");
    same("if a == b || a != b || !a || !b\nend\n");
}

#[test]
fn ranges() {
    check("let a = 1 .. 10\nlet b = 1 ... 10\n", "let a = 1..10\nlet b = 1...10\n");
    same("let c = lo..hi\n");
    same("let d = (1 + 2)..n\n");
}

#[test]
fn is_as_pipe() {
    check("let a = var  as  String\n", "let a = var as String\n");
    check("if a  is  Number\nend\n", "if a is Number\nend\n");
    check("let r = x as Number < 10\n", "let r = x as Number < 10\n");
    check("let r = 5|>double\n", "let r = 5 |> double\n");
    same("let chained = 1\n  |> double\n  |> add(3)\n  |> double\n");
    check("a\n|> some_func\n|> other\n", "a\n  |> some_func\n  |> other\n");
}

#[test]
fn nil_safe_operators() {
    check("let r = a ?. b\n", "let r = a?.b\n");
    check("let r = a?. [k]\nlet s = f ?. (x)\nlet t = o ?.:m(1)\n", "let r = a?.[k]\nlet s = f?.(x)\nlet t = o?.:m(1)\n");
    check("let r = a??b\n", "let r = a ?? b\n");
    check("x??=1\nt[k]  ??=  []\n", "x ??= 1\nt[k] ??= []\n");
    check("a?.b  =  1\n:p ?. x = 2\n", "a?.b = 1\n:p?.x = 2\n");
    same("let r = :LocalPlayer()?.:GetPos()\n");
    same("let r = a?.b.c(d)?.e ?? f\n");
    same("a?.m x, y\n");
    check("let name = player\n?.profile\n?.name\n?? \"nobody\"\n", "let name = player\n  ?.profile\n  ?.name\n  ?? \"nobody\"\n");
    // `?` alone stays tight in a type.
    same("let n: Number? = a ?? 0\n");
}

#[test]
fn calls() {
    check("f ( x )\n", "f(x)\n");
    check("f(a,b)\n", "f(a, b)\n");
    check("f()\n", "f()\n");
    check("import {a,} from 'x'\nf(1,)\nlet l = [1,]\nfn g(a,)\nend\n", "import { a, } from 'x'\nf(1, )\nlet l = [1, ]\nfn g(a, )\nend\n");
    check("f(*list)\nf(a, * list)\nf(a, **k)\n", "f(*list)\nf(a, *list)\nf(a, **k)\n");
    same("f(key: v)\nf(a, key: v)\n");
    // `f(key:v)` is the method call `key:v`; the colon keeps its reading.
    same("f(key:v)\n");
    check("f(a)(b)\n", "f(a)(b)\n");
    same("let r2 = add 1, add(2, 3)\n");
}

#[test]
fn command_calls() {
    same("print x\n");
    check("a  b,  c\n", "a b, c\n");
    same("print \"a\", \"b\"\n");
    // `five -1` is the subtraction `five - 1`.
    check("let s = five -1\n", "let s = five - 1\n");
    // `f *list` is the multiplication `f * list`.
    check("f *list\n", "f * list\n");
    same("f a, *list\n");
    same("f !x\n");
    same("f ~x\n");
    same("f {a: 1}\n");
    same("f key: 1\n");
    same("validates 'role', presence: true\n");
    same("print :string.len(\"x\")\n");
    same("print conn:send(\"x\")\n");
    same("Event.remove \"EventName\", \"handler_name\"\n");
    same("@on_hit x\n");
    same("t.type x\n");
    same("let m = re.match \"uwu\"\n");
    same("spawn f a, b\n");
}

#[test]
fn members_indexes_methods_bang() {
    check("x . y . z\n", "x.y.z\n");
    check("t [1]\nt[ \"k\" ]\n", "t[1]\nt[\"k\"]\n");
    same("let n = a.len\n");
    same("let n = \"héllo\".len\n");
    same("print 2.5.floor\n");
    same("print 100.divisible_by(10)\n");
    same("conn:sendMessage(\"hi\")\n");
    same("x:m\n");
    same("fn foo: Function\n  a!\nend\n");
    same("let f = @cb!\n");
    same("let g = x.end!\n");
    same("let h = ON_HIT!\n");
}

#[test]
fn trailing_blocks() {
    check("foo 123 do\nx\nend\n", "foo 123 do\n  x\nend\n");
    check("let doubled = apply 21 do | n |\nn * 2\nend\n", "let doubled = apply 21 do |n|\n  n * 2\nend\n");
    same("let typed = apply 5 do |n: Number|: Number\n  n + 1\nend\n");
    check("f do ||\nend\n", "f do ||\nend\n");
    check("f do | |\nend\n", "f do | |\nend\n");
    check("f do |a, b,| end\n", "f do |a, b, | end\n");
    same("f do |{x, y}| end\n");
    same("f do |*rest| end\n");
    same("f do |a = 1| end\n");
    same("f do |a: (A | B), m = (x | y)| end\n");
    same("each list do |v| print(\"inline #{v}\") end\n");
    same("items.each do |x|\n  x\nend\n");
    same("f(a) do\n  x\nend\n");
    same("task.on_done async do |p|\n  p\nend\n");
    same("f do |a| :print(a) end\n");
}

#[test]
fn literals_and_strings() {
    same("let a = 'simple string'\n");
    same("let b = \"variable a was #{a}\"\n");
    check("let c = \"a #{ x + 1 } b #{ f(\"c\") } d\"\n", "let c = \"a #{x + 1} b #{f(\"c\")} d\"\n");
    same("let d = 1_000_000\nlet e = 5e+20\nlet f = 0xabcdef\n");
    same("let t = true\nlet f = false\nlet n = nil\n");
    same("let s = self\nlet t = Self\n");
}

#[test]
fn heredocs_keep_their_text() {
    same("let d = \"\"\"\nmulti\n  line  \nthis works here: #{a}\n\"\"\"\n");
    same("let e = \"\"\"-\n  this\n    string\n\"\"\"\n");
    same("let f = \"\"\"=\nno #{a}\n\"\"\"\n");
    same("let g = \"\"\"-=\n  raw\n\"\"\"\n");
    same("let h = \"\"\"\nx\n\"\"\".strip\n");
    // A multi-line interpolation of a heredoc keeps its layout.
    same("let s = \"\"\"\n  v #{\n      x\n    } w\n\"\"\"\n");
    check("let s = \"\"\"\n  v #{\n      x  +  1\n    } w\n\"\"\"\n", "let s = \"\"\"\n  v #{\n      x + 1\n    } w\n\"\"\"\n");
}

#[test]
fn lists_and_tables() {
    check("let a = [ 1 , 2 , 3 ]\n", "let a = [1, 2, 3]\n");
    check("let b = { a : 1 , b : 2 }\n", "let b = {a: 1, b: 2}\n");
    same("let c = {\"any string\": 1, 1: 3}\n");
    check("let more = { \"any string\": 1, }\n", "let more = {\"any string\": 1, }\n");
    check("let more = {\"any string\": 1,}\n", "let more = {\"any string\": 1, }\n");
    same("let empty = {}\nlet none = []\n");
    same("let nested = {inner: {deep: \"yes\"}, list: [10, 20]}\n");
    same("let shorthand = {x: x}\n");
    same("let path = {f: :x}\n");
    check("let p = Foo{a: 1, b: \"x\"}\n", "let p = Foo { a: 1, b: \"x\" }\n");
    same("let q = Shapes.Circle { r: 1 }\n");
    same("let r = Foo {}\n");
    same("let s = [*a, 1]\n");
}

#[test]
fn multi_line_brackets() {
    check(
        "let b = {\na: 1,\nb: 2,\n1: 3\n}\n",
        "let b = {\n  a: 1,\n  b: 2,\n  1: 3\n}\n",
    );
    check("let c = [\n[\"table inside table\"]\n]\n", "let c = [\n  [\"table inside table\"]\n]\n");
    check("foo(\na,\nb\n)\n", "foo(\n  a,\n  b\n)\n");
    check("let p = Foo {\na: 1\n}\n", "let p = Foo {\n  a: 1\n}\n");
    check("f(1, fn(x)\nx\nend)\n", "f(1, fn(x)\n  x\nend)\n");
    check(
        "f(a,\ng(b) do |x|\nx\nend)\n",
        "f(a,\n  g(b) do |x|\n    x\n  end)\n",
    );
    check("f(a, [\nb,\n[\nc\n]\n])\n", "f(a, [\n  b,\n  [\n    c\n  ]\n])\n");
    check("fn f(\na,\nb\n)\nend\n", "fn f(\n  a,\n  b\n)\nend\n");
    check("let x: Box<\nT\n> = y\n", "let x: Box<\n  T\n> = y\n");
    check("f do |a,\nb|\nend\n", "f do |a,\n  b|\nend\n");
}

#[test]
fn continuation_lines() {
    check("let total = base +\nbonus\n", "let total = base +\n  bonus\n");
    check("let total = base\n+ bonus\n", "let total = base\n  + bonus\n");
    check("let name = player\n.name\n", "let name = player\n  .name\n");
    check("let a, b =\npos()\n", "let a, b =\n  pos()\n");
    check("print \"a\",\n\"b\"\n", "print \"a\",\n  \"b\"\n");
    check("if a &&\nb\nx\nend\n", "if a &&\n  b\n  x\nend\n");
    check("fn f\nlet y = x +\n1\ny\nend\n", "fn f\n  let y = x +\n    1\n  y\nend\n");
    check("let x = a\n-b\n", "let x = a\n-b\n");
    check("struct Foo\n  a: Number =\n  1\nend\n", "struct Foo\n  a: Number =\n    1\nend\n");
}

#[test]
fn fn_expressions() {
    check("let a = fn print('x') end\n", "let a = fn print('x') end\n");
    check("let b = fn (a) print('x') end\n", "let b = fn(a) print('x') end\n");
    same("let c = fn(a: Number): nil print('x') end\n");
    check("let d = fn\nx\nend\n", "let d = fn\n  x\nend\n");
    same("let e = async fn(x)\n  x\nend\n");
}

#[test]
fn short_functions() {
    // The short form stays short, spaced `|a, b| -> a + b`.
    check("let f = x->x*2\n", "let f = x -> x * 2\n");
    check("let g = | a , b |->a+b\n", "let g = |a, b| -> a + b\n");
    check("let h = ||->true\n", "let h = || -> true\n");
    same("let n = | | -> nil\n");
    check("let t = |a :Number , b: Number|:Number->a\n", "let t = |a: Number, b: Number|: Number -> a\n");
    same("let d = |a, b = 1, *rest, **opts| -> a\n");
    same("let add = a -> b -> a + b\n");
    same("sort(t, |a, b| -> a.id < b.id)\n");
    same("each items, x -> print(x)\n");
    // A block body is laid out like the body of `fn`.
    check("let m = |a| -> do\nlet b = a\nreturn a, b\nend\n", "let m = |a| -> do\n  let b = a\n  return a, b\nend\n");
    same("f(x -> do\n  print(x)\nend)\n");
    // A body on the line after `->` is one level deeper than the lambda's line.
    check("let f = x ->\nx + 1\n", "let f = x ->\n  x + 1\n");
    check("items.map(|a, b|\n-> a)\n", "items.map(|a, b|\n  -> a)\n");
    // Comments after the expression body stay with what follows.
    same("let f = x -> x  # aligned\n# about g\n## doc of g\nfn g\nend\n");
}

#[test]
fn spawn_and_on() {
    same("let task = spawn find_path(from, to)\n");
    check("let mesh = spawn do\nsimplify(chunk)\nend\n", "let mesh = spawn do\n  simplify(chunk)\nend\n");
    check("on EventName do\nx\nend\n", "on EventName do\n  x\nend\n");
    same("on EventName do |arg1: Number, arg2: String|: nil\n  x\nend\n");
    same("on EventName as 'handler_name' do\n  x\nend\n");
    same("on PlayerJoined async do |player|\n  x\nend\n");
}

#[test]
fn lua_paths() {
    check("print :string . len(\"x\")\n", "print :string.len(\"x\")\n");
    same(":player.getHealth()\n");
    same(":GetPlayerName(1)\n");
    same("let max = :config.maxPlayers\n");
    same(":hud.mainPanel:setVisible(true)\n");
    same(":print \"command call through a path\"\n");
    same("x = -:a.b\n");
    same("let z = !:cfg.on\n");
    same("let r = 1..:n\n");
    same("f(:a.b)\n");
    same("let v = :t[1].x\n");
    same(":say_hi\n");
    same("let n = (:f()).name\n");
    same(":_G.say_hi = fn print(\"hi\") end\n");
    same("let t = :items.each do |x|\n  x\nend\n");
}

// ---- Types -------------------------------------------------------------------

#[test]
fn types() {
    check("let a : Number|String|nil\n", "let a: Number | String | nil\n");
    check("let b: Number ?\n", "let b: Number?\n");
    check("let c: Function( a : Number , b : String ) : nil\n", "let c: Function(a: Number, b: String): nil\n");
    check("let d: Function\n", "let d: Function\n");
    check("let e: Box< Box< T > >\n", "let e: Box<Box<T>>\n");
    check("let f: {[String]: Number}\n", "let f: {[String]: Number}\n");
    check("let g: [ Number ]\n", "let g: [Number]\n");
    check("let h: ( A | B )\n", "let h: (A | B)\n");
    check("fn f(): ( Number , Number )\nend\n", "fn f(): (Number, Number)\nend\n");
    same("let i: Function(*args: any): any\n");
    same("let j: {name: String, 'odd key': Number}\n");
    same("let k: any\nlet l: never\nlet m: unknown\n");
}

#[test]
fn return_type_colon_keeps_its_reading() {
    // `fn f() :Number` is a body that starts with a Lua path, with a
    // warning: the spacing stays, and so does the warning.
    same("fn f() :Number\nend\n");
    same("fn f :Number\nend\n");
    check("fn f() : Number\nend\n", "fn f(): Number\nend\n");
    check("fn f : Number\nend\n", "fn f: Number\nend\n");
    check("fn Vec.-:Vec\nend\n", "fn Vec.-: Vec\nend\n");
}

// ---- Patterns ----------------------------------------------------------------

#[test]
fn patterns() {
    check("let {a,b} = tbl\n", "let {a, b} = tbl\n");
    check("let { a : Number , b : String } = tbl\n", "let {a: Number, b: String} = tbl\n");
    same("let {key: value} = tbl\n");
    same("let {key: value: Number} = tbl\n");
    same("let [a, b, *rest] = tbl\n");
    same("let [a, *] = tbl\n");
    check("case v\nwhen [ 123 , 456 ]\nx\nwhen { 1 : 123 , 2 : 456 }\ny\nwhen -1..5\nz\nwhen 'a'\nw\nend\n", "case v\nwhen [123, 456]\n  x\nwhen {1: 123, 2: 456}\n  y\nwhen -1..5\n  z\nwhen 'a'\n  w\nend\n");
    same("case v\nwhen Circle {r: 1} as c\n  x\nwhen Direction.North, Direction.East\n  y\nwhen is Number\n  z\nend\n");
    same("fn foo({a: var1, b: var2}: Foo)\nend\n");
}

// ---- Comments ----------------------------------------------------------------

#[test]
fn comments_keep_text_and_place() {
    same("# this is a comment\nlet a = 1 # trailing\n");
    check("let a = 1    # aligned\nlet bb = 2   # aligned\n", "let a = 1    # aligned\nlet bb = 2   # aligned\n");
    check("let a  = 1\t# tab\n", "let a = 1 # tab\n");
    check("fn foo # trailing\n# own line\nx\n# before end\nend\n", "fn foo # trailing\n  # own line\n  x\n  # before end\nend\n");
    check("fn foo\n# only a comment\nend\n", "fn foo\n  # only a comment\nend\n");
    check("struct Foo\na: Number # f\n# before end\nend\n", "struct Foo\n  a: Number # f\n  # before end\nend\n");
    check("case x\n# before when\nwhen 1\na\n# in when\nelse\nb\nend\n", "case x\n# before when\nwhen 1\n  a\n  # in when\nelse\n  b\nend\n");
    check("let t = {\na: 1, # c\n# own\n}\n", "let t = {\n  a: 1, # c\n  # own\n}\n");
    check("enum E\n# first\nA\nend\n", "enum E\n  # first\n  A\nend\n");
    check("f(\n# arg\na)\n", "f(\n  # arg\n  a)\n");
    check("fn f\n  x +\n  # between\n  y\nend\n", "fn f\n  x +\n    # between\n    y\nend\n");
}

#[test]
fn doc_comments() {
    check("## doc\nfn bar\nend\n", "## doc\nfn bar\nend\n");
    check("module M\n## doc\n# more\nfn bar\nend\nend\n", "module M\n  ## doc\n  # more\n  fn bar\n  end\nend\n");
    check("struct Foo\n## the field\na: Number\nend\n", "struct Foo\n  ## the field\n  a: Number\nend\n");
    check("  ## doc\n  struct Foo\n  end\n", "## doc\nstruct Foo\nend\n");
}

#[test]
fn comment_aligned_under_trailing_comment_stays_aligned() {
    check(
        "global C # globals must follow PascalCase\n         # and are discouraged\nlet d: Number # typed\n              # inferred otherwise\n              # really\n",
        "global C # globals must follow PascalCase\n         # and are discouraged\nlet d: Number # typed\n              # inferred otherwise\n              # really\n",
    );
    check(
        "let x =   1 # one\n            # two\n",
        "let x = 1 # one\n          # two\n",
    );
    check("fn foo(a: Number): nil # if one\n                       # the return\nend\n", "fn foo(a: Number): nil # if one\n                       # the return\nend\n");
}

// ---- Declaration files -------------------------------------------------------

#[test]
fn declaration_files() {
    check_any(
        "# comment\ndeclare module Foo=:foo\n## doc\ncoolness : Number as 'c'\nfn largest( a : Number , * b : Number ) : Number as 'L'\nconst x : Number\nstruct P\nfn new(self) : P\nfn +(right: P): P\nfn [](key: Number): Number\nend\nend\ndeclare type Player : Entity\nend\ndeclare fn local_player(): Player = :LocalPlayer\ndeclare const max: Number = :config.maxPlayers\ndeclare module Json = :require('json')\nfn encode(value: any): String\nend\n",
        "# comment\ndeclare module Foo = :foo\n  ## doc\n  coolness: Number as 'c'\n  fn largest(a: Number, *b: Number): Number as 'L'\n  const x: Number\n  struct P\n    fn new(self): P\n    fn +(right: P): P\n    fn [](key: Number): Number\n  end\nend\ndeclare type Player : Entity\nend\ndeclare fn local_player(): Player = :LocalPlayer\ndeclare const max: Number = :config.maxPlayers\ndeclare module Json = :require('json')\n  fn encode(value: any): String\nend\n",
        true,
    );
}

// ---- Errors ------------------------------------------------------------------

#[test]
fn syntax_errors_are_not_formatted() {
    let err = format("let x = (1\n", FormatOptions::default()).unwrap_err();
    assert!(!err.is_empty());
    assert!(err.iter().any(|d| d.is_error()));
    let err = format_decl("declare fn f(\n", FormatOptions::default()).unwrap_err();
    assert!(err.iter().any(|d| d.is_error()));
}

#[test]
fn warnings_do_not_stop_formatting() {
    // `fn f() :Number` warns and is still formatted.
    let out = format("fn f()  :Number\nend\n", FormatOptions::default()).unwrap();
    assert_eq!(out.text, "fn f() :Number\nend\n");
}

// ---- Continued strings and --fix ---------------------------------------------

/// The decoded value of every string literal of `src`, in order.
fn string_values(src: &str) -> Vec<Vec<Vec<u8>>> {
    let p = parse(src);
    assert!(p.diagnostics().is_empty(), "{src:?} has diagnostics: {:?}", p.diagnostics());
    p.syntax()
        .descendants()
        .filter(|n| matches!(n.kind(), SyntaxKind::LITERAL | SyntaxKind::INTERP_STRING))
        .filter_map(|n| {
            let parts: Vec<String> = n
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| t.kind().is_string_part())
                .map(|t| t.text().to_string())
                .collect();
            if parts.is_empty() {
                return None;
            }
            let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
            Some(decode_string(&refs))
        })
        .collect()
}

fn check_fix(src: &str, expected: &str) {
    let out = run(src, false, true);
    assert_eq!(out.text, expected, "fixing {src:?}");
    assert_eq!(string_values(src), string_values(expected), "the values changed for {src:?}");
    assert_eq!(run(expected, false, true).text, expected, "not idempotent after the fix");
    assert!(run(expected, false, true).continued.is_empty());
}

#[test]
fn continued_strings_are_reported_and_kept_without_fix() {
    let src = "let foo = \"aaa \\\nbbb \\\nccc\"\nprint foo\n";
    let out = run(src, false, false);
    assert_eq!(out.text, src);
    assert_eq!(out.continued.len(), 1);
    let c = &out.continued[0];
    assert_eq!(u32::from(c.range.start()), 10);
    assert!(!c.fixed && !c.in_interpolation);
    // An escaped backslash before a line break is not a continuation; such
    // a string does not parse anyway.
    assert!(!super::has_continuation("\"a\\\\"));
    assert!(super::has_continuation("\"a\\\\\\\nb\""));
}

#[test]
fn fix_plain_string() {
    check_fix(
        "let foo = \"aaa \\\nbbb \\\nccc\"\nprint foo\n",
        "let foo = \"\"\"\naaa \nbbb \nccc\n\"\"\"\nprint foo\n",
    );
    check_fix("let x = \"a\\\n\"\n", "let x = \"\"\"\na\n\n\"\"\"\n");
    check_fix("let x = \"\\\nb\"\n", "let x = \"\"\"\n\nb\n\"\"\"\n");
}

#[test]
fn fix_keeps_what_follows_the_string() {
    check_fix("let n = \"one \\\ntwo\".len\n", "let n = \"\"\"\none \ntwo\n\"\"\".len\n");
    check_fix("f(\"a \\\nb\", x)\n", "f(\"\"\"\na \nb\n\"\"\", x)\n");
    check_fix("print \"a \\\nb\", 1\n", "print \"\"\"\na \nb\n\"\"\", 1\n");
    check_fix("if x == \"a \\\nb\"\n  y\nend\n", "if x == \"\"\"\na \nb\n\"\"\"\n  y\nend\n");
}

#[test]
fn fix_keeps_escapes_and_interpolations() {
    check_fix(
        "let s = \"tab\\t \\\"q\\\" \\\\ \\#{x} \\\n  #{name} and #{name.upcase} \\\nbye\"\n",
        "let s = \"\"\"\ntab\\t \\\"q\\\" \\\\ \\#{x} \n  #{name} and #{name.upcase} \nbye\n\"\"\"\n",
    );
    check_fix(
        "let s = \"a#{x} \\\n  b#{y}\\\nc\"\n",
        "let s = \"\"\"\na#{x} \n  b#{y}\nc\n\"\"\"\n",
    );
    check_fix("let s = \"x\\\n  \\\"\\\"\\\"y\"\n", "let s = \"\"\"\nx\n  \\\"\\\"\\\"y\n\"\"\"\n");
}

#[test]
fn fix_escapes_a_triple_quote_at_line_start() {
    // The rule is applied even though a valid `"…"` string cannot produce
    // such a line.
    assert_eq!(super::content_lines("\"\"\"x", true), "\\\"\"\"x");
    assert_eq!(super::content_lines("  \"\"\"x", true), "  \\\"\"\"x");
    assert_eq!(super::content_lines("a\\\n\"\"\"x", true), "a\n\\\"\"\"x");
    assert_eq!(super::content_lines("}\"\"\"x", false), "}\"\"\"x");
}

#[test]
fn fix_keeps_crlf_line_breaks() {
    check_fix("let x = \"a \\\r\nb\"\r\n", "let x = \"\"\"\r\na \r\nb\r\n\"\"\"\r\n");
}

#[test]
fn fix_keeps_cr_line_breaks() {
    check_fix("let x = \"a \\\rb\"\rprint x\r", "let x = \"\"\"\ra \rb\r\"\"\"\rprint x\r");
}

#[test]
fn continued_string_in_an_interpolation_is_not_fixed() {
    let src = "let s = \"outer #{\"inner \\\nx\"} end\"\n";
    let out = run(src, false, true);
    assert_eq!(out.text, src);
    assert_eq!(out.continued.len(), 1);
    assert!(out.continued[0].in_interpolation);
    assert!(!out.continued[0].fixed);
    let src = "let s = \"\"\"\nouter #{\"inner \\\nx\"} end\n\"\"\"\n";
    let out = run(src, false, true);
    assert_eq!(out.text, src);
    assert!(out.continued[0].in_interpolation);
}

#[test]
fn fix_reports_each_string_once() {
    let src = "let a = \"x \\\ny\"\nlet b = \"#{a} \\\nz\"\n";
    let out = run(src, false, true);
    assert_eq!(out.continued.len(), 2);
    assert!(out.continued.iter().all(|c| c.fixed));
    assert_eq!(out.text, "let a = \"\"\"\nx \ny\n\"\"\"\nlet b = \"\"\"\n#{a} \nz\n\"\"\"\n");
    assert_eq!(string_values(src), string_values(&out.text));
}
