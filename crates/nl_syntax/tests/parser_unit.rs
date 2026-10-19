//! Unit tests of the parser for the rules most likely to go wrong. Trees are
//! compared as compact S-expressions without trivia: `(KIND child …)`, with
//! tokens written as their text.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use nl_syntax::{Parse, SyntaxKind, SyntaxNode, doc_comment, parse};

fn sexp(node: &SyntaxNode, out: &mut String) {
    out.push('(');
    out.push_str(&format!("{:?}", node.kind()));
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => {
                out.push(' ');
                sexp(&n, out);
            }
            rowan::NodeOrToken::Token(t) => {
                if !t.kind().is_trivia() {
                    out.push(' ');
                    out.push_str(t.text());
                }
            }
        }
    }
    out.push(')');
}

fn checked(src: &str) -> Parse {
    let p = parse(src);
    assert_eq!(p.syntax().to_string(), src, "lossless");
    p
}

/// The statements of `src`, one S-expression per line; `src` must parse
/// without diagnostics.
fn tree(src: &str) -> String {
    let p = checked(src);
    let diags: Vec<_> = p.diagnostics().iter().map(|d| d.message.clone()).collect();
    assert!(diags.is_empty(), "unexpected diagnostics for {src:?}: {diags:#?}");
    statements(&p)
}

fn statements(p: &Parse) -> String {
    let block = p.syntax().children().find(|n| n.kind() == SyntaxKind::BLOCK).unwrap();
    let mut lines = Vec::new();
    for stmt in block.children() {
        let mut s = String::new();
        sexp(&stmt, &mut s);
        lines.push(s);
    }
    lines.join("\n")
}

/// The value of `x = <expr>`.
fn rhs(expr: &str) -> String {
    let t = tree(&format!("x = {expr}"));
    let prefix = "(ASSIGN_STMT (NAME_REF x) = ";
    assert!(t.starts_with(prefix), "{t}");
    t[prefix.len()..t.len() - 1].to_string()
}

/// The error messages of `src`.
fn errors(src: &str) -> Vec<String> {
    checked(src).diagnostics().iter().filter(|d| d.is_error()).map(|d| d.message.clone()).collect()
}

/// `src` has exactly one error, and its message contains `needle`.
fn one_error(src: &str, needle: &str) {
    let errs = errors(src);
    assert_eq!(errs.len(), 1, "expected one error for {src:?}, got {errs:#?}");
    assert!(errs[0].contains(needle), "for {src:?}: {:?} does not contain {needle:?}", errs[0]);
}

const A: &str = "(NAME_REF a)";
const B: &str = "(NAME_REF b)";
const C: &str = "(NAME_REF c)";

// ---- Precedence and associativity --------------------------------------------

#[test]
fn precedence_levels() {
    assert_eq!(rhs("a + b * c"), format!("(BIN_EXPR {A} + (BIN_EXPR {B} * {C}))"));
    assert_eq!(rhs("a * b + c"), format!("(BIN_EXPR (BIN_EXPR {A} * {B}) + {C})"));
    assert_eq!(rhs("a or b and c"), format!("(BIN_EXPR {A} or (BIN_EXPR {B} and {C}))"));
    assert_eq!(rhs("a || b && c"), format!("(BIN_EXPR {A} || (BIN_EXPR {B} && {C}))"));
    assert_eq!(
        rhs("a == b and c"),
        format!("(BIN_EXPR (BIN_EXPR {A} == {B}) and {C})")
    );
    assert_eq!(rhs("a | b ^ c"), format!("(BIN_EXPR {A} | (BIN_EXPR {B} ^ {C}))"));
    assert_eq!(rhs("a ^ b & c"), format!("(BIN_EXPR {A} ^ (BIN_EXPR {B} & {C}))"));
    assert_eq!(rhs("a & b << c"), format!("(BIN_EXPR {A} & (BIN_EXPR {B} << {C}))"));
    assert_eq!(rhs("a << b <> c"), format!("(BIN_EXPR {A} << (BIN_EXPR {B} <> {C}))"));
    assert_eq!(rhs("a <> b + c"), format!("(BIN_EXPR {A} <> (BIN_EXPR {B} + {C}))"));
    assert_eq!(rhs("a + b .. c"), format!("(BIN_EXPR (BIN_EXPR {A} + {B}) .. {C})"));
    assert_eq!(rhs("a .. b == c"), format!("(BIN_EXPR (BIN_EXPR {A} .. {B}) == {C})"));
    assert_eq!(rhs("a |> f == c"), format!("(BIN_EXPR (PIPE_EXPR {A} |> (NAME_REF f)) == {C})"));
    assert_eq!(rhs("a + b |> f"), format!("(PIPE_EXPR (BIN_EXPR {A} + {B}) |> (NAME_REF f))"));
    // `??` is on the level of `or`.
    assert_eq!(rhs("a ?? b and c"), format!("(BIN_EXPR {A} ?? (BIN_EXPR {B} and {C}))"));
    assert_eq!(rhs("a or b ?? c"), format!("(BIN_EXPR (BIN_EXPR {A} or {B}) ?? {C})"));
    assert_eq!(rhs("a ?? b or c"), format!("(BIN_EXPR (BIN_EXPR {A} ?? {B}) or {C})"));
    assert_eq!(rhs("a ?? b == c"), format!("(BIN_EXPR {A} ?? (BIN_EXPR {B} == {C}))"));
    assert_eq!(rhs("a?.b ?? c"), format!("(BIN_EXPR (FIELD_EXPR {A} ?. b) ?? {C})"));
    assert_eq!(rhs("-a?.b"), format!("(PREFIX_EXPR - (FIELD_EXPR {A} ?. b))"));
}

#[test]
fn associativity() {
    assert_eq!(rhs("a - b - c"), format!("(BIN_EXPR (BIN_EXPR {A} - {B}) - {C})"));
    assert_eq!(rhs("a / b * c"), format!("(BIN_EXPR (BIN_EXPR {A} / {B}) * {C})"));
    assert_eq!(rhs("a <> b <> c"), format!("(BIN_EXPR {A} <> (BIN_EXPR {B} <> {C}))"));
    assert_eq!(rhs("a ** b ** c"), format!("(BIN_EXPR {A} ** (BIN_EXPR {B} ** {C}))"));
    assert_eq!(rhs("a |> f |> g"), format!("(PIPE_EXPR (PIPE_EXPR {A} |> (NAME_REF f)) |> (NAME_REF g))"));
    assert_eq!(rhs("a ?? b ?? c"), format!("(BIN_EXPR (BIN_EXPR {A} ?? {B}) ?? {C})"));
    assert_eq!(
        rhs("x as A as B"),
        "(AS_EXPR (AS_EXPR (NAME_REF x) as (NAMED_TYPE (QUAL_NAME A))) as (NAMED_TYPE (QUAL_NAME B)))"
    );
}

#[test]
fn non_associative_levels() {
    one_error("x = a < b < c", "comparisons cannot be chained");
    one_error("x = a == b == c", "comparisons cannot be chained");
    one_error("x = x is A is B", "comparisons cannot be chained");
    one_error("x = 1..2..3", "ranges cannot be chained");
    assert!(errors("x = a < b and b < c").is_empty());
}

#[test]
fn unary_and_power() {
    assert_eq!(rhs("-x ** 2"), "(PREFIX_EXPR - (BIN_EXPR (NAME_REF x) ** (LITERAL 2)))");
    assert_eq!(rhs("2 ** -1"), "(BIN_EXPR (LITERAL 2) ** (PREFIX_EXPR - (LITERAL 1)))");
    assert_eq!(rhs("-5.abs"), "(PREFIX_EXPR - (FIELD_EXPR (LITERAL 5) . abs))");
    assert_eq!(rhs("- - a"), format!("(PREFIX_EXPR - (PREFIX_EXPR - {A}))"));
    assert_eq!(rhs("!a && b"), format!("(BIN_EXPR (PREFIX_EXPR ! {A}) && {B})"));
    assert_eq!(rhs("-3..-1"), "(BIN_EXPR (PREFIX_EXPR - (LITERAL 3)) .. (PREFIX_EXPR - (LITERAL 1)))");
    assert_eq!(tree("-5.abs"), "(EXPR_STMT (PREFIX_EXPR - (FIELD_EXPR (LITERAL 5) . abs)))");
}

#[test]
fn is_and_as() {
    assert_eq!(
        rhs("x is A | B and y"),
        "(BIN_EXPR (IS_EXPR (NAME_REF x) is (UNION_TYPE (NAMED_TYPE (QUAL_NAME A)) | (NAMED_TYPE \
         (QUAL_NAME B)))) and (NAME_REF y))"
    );
    assert_eq!(
        rhs("not x is T"),
        "(IS_EXPR (PREFIX_EXPR not (NAME_REF x)) is (NAMED_TYPE (QUAL_NAME T)))"
    );
    assert_eq!(rhs("x as T | y"), "(BIN_EXPR (AS_EXPR (NAME_REF x) as (NAMED_TYPE (QUAL_NAME T))) | (NAME_REF y))");
    assert_eq!(
        rhs("x as Number < 10"),
        "(BIN_EXPR (AS_EXPR (NAME_REF x) as (NAMED_TYPE (QUAL_NAME Number))) < (LITERAL 10))"
    );
}

#[test]
fn pipes() {
    assert_eq!(rhs("a |> f(b)"), format!("(PIPE_EXPR {A} |> (CALL_EXPR (NAME_REF f) (ARG_LIST ( {B} ))))"));
    assert_eq!(rhs("a |> x:m(b)"), format!("(PIPE_EXPR {A} |> (METHOD_CALL (NAME_REF x) : m (ARG_LIST ( {B} ))))"));
    assert_eq!(rhs("a |> :string.upper"), format!("(PIPE_EXPR {A} |> (LUA_PATH : string (LUA_FIELD . upper)))"));
    assert_eq!(
        rhs("a |> x.y.m"),
        format!("(PIPE_EXPR {A} |> (FIELD_EXPR (FIELD_EXPR (NAME_REF x) . y) . m))")
    );
    one_error("x = a |> f(b).await", "write `(a |> f(b)).await`");
    one_error("x = a |> f + 1", "put the pipe in parentheses");
    one_error("x = a |> f b", "a pipe target takes no arguments without parentheses");
    one_error("x = a |> Foo", "the target of `|>` must name a function");
}

// ---- Newlines ----------------------------------------------------------------

#[test]
fn continuation_lines() {
    let sum = "(LET_DECL let (BIND_PAT (NAME t)) = (EXPR_LIST (BIN_EXPR (NAME_REF a) + (NAME_REF b))))";
    assert_eq!(tree("let t = a +\n  b"), sum);
    assert_eq!(tree("let t = a\n  + b"), sum);
    assert_eq!(tree("let t =\n  a + b"), sum);
    assert_eq!(tree("let n = p\n  .name"), "(LET_DECL let (BIND_PAT (NAME n)) = (EXPR_LIST (FIELD_EXPR (NAME_REF p) . name)))");
    assert_eq!(tree("let n = p.\n  name"), "(LET_DECL let (BIND_PAT (NAME n)) = (EXPR_LIST (FIELD_EXPR (NAME_REF p) . name)))");
    assert_eq!(tree("a\n|> f\n|> g"), "(EXPR_STMT (PIPE_EXPR (PIPE_EXPR (NAME_REF a) |> (NAME_REF f)) |> (NAME_REF g)))");
    assert_eq!(tree("x = a\n  and b"), format!("(ASSIGN_STMT (NAME_REF x) = (BIN_EXPR {A} and {B}))"));
    assert_eq!(tree("x = a\n  ..b"), format!("(ASSIGN_STMT (NAME_REF x) = (BIN_EXPR {A} .. {B}))"));
    assert_eq!(
        tree("print \"a\",\n  \"b\""),
        "(EXPR_STMT (COMMAND_CALL (NAME_REF print) (COMMAND_ARGS (LITERAL \"a\") , (LITERAL \"b\"))))"
    );
    assert_eq!(tree("type S = A\n  | B"), "(TYPE_DECL type (NAME S) = (UNION_TYPE (NAMED_TYPE (QUAL_NAME A)) | (NAMED_TYPE (QUAL_NAME B))))");
    assert_eq!(tree("type S = A |\n  B"), "(TYPE_DECL type (NAME S) = (UNION_TYPE (NAMED_TYPE (QUAL_NAME A)) | (NAMED_TYPE (QUAL_NAME B))))");
}

#[test]
fn line_starts_that_begin_a_statement() {
    assert_eq!(tree("x = a\n-b"), format!("(ASSIGN_STMT (NAME_REF x) = {A})\n(EXPR_STMT (PREFIX_EXPR - {B}))"));
    assert_eq!(tree("f\n(x)"), "(EXPR_STMT (NAME_REF f))\n(EXPR_STMT (PAREN_EXPR ( (NAME_REF x) )))");
    assert_eq!(tree("x = a\n!b"), format!("(ASSIGN_STMT (NAME_REF x) = {A})\n(EXPR_STMT (PREFIX_EXPR ! {B}))"));
    assert_eq!(tree("x = a\n~b"), format!("(ASSIGN_STMT (NAME_REF x) = {A})\n(EXPR_STMT (PREFIX_EXPR ~ {B}))"));
    assert_eq!(tree("x = a\n{b: 1}"), format!("(ASSIGN_STMT (NAME_REF x) = {A})\n(EXPR_STMT (TABLE_EXPR {{ (TABLE_PAIR (TABLE_KEY b) : (LITERAL 1)) }}))"));
    assert_eq!(tree("x = a\n[b]"), format!("(ASSIGN_STMT (NAME_REF x) = {A})\n(EXPR_STMT (LIST_EXPR [ {B} ]))"));
    let p = checked("x = a\n[b]");
    assert_eq!(statements(&p).lines().count(), 2, "`[` on a new line is not an index");
    assert!(p.diagnostics().is_empty(), "a list on its own line");
    assert_eq!(tree("x = a\n:print(b)").lines().count(), 2);
    assert_eq!(tree("x = a\nf"), format!("(ASSIGN_STMT (NAME_REF x) = {A})\n(EXPR_STMT (NAME_REF f))"));
    // `f` then a new line: the command arguments must start on its line.
    assert_eq!(tree("print\nx").lines().count(), 2);
}

#[test]
fn brackets_ignore_line_breaks() {
    assert_eq!(
        tree("print(1 +\n  1)"),
        "(EXPR_STMT (CALL_EXPR (NAME_REF print) (ARG_LIST ( (BIN_EXPR (LITERAL 1) + (LITERAL 1)) ))))"
    );
    assert_eq!(
        tree("x = [\n  1,\n  2,\n]"),
        "(ASSIGN_STMT (NAME_REF x) = (LIST_EXPR [ (LITERAL 1) , (LITERAL 2) , ]))"
    );
    assert_eq!(tree("x = (a\n  - b)"), format!("(ASSIGN_STMT (NAME_REF x) = (PAREN_EXPR ( (BIN_EXPR {A} - {B}) )))"));
}

#[test]
fn bodies_inside_brackets_have_statements_again() {
    assert_eq!(
        tree("f(fn(x)\n  a\n  b\nend)"),
        format!(
            "(EXPR_STMT (CALL_EXPR (NAME_REF f) (ARG_LIST ( (FN_EXPR fn (PARAM_LIST ( (PARAM (BIND_PAT (NAME \
             x))) )) (BLOCK (EXPR_STMT {A}) (EXPR_STMT {B})) end) ))))"
        )
    );
    // A header inside parentheses ends at the line break.
    assert_eq!(
        tree("f(if a\n  b\nend)"),
        format!("(EXPR_STMT (CALL_EXPR (NAME_REF f) (ARG_LIST ( (IF_EXPR if {A} (BLOCK (EXPR_STMT {B})) end) ))))")
    );
    // Inside an interpolation of a heredoc.
    assert!(errors("x = \"\"\"\n#{ if a\n  b\nend }\n\"\"\"").is_empty());
}

#[test]
fn semicolons() {
    assert_eq!(tree("a; b;; c;").lines().count(), 3);
    assert_eq!(
        tree("if a; b; else; c; end"),
        format!("(EXPR_STMT (IF_EXPR if {A} ; (BLOCK (EXPR_STMT {B}) ;) (ELSE_CLAUSE else (BLOCK ; (EXPR_STMT {C}) ;)) end))")
    );
    one_error("f(a; b)", "`;` cannot separate items inside brackets");
}

#[test]
fn statement_separation() {
    one_error("x = 1 y = 2", "statements are separated by line breaks or `;`");
    one_error("if a\n  b\nend if x", "cannot follow a block that ends in `end`");
}

// ---- Command calls -----------------------------------------------------------

#[test]
fn command_calls() {
    assert_eq!(tree("f a, b"), format!("(EXPR_STMT (COMMAND_CALL (NAME_REF f) (COMMAND_ARGS {A} , {B})))"));
    assert_eq!(tree("f -1"), "(EXPR_STMT (BIN_EXPR (NAME_REF f) - (LITERAL 1)))");
    assert_eq!(tree("f *a"), format!("(EXPR_STMT (BIN_EXPR (NAME_REF f) * {A}))"));
    assert_eq!(tree("f [1]"), "(EXPR_STMT (INDEX_EXPR (NAME_REF f) [ (LITERAL 1) ]))");
    assert_eq!(tree("f (a)"), format!("(EXPR_STMT (CALL_EXPR (NAME_REF f) (ARG_LIST ( {A} ))))"));
    assert_eq!(
        tree("f (a + b) * 2"),
        format!("(EXPR_STMT (BIN_EXPR (CALL_EXPR (NAME_REF f) (ARG_LIST ( (BIN_EXPR {A} + {B}) ))) * (LITERAL 2)))")
    );
    assert_eq!(tree("f ..a"), format!("(EXPR_STMT (BIN_EXPR (NAME_REF f) .. {A}))"));
    assert_eq!(tree("f !a"), format!("(EXPR_STMT (COMMAND_CALL (NAME_REF f) (COMMAND_ARGS (PREFIX_EXPR ! {A}))))"));
    assert_eq!(tree("f ~a"), format!("(EXPR_STMT (COMMAND_CALL (NAME_REF f) (COMMAND_ARGS (PREFIX_EXPR ~ {A}))))"));
    assert_eq!(tree("f and a"), format!("(EXPR_STMT (BIN_EXPR (NAME_REF f) and {A}))"));
    assert_eq!(tree("f a, *b"), format!("(EXPR_STMT (COMMAND_CALL (NAME_REF f) (COMMAND_ARGS {A} , (SPREAD_ARG * {B}))))"));
    assert_eq!(tree("print :type(a)"), format!("(EXPR_STMT (COMMAND_CALL (NAME_REF print) (COMMAND_ARGS (LUA_PATH : type (LUA_CALL (ARG_LIST ( {A} )))))))"));
    assert_eq!(tree("a b, c(d)"), format!("(EXPR_STMT (COMMAND_CALL {A} (COMMAND_ARGS {B} , (CALL_EXPR {C} (ARG_LIST ( (NAME_REF d) ))))))"));
    // Callee shapes.
    assert_eq!(tree("@on_hit a"), format!("(EXPR_STMT (COMMAND_CALL (AT_EXPR @on_hit) (COMMAND_ARGS {A})))"));
    assert_eq!(tree("t.type a"), format!("(EXPR_STMT (COMMAND_CALL (FIELD_EXPR (NAME_REF t) . type) (COMMAND_ARGS {A})))"));
    assert_eq!(tree("x:m a"), format!("(EXPR_STMT (COMMAND_CALL (METHOD_CALL (NAME_REF x) : m) (COMMAND_ARGS {A})))"));
    assert_eq!(tree(":http.request a"), format!("(EXPR_STMT (COMMAND_CALL (LUA_PATH : http (LUA_FIELD . request)) (COMMAND_ARGS {A})))"));
    assert_eq!(
        tree("Regex.new(',').split \"a\""),
        "(EXPR_STMT (COMMAND_CALL (FIELD_EXPR (CALL_EXPR (FIELD_EXPR (NAME_REF Regex) . new) (ARG_LIST ( (LITERAL ',') ))) . split) (COMMAND_ARGS (LITERAL \"a\"))))"
    );
    // Positions: value of `=`, last of a list, `return`, headers, modifier.
    assert_eq!(tree("x = f a"), format!("(ASSIGN_STMT (NAME_REF x) = (COMMAND_CALL (NAME_REF f) (COMMAND_ARGS {A})))"));
    assert_eq!(
        tree("let a, b = 1, f x, y"),
        "(LET_DECL let (BIND_PAT (NAME a)) , (BIND_PAT (NAME b)) = (EXPR_LIST (LITERAL 1) , (COMMAND_CALL (NAME_REF f) (COMMAND_ARGS (NAME_REF x) , (NAME_REF y)))))"
    );
    assert_eq!(tree("return f a, b"), format!("(RETURN_STMT return (EXPR_LIST (COMMAND_CALL (NAME_REF f) (COMMAND_ARGS {A} , {B}))))"));
    assert!(tree("for m in date.scan text\nend").contains("(COMMAND_CALL (FIELD_EXPR (NAME_REF date) . scan)"));
}

#[test]
fn command_call_errors() {
    one_error("a b, c d", "command calls cannot be nested: write `c(d)`");
    one_error("add(widget {x: 1})", "write `widget({x: 1})`");
    one_error("x = (f a)", "a call without parentheses is only allowed");
    one_error("x = 1 + f a", "a call without parentheses is only allowed");
    one_error("Foo x", "a capitalized name is not called without parentheses");
    one_error("f! x", "`f!` is the function itself and cannot be called");
    one_error("f(a) b", "already has its arguments in parentheses");
}

// ---- Trailing blocks ---------------------------------------------------------

#[test]
fn trailing_block_attachment() {
    // In a command call: to the command call, not to the parenthesized call in it.
    assert_eq!(
        tree("a b(c) do\n  d\nend"),
        format!("(EXPR_STMT (COMMAND_CALL {A} (COMMAND_ARGS (CALL_EXPR {B} (ARG_LIST ( {C} )))) (TRAILING_BLOCK do (BLOCK (EXPR_STMT (NAME_REF d))) end)))")
    );
    // Inside brackets, to the call before it again.
    assert_eq!(
        tree("a f(g(x) do\nend)"),
        format!("(EXPR_STMT (COMMAND_CALL {A} (COMMAND_ARGS (CALL_EXPR (NAME_REF f) (ARG_LIST ( (CALL_EXPR (NAME_REF g) (ARG_LIST ( (NAME_REF x) )) (TRAILING_BLOCK do (BLOCK) end)) ))))))")
    );
    // Inside a block body nested in an argument, to the call before it again.
    assert!(tree("a fn() items.each do\nend end").contains("(CALL_EXPR (FIELD_EXPR (NAME_REF items) . each) (TRAILING_BLOCK"));
    // To the call or callee before it.
    assert_eq!(tree("f(a) do end"), format!("(EXPR_STMT (CALL_EXPR (NAME_REF f) (ARG_LIST ( {A} )) (TRAILING_BLOCK do (BLOCK) end)))"));
    assert_eq!(tree("emitter.on do end"), "(EXPR_STMT (CALL_EXPR (FIELD_EXPR (NAME_REF emitter) . on) (TRAILING_BLOCK do (BLOCK) end)))");
    assert_eq!(
        tree("Task.new do |resolve, reject|\nend"),
        "(EXPR_STMT (CALL_EXPR (FIELD_EXPR (NAME_REF Task) . new) (TRAILING_BLOCK do (BLOCK_PARAMS | (PARAM (BIND_PAT (NAME resolve))) , (PARAM (BIND_PAT (NAME reject))) |) (BLOCK) end)))"
    );
    assert_eq!(tree("x:m do end"), "(EXPR_STMT (METHOD_CALL (NAME_REF x) : m (TRAILING_BLOCK do (BLOCK) end)))");
    // Postfix operators after `end`.
    assert_eq!(
        tree("items.map do |x| x end.sum"),
        "(EXPR_STMT (FIELD_EXPR (CALL_EXPR (FIELD_EXPR (NAME_REF items) . map) (TRAILING_BLOCK do (BLOCK_PARAMS | (PARAM (BIND_PAT (NAME x))) |) (BLOCK (EXPR_STMT (NAME_REF x))) end)) . sum))"
    );
    // A `do` that starts a line is a scope block.
    assert_eq!(tree("f\ndo\nend"), "(EXPR_STMT (NAME_REF f))\n(DO_BLOCK do (BLOCK) end)");
    // `async do`.
    assert_eq!(tree("start async do end"), "(EXPR_STMT (CALL_EXPR (NAME_REF start) (TRAILING_BLOCK async do (BLOCK) end)))");
    // A trailing block in a header.
    assert!(tree("if items.any do |x| x > 0 end\n  y\nend").starts_with("(EXPR_STMT (IF_EXPR if (CALL_EXPR"));
}

#[test]
fn block_parameters() {
    assert_eq!(tree("f do || 1 end"), "(EXPR_STMT (CALL_EXPR (NAME_REF f) (TRAILING_BLOCK do (BLOCK_PARAMS ||) (BLOCK (EXPR_STMT (LITERAL 1))) end)))");
    // A top-level `|` closes the list; parentheses group.
    assert!(tree("f do |a: (A | B), m = (x | y)|\nend").contains("(PAREN_TYPE ( (UNION_TYPE"));
    assert!(tree("f do |a, b = 1 ^ 2|\nend").contains("(PARAM (BIND_PAT (NAME b)) = (BIN_EXPR (LITERAL 1) ^ (LITERAL 2)))"));
    assert!(tree("f do |{x, y}|\nend").contains("(BRACE_PAT"));
    assert!(tree("apply 5 do |n: Number|: Number\n  n\nend").contains("|) (RETURN_TYPE : (NAMED_TYPE (QUAL_NAME Number))) (BLOCK"));
}

#[test]
fn trailing_block_errors() {
    one_error("x = 5 do\nend", "a `do … end` block passed to a call follows the call");
    one_error("f! do\nend", "nothing that calls may follow it");
    one_error("x = do\nend", "is a scope block, which is a statement");
}

// ---- Modifiers ---------------------------------------------------------------

#[test]
fn modifiers() {
    assert_eq!(tree("print if x"), "(EXPR_STMT (NAME_REF print) (MODIFIER if (NAME_REF x)))");
    assert_eq!(tree("return if done"), "(RETURN_STMT return (MODIFIER if (NAME_REF done)))");
    assert_eq!(tree("break if done"), "(BREAK_STMT break (MODIFIER if (NAME_REF done)))");
    assert_eq!(tree("x = 1 unless y"), "(ASSIGN_STMT (NAME_REF x) = (LITERAL 1) (MODIFIER unless (NAME_REF y)))");
    assert_eq!(tree("step() while busy"), "(EXPR_STMT (CALL_EXPR (NAME_REF step) (ARG_LIST ( ))) (MODIFIER while (NAME_REF busy)))");
    assert_eq!(
        tree("save if ask \"s?\""),
        "(EXPR_STMT (NAME_REF save) (MODIFIER if (COMMAND_CALL (NAME_REF ask) (COMMAND_ARGS (LITERAL \"s?\")))))"
    );
    assert_eq!(
        tree("f x do end if y"),
        "(EXPR_STMT (COMMAND_CALL (NAME_REF f) (COMMAND_ARGS (NAME_REF x)) (TRAILING_BLOCK do (BLOCK) end)) (MODIFIER if (NAME_REF y)))"
    );
    assert_eq!(
        tree("return amount / 2 if target.armored"),
        "(RETURN_STMT return (EXPR_LIST (BIN_EXPR (NAME_REF amount) / (LITERAL 2))) (MODIFIER if (FIELD_EXPR (NAME_REF target) . armored)))"
    );
    // At the start of an operand, `if` begins an expression.
    assert!(tree("x = if a\n  b\nend").starts_with("(ASSIGN_STMT (NAME_REF x) = (IF_EXPR"));
    one_error("a if b if c", "one modifier only");
    one_error("let x = 1 if y", "cannot follow a declaration");
    one_error("while a\nend if b", "cannot follow a loop");
}

// ---- The three roles of `:` --------------------------------------------------

#[test]
fn colons() {
    // Method call: touches both sides.
    assert_eq!(tree("conn:send(a)"), format!("(EXPR_STMT (METHOD_CALL (NAME_REF conn) : send (ARG_LIST ( {A} ))))"));
    assert_eq!(tree("x:m"), "(EXPR_STMT (METHOD_CALL (NAME_REF x) : m))");
    assert_eq!(tree("conn:sendMessage(a)"), format!("(EXPR_STMT (METHOD_CALL (NAME_REF conn) : sendMessage (ARG_LIST ( {A} ))))"));
    // Lua path: where an operand starts, touching the word after it.
    assert_eq!(tree("x = -:a.b"), "(ASSIGN_STMT (NAME_REF x) = (PREFIX_EXPR - (LUA_PATH : a (LUA_FIELD . b))))");
    assert_eq!(tree("x = 1..:n"), "(ASSIGN_STMT (NAME_REF x) = (BIN_EXPR (LITERAL 1) .. (LUA_PATH : n)))");
    assert_eq!(tree("x = !:cfg.on"), "(ASSIGN_STMT (NAME_REF x) = (PREFIX_EXPR ! (LUA_PATH : cfg (LUA_FIELD . on))))");
    assert_eq!(tree(":player.getHealth()"), "(EXPR_STMT (LUA_PATH : player (LUA_FIELD . getHealth) (LUA_CALL (ARG_LIST ( )))))");
    assert_eq!(
        tree(":hud.mainPanel:setVisible(true)"),
        "(EXPR_STMT (LUA_PATH : hud (LUA_FIELD . mainPanel) (LUA_METHOD : setVisible (ARG_LIST ( (LITERAL true) )))))"
    );
    // The path takes every step; parentheses end it.
    assert_eq!(tree(":a.b().c"), "(EXPR_STMT (LUA_PATH : a (LUA_FIELD . b) (LUA_CALL (ARG_LIST ( ))) (LUA_FIELD . c)))");
    assert_eq!(tree("(:a.b()).c"), "(EXPR_STMT (FIELD_EXPR (PAREN_EXPR ( (LUA_PATH : a (LUA_FIELD . b) (LUA_CALL (ARG_LIST ( )))) )) . c))");
    assert_eq!(tree(":config.maxPlayers = 8"), "(ASSIGN_STMT (LUA_PATH : config (LUA_FIELD . maxPlayers)) = (LITERAL 8))");
    // Plain colon: annotations and keys, whatever the spacing.
    assert_eq!(tree("let a :Number"), "(LET_DECL let (TYPED_PAT (BIND_PAT (NAME a)) : (NAMED_TYPE (QUAL_NAME Number))))");
    assert_eq!(tree("x = {a :1, f :x, g: :y}"), "(ASSIGN_STMT (NAME_REF x) = (TABLE_EXPR { (TABLE_PAIR (TABLE_KEY a) : (LITERAL 1)) , (TABLE_PAIR (TABLE_KEY f) : (NAME_REF x)) , (TABLE_PAIR (TABLE_KEY g) : (LUA_PATH : y)) }))");
    assert!(tree("struct A :B\nend").contains("(STRUCT_PARENT : (NAMED_TYPE (QUAL_NAME B)))"));
    // The overlap after an `fn` header.
    assert!(tree("fn f: T\nend").contains("(RETURN_TYPE : (NAMED_TYPE (QUAL_NAME T)))"));
    assert!(tree("fn f() : T\nend").contains("(RETURN_TYPE"));
    assert!(tree("fn Vec.-:Vec\nend").contains("(OP_NAME -) (RETURN_TYPE"));
    assert!(tree("f do |a|: nil\nend").contains("(RETURN_TYPE : (NAMED_TYPE nil))"));
    let p = checked("fn f() :Number\nend");
    assert!(!p.has_errors());
    assert_eq!(p.diagnostics().len(), 1, "the Lua-path warning");
    assert!(statements(&p).contains("(BLOCK (EXPR_STMT (LUA_PATH : Number)))"));
    one_error("x = a : b", "statements are separated");
}

// ---- Lua paths --------------------------------------------------------------

#[test]
fn lua_paths() {
    // The owner's example: parentheses end the path; `.divisible_by` is NL.
    assert_eq!(
        tree("(:LocalPlayer().getHealth()).divisible_by 10"),
        "(EXPR_STMT (COMMAND_CALL (FIELD_EXPR (PAREN_EXPR ( (LUA_PATH : LocalPlayer (LUA_CALL (ARG_LIST ( ))) (LUA_FIELD . getHealth) (LUA_CALL (ARG_LIST ( )))) )) . divisible_by) (COMMAND_ARGS (LITERAL 10))))"
    );
    // Every step kind in one chain; the arguments and indexes are NL.
    assert_eq!(
        tree(":a.b[k.len](x):m(y).c:n do |z| z end"),
        "(EXPR_STMT (LUA_PATH : a (LUA_FIELD . b) (LUA_INDEX [ (FIELD_EXPR (NAME_REF k) . len) ]) (LUA_CALL (ARG_LIST ( (NAME_REF x) ))) (LUA_METHOD : m (ARG_LIST ( (NAME_REF y) ))) (LUA_FIELD . c) (LUA_METHOD : n (TRAILING_BLOCK do (BLOCK_PARAMS | (PARAM (BIND_PAT (NAME z))) |) (BLOCK (EXPR_STMT (NAME_REF z))) end))))"
    );
    // Continued on lines that start with `.`; `(`, `[` and `:` on a new line
    // begin a new statement.
    assert_eq!(tree(":a\n  .b()\n  .c"), "(EXPR_STMT (LUA_PATH : a (LUA_FIELD . b) (LUA_CALL (ARG_LIST ( ))) (LUA_FIELD . c)))");
    assert_eq!(tree(":a.b\n(x)").lines().count(), 2);
    assert_eq!(tree(":a.b\n:c()").lines().count(), 2);
    // Names after any step: any style, keywords as names (remapped).
    assert_eq!(tree(":f().end:Type().getX"), "(EXPR_STMT (LUA_PATH : f (LUA_CALL (ARG_LIST ( ))) (LUA_FIELD . end) (LUA_METHOD : Type (ARG_LIST ( ))) (LUA_FIELD . getX)))");
    let p = checked(":f().end");
    assert!(p.syntax().descendants_with_tokens().filter_map(|e| e.into_token()).all(|t| t.kind() != SyntaxKind::END_KW));
    // Trailing blocks: after the first name, a `.name`, a `:name`, a call.
    assert_eq!(tree(":each do end"), "(EXPR_STMT (LUA_PATH : each (LUA_CALL (TRAILING_BLOCK do (BLOCK) end))))");
    assert_eq!(tree(":t.each do end"), "(EXPR_STMT (LUA_PATH : t (LUA_FIELD . each) (LUA_CALL (TRAILING_BLOCK do (BLOCK) end))))");
    assert_eq!(tree(":t:each do end"), "(EXPR_STMT (LUA_PATH : t (LUA_METHOD : each (TRAILING_BLOCK do (BLOCK) end))))");
    assert_eq!(tree(":f(a) do end"), "(EXPR_STMT (LUA_PATH : f (LUA_CALL (ARG_LIST ( (NAME_REF a) )) (TRAILING_BLOCK do (BLOCK) end))))");
    one_error(":t[1] do end", "cannot follow an index in a Lua path");
    // Command callees: a path ending in `.name` or `:name`, or a first name.
    for src in [":print a", ":http.request a", ":hud:show a", ":f().g a", ":f {a: 1}"] {
        assert!(tree(src).contains("(COMMAND_CALL (LUA_PATH"), "{src}");
    }
    one_error(":f() a", "already has its arguments in parentheses");
    one_error(":t[1] a", "statements are separated");
    one_error(":t:m() a", "already has its arguments in parentheses");
    one_error(":f() {a: 1}", "`{` cannot follow this Lua path");
    // Assignment targets: a path ending in `.name` or `[expr]`.
    for src in [":config.max = 8", ":players[1].hp = 0", ":game.get(\"x\").ready = true", ":t[k] = 1", ":a.b += 1"] {
        assert!(errors(src).is_empty(), "{src}: {:?}", errors(src));
    }
    one_error(":x = 1", "write `:_G.name = value`");
    one_error(":f() = 1", "a Lua path that ends in a call cannot be assigned to");
    one_error(":t:m = 1", "a Lua path that ends in a call cannot be assigned to");
    one_error(":t.each do end = 1", "a Lua path that ends in a call cannot be assigned to");
    // Pipe targets: names, an optional `:name`, one argument list.
    assert_eq!(rhs("a |> :t.f(b)"), format!("(PIPE_EXPR {A} |> (LUA_PATH : t (LUA_FIELD . f) (LUA_CALL (ARG_LIST ( {B} )))))"));
    assert_eq!(rhs("a |> :t:m(b)"), format!("(PIPE_EXPR {A} |> (LUA_PATH : t (LUA_METHOD : m (ARG_LIST ( {B} )))))"));
    one_error("x = a |> :f().g", "write `(a |> :f()).g`");
    one_error("x = a |> :t[1]", "nothing may follow a pipe target");
    // `!` after any step is an error; the path goes on.
    one_error("x = :f!", "takes no `!`");
    one_error("x = :f().g!", "takes no `!`");
    one_error("x = :a.b!.c", "takes no `!`");
    // `spawn` takes no path.
    one_error("spawn :lua.f(x)", "Worker.run");
}

// ---- Postfix `!` -------------------------------------------------------------

#[test]
fn postfix_bang() {
    assert_eq!(rhs("f!"), "(BANG_EXPR (NAME_REF f) !)");
    assert_eq!(rhs("x.m!"), "(BANG_EXPR (FIELD_EXPR (NAME_REF x) . m) !)");
    assert_eq!(rhs("x.end!"), "(BANG_EXPR (FIELD_EXPR (NAME_REF x) . end) !)");
    assert_eq!(rhs("@h!"), "(BANG_EXPR (AT_EXPR @h) !)");
    assert_eq!(rhs("ON_HIT!"), "(BANG_EXPR (NAME_REF ON_HIT) !)");
    assert_eq!(rhs("x.len!"), "(BANG_EXPR (FIELD_EXPR (NAME_REF x) . len) !)");
    assert_eq!(rhs("f!.x"), "(FIELD_EXPR (BANG_EXPR (NAME_REF f) !) . x)");
    assert_eq!(rhs("a!=b"), format!("(BIN_EXPR {A} != {B})"));
    assert_eq!(tree("if!done\nend"), "(EXPR_STMT (IF_EXPR if (PREFIX_EXPR ! (NAME_REF done)) (BLOCK) end))");
    one_error("x = f()!", "`!` cannot follow a call");
    one_error("x = t[1]!", "`!` can only follow a name");
    one_error("x = :f!", "a Lua path is never called automatically");
    one_error("x = f!(a)", "nothing that calls may follow it");
    one_error("a! = b", "`a!` is the function value itself");
}

// ---- Struct literals and tables ----------------------------------------------

#[test]
fn struct_literal_versus_call_with_table() {
    assert_eq!(
        tree("Monster { id: 1 }"),
        "(EXPR_STMT (STRUCT_LIT (NAME_REF Monster) { (FIELD_INIT id : (LITERAL 1)) }))"
    );
    assert_eq!(
        tree("f {a: 1}"),
        "(EXPR_STMT (COMMAND_CALL (NAME_REF f) (COMMAND_ARGS (TABLE_EXPR { (TABLE_PAIR (TABLE_KEY a) : (LITERAL 1)) }))))"
    );
    assert_eq!(
        tree("x.m {a: 1}"),
        "(EXPR_STMT (COMMAND_CALL (FIELD_EXPR (NAME_REF x) . m) (COMMAND_ARGS (TABLE_EXPR { (TABLE_PAIR (TABLE_KEY a) : (LITERAL 1)) }))))"
    );
    assert_eq!(rhs("Shapes.Circle { r: 1 }"), "(STRUCT_LIT (FIELD_EXPR (NAME_REF Shapes) . Circle) { (FIELD_INIT r : (LITERAL 1)) })");
    assert_eq!(rhs("Self { x: 0 }"), "(STRUCT_LIT (SELF_EXPR Self) { (FIELD_INIT x : (LITERAL 0)) })");
    assert_eq!(
        tree("raise E { reason: 1 }"),
        "(EXPR_STMT (COMMAND_CALL (NAME_REF raise) (COMMAND_ARGS (STRUCT_LIT (NAME_REF E) { (FIELD_INIT reason : (LITERAL 1)) }))))"
    );
    assert_eq!(rhs("Foo { type: 1 }"), "(STRUCT_LIT (NAME_REF Foo) { (FIELD_INIT type : (LITERAL 1)) })");
    one_error("x = shapes.Circle { r: 1 }", "`{` cannot follow this expression");
    one_error("x = f(x) {a: 1}", "`{` cannot follow this expression");
    one_error("x = Foo { x }", "a struct literal has no shorthand");
}

#[test]
fn table_keys() {
    assert_eq!(
        rhs("{type: 1, end: 2, 1: 3, \"k\": 4}"),
        "(TABLE_EXPR { (TABLE_PAIR (TABLE_KEY type) : (LITERAL 1)) , (TABLE_PAIR (TABLE_KEY end) : (LITERAL 2)) , (TABLE_PAIR (TABLE_KEY 1) : (LITERAL 3)) , (TABLE_PAIR (TABLE_KEY \"k\") : (LITERAL 4)) })"
    );
    // Lists in brackets, with spreads; no positional item and no
    // computed key in braces.
    assert_eq!(rhs("[1, *a, [2]]"), "(LIST_EXPR [ (LITERAL 1) , (SPREAD_ARG * (NAME_REF a)) , (LIST_EXPR [ (LITERAL 2) ]) ])");
    assert_eq!(rhs("[]"), "(LIST_EXPR [ ])");
    one_error("x = {a: 1, 2}", "a list is written with brackets: `[2]`");
    one_error("x = {[k]: 5}", "a computed key is set with an assignment, `t[k] = …`");
    one_error("x = f [1, 2]", "to pass a list, write `f([1, 2])`");
    one_error("x = {-1: a}", "a table key cannot be negative");
    // `true`, `false` and `nil` are not keys in a table or a keyed
    // pattern; one error per item, the value still read.
    one_error(
        "x = {true: 1}",
        "`true` is not a key name here. For the string key write `{\"true\": 1}`; a boolean key is set with an assignment, `t[true] = 1`",
    );
    one_error("x = {a: 1, false: 2, b: 3}", "a boolean key is set with an assignment, `t[false] = 2`");
    one_error("x = {nil: 1}", "`nil` is not a key name here, and `nil` cannot be a key. For the string key write `{\"nil\": 1}`");
    assert_eq!(errors("x = {true: f(), nil: 2, k: 3}").len(), 2);
    assert!(statements(&checked("x = {true: f()}")).contains("(TABLE_PAIR (ERROR_NODE true) : (CALL_EXPR"));
    one_error("let {true: x} = t", "write the key as a string, `{\"true\": x}`");
    one_error("let {a, nil: {b}} = t", "write the key as a string, `{\"nil\": {b}}`");
    // Where a word can only be a field name, the three words are names.
    for src in [
        "x = Flags { true: 1, nil: 2 }",
        "let Flags {true: t, false: f} = v",
        "let x: {true: Number, nil: String} = y",
        "struct S\n  true: Bool\n  nil = 1\nend",
        "interface I\n  false: Bool\nend",
        "x = y.nil + y.true + @false",
        "x = {\"true\": 1, \"nil\": 2}",
    ] {
        assert!(errors(src).is_empty(), "{src}: {:?}", errors(src));
    }
    one_error("x = {someKey: 1}", "written as a string");
    one_error("x = {\"a#{b}\": 1}", "a string key cannot be interpolated");
    assert!(errors("x = [1, 2]").is_empty());
}

// ---- Patterns ----------------------------------------------------------------

fn pat(p: &str) -> String {
    let t = tree(&format!("let {p} = v"));
    let prefix = "(LET_DECL let ";
    let suffix = " = (EXPR_LIST (NAME_REF v)))";
    assert!(t.starts_with(prefix) && t.ends_with(suffix), "{t}");
    t[prefix.len()..t.len() - suffix.len()].to_string()
}

#[test]
fn patterns() {
    assert_eq!(pat("{name, hp}"), "(BRACE_PAT { (PAT_FIELD (NAME name)) , (PAT_FIELD (NAME hp)) })");
    assert_eq!(pat("[head, *tail]"), "(BRACKET_PAT [ (BIND_PAT (NAME head)) , (REST_PAT * (NAME tail)) ])");
    assert_eq!(pat("[a, *]"), "(BRACKET_PAT [ (BIND_PAT (NAME a)) , (REST_PAT *) ])");
    // Case rule after `key:`.
    assert_eq!(pat("{a: value}"), "(BRACE_PAT { (PAT_FIELD a : (BIND_PAT (NAME value))) })");
    assert_eq!(pat("{a: Number}"), "(BRACE_PAT { (PAT_FIELD a : (NAMED_TYPE (QUAL_NAME Number))) })");
    assert_eq!(
        pat("{a: Number | String}"),
        "(BRACE_PAT { (PAT_FIELD a : (UNION_TYPE (NAMED_TYPE (QUAL_NAME Number)) | (NAMED_TYPE (QUAL_NAME String)))) })"
    );
    assert_eq!(pat("{p: Geo.Point?}"), "(BRACE_PAT { (PAT_FIELD p : (OPTIONAL_TYPE (NAMED_TYPE (QUAL_NAME Geo . Point)) ?)) })");
    assert_eq!(pat("{b: Lib.Box<T>}"), "(BRACE_PAT { (PAT_FIELD b : (NAMED_TYPE (QUAL_NAME Lib . Box) (TYPE_ARGS < (NAMED_TYPE (QUAL_NAME T)) >))) })");
    assert_eq!(pat("{d: Direction.North}"), "(BRACE_PAT { (PAT_FIELD d : (QUALIFIED_PAT (QUAL_NAME Direction . North))) })");
    assert_eq!(pat("{n: nil, a: any}"), "(BRACE_PAT { (PAT_FIELD n : (NAMED_TYPE nil)) , (PAT_FIELD a : (NAMED_TYPE any)) })");
    assert_eq!(pat("{s: Circle {r}}"), "(BRACE_PAT { (PAT_FIELD s : (STRUCT_PAT (QUAL_NAME Circle) { (PAT_FIELD (NAME r)) })) })");
    assert_eq!(pat("{key: name: T}"), "(BRACE_PAT { (PAT_FIELD key : (TYPED_PAT (BIND_PAT (NAME name)) : (NAMED_TYPE (QUAL_NAME T)))) })");
    assert_eq!(pat("{a: \"admin\"}"), "(BRACE_PAT { (PAT_FIELD a : (LITERAL_PAT \"admin\")) })");
    assert_eq!(pat("{pos: {x, y}}"), "(BRACE_PAT { (PAT_FIELD pos : (BRACE_PAT { (PAT_FIELD (NAME x)) , (PAT_FIELD (NAME y)) })) })");
    assert_eq!(pat("{type: t}"), "(BRACE_PAT { (PAT_FIELD type : (BIND_PAT (NAME t))) })");
    // Keyed versus positional.
    assert_eq!(pat("{1: a, 2: b}"), "(BRACE_PAT { (PAT_FIELD 1 : (BIND_PAT (NAME a))) , (PAT_FIELD 2 : (BIND_PAT (NAME b))) })");
    assert_eq!(errors("let {123, 456} = t").len(), 2);
    assert!(errors("let {123, 456} = t")[0].contains("written with brackets: `[123]`"));
    assert_eq!(pat("{\"someKey\": v2}"), "(BRACE_PAT { (PAT_FIELD \"someKey\" : (BIND_PAT (NAME v2))) })");
    // Suffixes: `as` then `:`.
    assert_eq!(
        pat("{a, b} as tbl: Table"),
        "(TYPED_PAT (AS_PAT (BRACE_PAT { (PAT_FIELD (NAME a)) , (PAT_FIELD (NAME b)) }) as (NAME tbl)) : (NAMED_TYPE (QUAL_NAME Table)))"
    );
    // Struct patterns and qualified names.
    assert_eq!(pat("Rect {w, h: height}"), "(STRUCT_PAT (QUAL_NAME Rect) { (PAT_FIELD (NAME w)) , (PAT_FIELD h : (BIND_PAT (NAME height))) })");
    assert_eq!(pat("Shapes.Circle {r}"), "(STRUCT_PAT (QUAL_NAME Shapes . Circle) { (PAT_FIELD (NAME r)) })");
    assert_eq!(pat("[Direction.North, x]"), "(BRACKET_PAT [ (QUALIFIED_PAT (QUAL_NAME Direction . North)) , (BIND_PAT (NAME x)) ])");
    // Literals and ranges, typed and `is`.
    assert_eq!(pat("[-1, 1..5, 0...-2, true, nil]"), "(BRACKET_PAT [ (LITERAL_PAT - 1) , (RANGE_PAT (LITERAL_PAT 1) .. (LITERAL_PAT 5)) , (RANGE_PAT (LITERAL_PAT 0) ... (LITERAL_PAT - 2)) , (LITERAL_PAT true) , (LITERAL_PAT nil) ])");
    assert_eq!(pat("[is Number | String]"), "(BRACKET_PAT [ (IS_PAT is (UNION_TYPE (NAMED_TYPE (QUAL_NAME Number)) | (NAMED_TYPE (QUAL_NAME String)))) ])");
    assert_eq!(pat("{a, b}: Foo"), "(TYPED_PAT (BRACE_PAT { (PAT_FIELD (NAME a)) , (PAT_FIELD (NAME b)) }) : (NAMED_TYPE (QUAL_NAME Foo)))");
    // `const` binds a SCREAMING name.
    assert_eq!(tree("const MAX_HP = 100"), "(CONST_DECL const (BIND_PAT (NAME MAX_HP)) = (EXPR_LIST (LITERAL 100)))");
}

#[test]
fn pattern_errors() {
    one_error("let Foo = v", "to test the type write `is Foo`");
    one_error("let [Number, x] = v", "is not a pattern");
    one_error("let {1, a} = t", "written with brackets: `[1]`");
    one_error("let {a = 1} = t", "defaults inside patterns are not part of NL yet");
    one_error("let {type} = t", "is a keyword and cannot be bound");
    one_error("let [*a, b] = t", "must be the last item");
    one_error("let {*a} = t", "is not allowed in `{…}`");
    one_error("let {a, b}", "a destructuring pattern needs a value");
    one_error("let Rect {1} = r", "keyed by field name");
    one_error("let x: Number as y = v", "write `as` before the type");
}

// ---- `when` items ------------------------------------------------------------

#[test]
fn when_items() {
    let t = tree(
        "case x\nwhen {a} as w, [b], is T\n  1\nwhen Circle {r} if r > 1\n  2\nwhen Circle, Direction.North, 3..9, lo..hi, y\n  3\nelse\n  4\nend",
    );
    assert!(t.contains("(WHEN_CLAUSE when (AS_PAT (BRACE_PAT { (PAT_FIELD (NAME a)) }) as (NAME w)) , (BRACKET_PAT [ (BIND_PAT (NAME b)) ]) , (IS_PAT is (NAMED_TYPE (QUAL_NAME T)))"), "{t}");
    assert!(t.contains("(WHEN_CLAUSE when (STRUCT_PAT (QUAL_NAME Circle) { (PAT_FIELD (NAME r)) }) (WHEN_GUARD if (BIN_EXPR (NAME_REF r) > (LITERAL 1)))"), "{t}");
    assert!(
        t.contains("(WHEN_CLAUSE when (NAME_REF Circle) , (FIELD_EXPR (NAME_REF Direction) . North) , (BIN_EXPR (LITERAL 3) .. (LITERAL 9)) , (BIN_EXPR (NAME_REF lo) .. (NAME_REF hi)) , (NAME_REF y) (BLOCK"),
        "{t}"
    );
    assert!(t.contains("(ELSE_CLAUSE else (BLOCK (EXPR_STMT (LITERAL 4))))"));
    // A `when` pattern typed after it, and a body starting with a Lua path.
    assert!(tree("case x\nwhen {tag: \"c\"}: Tagged\n  1\nend").contains("(TYPED_PAT (BRACE_PAT"));
    assert!(tree("case x\nwhen {a}\n  :print(a)\nend").contains("(BLOCK (EXPR_STMT (LUA_PATH : print (LUA_CALL"));
    one_error("case x\nwhen f y\n  1\nend", "a call without parentheses");
    one_error("case x\nend", "needs at least one `when`");
}

// ---- Types -------------------------------------------------------------------

fn ty(t: &str) -> String {
    let s = tree(&format!("let x: {t}"));
    let prefix = "(LET_DECL let (TYPED_PAT (BIND_PAT (NAME x)) : ";
    assert!(s.starts_with(prefix), "{s}");
    s[prefix.len()..s.len() - 2].to_string()
}

#[test]
fn types() {
    assert_eq!(ty("Box<Box<T>>"), "(NAMED_TYPE (QUAL_NAME Box) (TYPE_ARGS < (NAMED_TYPE (QUAL_NAME Box) (TYPE_ARGS < (NAMED_TYPE (QUAL_NAME T)) >)) >))");
    assert_eq!(
        ty("A<B<C<D>>>"),
        "(NAMED_TYPE (QUAL_NAME A) (TYPE_ARGS < (NAMED_TYPE (QUAL_NAME B) (TYPE_ARGS < (NAMED_TYPE (QUAL_NAME C) (TYPE_ARGS < (NAMED_TYPE (QUAL_NAME D)) >)) >)) >))"
    );
    assert_eq!(
        tree("let x: Box<T>= y"),
        "(LET_DECL let (TYPED_PAT (BIND_PAT (NAME x)) : (NAMED_TYPE (QUAL_NAME Box) (TYPE_ARGS < (NAMED_TYPE (QUAL_NAME T)) >))) = (EXPR_LIST (NAME_REF y)))"
    );
    assert_eq!(
        tree("let x: Box<Box<T>>= y"),
        "(LET_DECL let (TYPED_PAT (BIND_PAT (NAME x)) : (NAMED_TYPE (QUAL_NAME Box) (TYPE_ARGS < (NAMED_TYPE (QUAL_NAME Box) (TYPE_ARGS < (NAMED_TYPE (QUAL_NAME T)) >)) >))) = (EXPR_LIST (NAME_REF y)))"
    );
    assert_eq!(ty("~A | B?"), "(UNION_TYPE (NOT_TYPE ~ (NAMED_TYPE (QUAL_NAME A))) | (OPTIONAL_TYPE (NAMED_TYPE (QUAL_NAME B)) ?))");
    assert_eq!(ty("~A?"), "(NOT_TYPE ~ (OPTIONAL_TYPE (NAMED_TYPE (QUAL_NAME A)) ?))");
    assert_eq!(ty("(Number | String)?"), "(OPTIONAL_TYPE (PAREN_TYPE ( (UNION_TYPE (NAMED_TYPE (QUAL_NAME Number)) | (NAMED_TYPE (QUAL_NAME String))) )) ?)");
    assert_eq!(ty("{[String]: [Number]}"), "(MAP_TYPE { [ (NAMED_TYPE (QUAL_NAME String)) ] : (ARRAY_TYPE [ (NAMED_TYPE (QUAL_NAME Number)) ]) })");
    assert_eq!(ty("{type: String, \"k\": T}"), "(RECORD_TYPE { (RECORD_FIELD type : (NAMED_TYPE (QUAL_NAME String))) , (RECORD_FIELD \"k\" : (NAMED_TYPE (QUAL_NAME T))) })");
    assert_eq!(
        ty("Function(a: Number, String, *r: T): nil"),
        "(FN_TYPE Function (FN_TYPE_PARAMS ( (FN_TYPE_PARAM (NAME a) : (NAMED_TYPE (QUAL_NAME Number))) , (FN_TYPE_PARAM (NAMED_TYPE (QUAL_NAME String))) , (FN_TYPE_PARAM * (NAME r) : (NAMED_TYPE (QUAL_NAME T))) )) (RETURN_TYPE : (NAMED_TYPE nil)))"
    );
    assert_eq!(ty("Function"), "(NAMED_TYPE (QUAL_NAME Function))");
    assert_eq!(ty("Self"), "(NAMED_TYPE Self)");
    assert_eq!(ty("Inventory.Filters.Item"), "(NAMED_TYPE (QUAL_NAME Inventory . Filters . Item))");
    // `Function (` with a space is not a signature.
    assert!(tree("fn f: Function (x) end").contains("(RETURN_TYPE : (NAMED_TYPE (QUAL_NAME Function))) (BLOCK (EXPR_STMT (PAREN_EXPR"));
    one_error("let x: number", "is not a type");
    one_error("let x: {}", "a record type needs at least one field");
    one_error("let x: Box<T", "expected `>` to close the type arguments");
}

// ---- spawn, import, on, try, if let, for, cond -------------------------------

#[test]
fn spawn_forms() {
    assert_eq!(rhs("spawn f(a).await"), format!("(FIELD_EXPR (SPAWN_EXPR spawn (CALL_EXPR (NAME_REF f) (ARG_LIST ( {A} )))) . await)"));
    assert_eq!(rhs("spawn f a, b"), format!("(SPAWN_EXPR spawn (COMMAND_CALL (NAME_REF f) (COMMAND_ARGS {A} , {B})))"));
    assert_eq!(rhs("spawn shape.area()"), "(SPAWN_EXPR spawn (CALL_EXPR (FIELD_EXPR (NAME_REF shape) . area) (ARG_LIST ( ))))");
    assert_eq!(rhs("spawn @step(a)"), format!("(SPAWN_EXPR spawn (CALL_EXPR (AT_EXPR @step) (ARG_LIST ( {A} ))))"));
    assert_eq!(rhs("spawn do\n  a\nend.await"), format!("(FIELD_EXPR (SPAWN_EXPR spawn do (BLOCK (EXPR_STMT {A})) end) . await)"));
    one_error("spawn f", "`spawn` needs an argument list: `spawn f()`");
    one_error("x = spawn f.await", "`spawn f.await()`");
    one_error("spawn f(x) do\nend", "a block cannot be sent to a worker");
    one_error("spawn f x do\nend", "a block cannot be sent to a worker");
    one_error("spawn Foo(x)", "must end in a lowercase function name");
    one_error("spawn self(x)", "must end in a lowercase function name");
    one_error("spawn :lua.f(x)", "Worker.run");
    one_error("spawn do |x|\nend", "takes no parameters");
    one_error("spawn async do\nend", "takes no `async`");
    one_error("x = 1 + spawn f a", "only allowed as a whole statement");
}

#[test]
fn import_forms() {
    assert_eq!(tree("import foo from 'lib'"), "(IMPORT_DECL import (NAME foo) from 'lib')");
    assert_eq!(tree("import { from, Mode, } from 'lib'"), "(IMPORT_DECL import (IMPORT_LIST { (NAME from) , (NAME Mode) , }) from 'lib')");
    assert_eq!(tree("import from 'lib' as Lib"), "(IMPORT_DECL import from 'lib' as (NAME Lib))");
    // `from` and `step` get keyword kinds in the tree; elsewhere they are names.
    let p = checked("import from 'lib' as Lib\nlet from = step");
    let kinds: Vec<_> = p.syntax().descendants_with_tokens().filter_map(|e| e.into_token()).map(|t| t.kind()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == SyntaxKind::FROM_KW).count(), 1);
    assert!(tree("for i in 1..5 step 2\nend").contains("(FOR_STEP step (LITERAL 2))"));
    one_error("import { a as b } from 'lib'", "cannot be renamed");
    one_error("import a from \"#{p}\"", "plain string");
    one_error("fn f\n  import a from 'x'\nend", "only allowed at the top of a file");
    // Imports are the first statements of the file; reported once.
    one_error("let x = 1\nimport a from 'x'\nimport b from 'y'", "must come before every other statement");
    one_error("pub fn f end\nimport a from 'x'", "must come before every other statement");
    assert!(errors("# comment\n\nimport a from 'x'\nimport { b } from 'y'\nlet x = 1").is_empty());
    one_error("module M\n  pub fn f end\nend", "only allowed at the top level");
    one_error("pub let x = 1", "`pub` cannot be used on `let`");
    one_error("pub fn Foo.bar end", "cannot be used on a method");
    one_error("import x 'lib'", "expected `from`");
}

#[test]
fn on_forms() {
    assert_eq!(
        tree("on PlayerHit as 'armor' async do |t, a: Number|: nil\n  t\nend"),
        "(EXPR_STMT (ON_EXPR on (QUAL_NAME PlayerHit) as 'armor' async do (BLOCK_PARAMS | (PARAM (BIND_PAT (NAME t))) , (PARAM (TYPED_PAT (BIND_PAT (NAME a)) : (NAMED_TYPE (QUAL_NAME Number)))) |) (RETURN_TYPE : (NAMED_TYPE nil)) (BLOCK (EXPR_STMT (NAME_REF t))) end))"
    );
    assert_eq!(tree("on Game.Tick do end"), "(EXPR_STMT (ON_EXPR on (QUAL_NAME Game . Tick) do (BLOCK) end))");
    assert!(tree("let h = on Other as 'mine' do\nend").starts_with("(LET_DECL let (BIND_PAT (NAME h)) = (EXPR_LIST (ON_EXPR"));
    one_error("on X as \"#{n}\" do\nend", "plain string");
    one_error("on X\n  y\nend", "expected `do` on the line of `on`");
}

#[test]
fn try_forms() {
    assert_eq!(
        tree("try\n  a\ncatch e: NotFound | Timeout\n  b\ncatch e\n  c\ncatch\n  d\nfinally\n  e\nend"),
        "(EXPR_STMT (TRY_EXPR try (BLOCK (EXPR_STMT (NAME_REF a))) (CATCH_CLAUSE catch (NAME e) : (UNION_TYPE (NAMED_TYPE (QUAL_NAME NotFound)) | (NAMED_TYPE (QUAL_NAME Timeout))) (BLOCK (EXPR_STMT (NAME_REF b)))) (CATCH_CLAUSE catch (NAME e) (BLOCK (EXPR_STMT (NAME_REF c)))) (CATCH_CLAUSE catch (BLOCK (EXPR_STMT (NAME_REF d)))) (FINALLY_CLAUSE finally (BLOCK (EXPR_STMT (NAME_REF e)))) end))"
    );
    assert_eq!(tree("try a finally b end"), format!("(EXPR_STMT (TRY_EXPR try (BLOCK (EXPR_STMT {A})) (FINALLY_CLAUSE finally (BLOCK (EXPR_STMT {B}))) end))"));
    one_error("try\n  a\nend", "needs at least one `catch` or a `finally`");
    one_error("try\n  a\ncatch NotFound\n  b\nend", "write `catch err: NotFound`");
    one_error("try\n  a\nfinally\n  b\ncatch\n  c\nend", "`catch` cannot follow `finally`");
}

#[test]
fn if_let_and_conditionals() {
    assert_eq!(
        tree("if let {role: \"admin\", name} = user\n  a\nelseif let x = y\n  b\nelse\n  c\nend"),
        "(EXPR_STMT (IF_EXPR if (LET_CONDITION let (BRACE_PAT { (PAT_FIELD role : (LITERAL_PAT \"admin\")) , (PAT_FIELD (NAME name)) }) = (NAME_REF user)) (BLOCK (EXPR_STMT (NAME_REF a))) (ELSEIF_CLAUSE elseif (LET_CONDITION let (BIND_PAT (NAME x)) = (NAME_REF y)) (BLOCK (EXPR_STMT (NAME_REF b)))) (ELSE_CLAUSE else (BLOCK (EXPR_STMT (NAME_REF c)))) end))"
    );
    assert!(tree("if let p = find name\nend").contains("(LET_CONDITION let (BIND_PAT (NAME p)) = (COMMAND_CALL"));
    assert_eq!(tree("unless a\n  b\nelse\n  c\nend"), format!("(EXPR_STMT (UNLESS_EXPR unless {A} (BLOCK (EXPR_STMT {B})) (ELSE_CLAUSE else (BLOCK (EXPR_STMT {C}))) end))"));
    assert_eq!(tree("cond\nwhen a\n  b\nend"), format!("(EXPR_STMT (COND_EXPR cond (COND_CLAUSE when {A} (BLOCK (EXPR_STMT {B}))) end))"));
    assert_eq!(tree("cond; when a; b; end").lines().count(), 1);
    one_error("unless let x = y\nend", "`unless let` is not allowed");
    one_error("unless a\n  b\nelseif c\n  d\nend", "`unless` takes no `elseif`");
    one_error("if x = 1\nend", "to compare, write `==`");
    one_error("if\n  x\nend", "must be on one line");
    one_error("cond\nwhen a, b\n  c\nend", "takes one condition");
}

#[test]
fn for_headers() {
    assert_eq!(tree("for k, v in map\nend"), "(FOR_STMT for (BIND_PAT (NAME k)) , (BIND_PAT (NAME v)) in (NAME_REF map) (BLOCK) end)");
    assert!(tree("for {name, hp} in units\nend").contains("(BRACE_PAT"));
    assert!(tree("for i: Number in xs\nend").contains("(TYPED_PAT"));
    assert!(tree("for i in (1..10) step 2\nend").contains("(FOR_STEP"));
    one_error("for i in r step 2\nend", "`step` needs a range written in the header");
    one_error("for i = 1, 10\nend", "write `for i in 1..10`");
    one_error("for i = 1, 10 do\n  print(i)\nend", "write `for i in 1..10` (a range includes its end), without `do`");
}

// ---- Declarations ------------------------------------------------------------

#[test]
fn declarations() {
    assert_eq!(tree("fn Foo.[]=(k, v)\nend"), "(FN_DECL fn (QUAL_NAME Foo) . (OP_NAME [ ] =) (PARAM_LIST ( (PARAM (BIND_PAT (NAME k))) , (PARAM (BIND_PAT (NAME v))) )) (BLOCK) end)");
    assert_eq!(tree("fn Foo.()(*args)\nend"), "(FN_DECL fn (QUAL_NAME Foo) . (OP_NAME ( )) (PARAM_LIST ( (REST_PARAM * (NAME args)) )) (BLOCK) end)");
    assert_eq!(tree("fn foo(a, callback: Function, *args)\nend").matches("REST_PARAM").count(), 1);
    // NL has no `...`; each use is reported with the `*args` form.
    one_error("fn f(...)\nend", "NL has no varargs parameter `...`: write `*args`");
    one_error("fn f(*args)\n  g(...)\nend", "NL has no varargs: `...` is the range operator");
    one_error("fn f(*args)\n  g ...\nend", "NL has no varargs; use a `*args` parameter");
    one_error("x = ...", "NL has no varargs");
    one_error("let f: Function(Number, ...): nil", "written `*args: Type`");
    // `...` is the exclusive range, at the level of `..`; `..<` is reported.
    assert_eq!(tree("x = 1...10"), "(ASSIGN_STMT (NAME_REF x) = (BIN_EXPR (LITERAL 1) ... (LITERAL 10)))");
    assert_eq!(tree("x = a\n  ...b"), format!("(ASSIGN_STMT (NAME_REF x) = (BIN_EXPR {A} ... {B}))"));
    assert!(tree("for i in 0...n step 2\nend").contains("(FOR_STEP step (LITERAL 2))"));
    one_error("x = 1...2...3", "ranges cannot be chained");
    one_error("for i in 1..<10\nend", "NL has no `..<`: a range that leaves out its end is written `a...b`");
    one_error("let 1..<5 = q", "NL has no `..<`");
    one_error("fn f(*args, b)\nend", "`*rest` must be the last parameter");
    assert_eq!(tree("fn Foo.type end"), "(FN_DECL fn (QUAL_NAME Foo) . (NAME type) (BLOCK) end)");
    assert_eq!(tree("fn Inventory.Filters.add end"), "(FN_DECL fn (QUAL_NAME Inventory . Filters) . (NAME add) (BLOCK) end)");
    for op in ["+", "-", "*", "/", "%", "**", "//", "<>", "|", "&", "^", "<<", ">>", ">>>", "==", "<", "<=", ">", ">=", "~"] {
        let t = tree(&format!("fn V.{op}(o)\nend"));
        assert!(t.contains(&format!("(OP_NAME {op}) (PARAM_LIST")), "{op}: {t}");
    }
    assert_eq!(tree("fn foo ({key: value})\nend"), "(FN_DECL fn (NAME foo) (PARAM_LIST ( (PARAM (BRACE_PAT { (PAT_FIELD key : (BIND_PAT (NAME value))) })) )) (BLOCK) end)");
    assert_eq!(tree("pub async fn foo end"), "(FN_DECL pub async fn (NAME foo) (BLOCK) end)");
    assert_eq!(tree("fn double(x) x * 2 end"), "(FN_DECL fn (NAME double) (PARAM_LIST ( (PARAM (BIND_PAT (NAME x))) )) (BLOCK (EXPR_STMT (BIN_EXPR (NAME_REF x) * (LITERAL 2)))) end)");
    // `fn` then anything but a name is an anonymous function.
    assert_eq!(tree("fn() a end"), format!("(EXPR_STMT (FN_EXPR fn (PARAM_LIST ( )) (BLOCK (EXPR_STMT {A})) end))"));
    assert_eq!(rhs("fn print(a) end"), format!("(FN_EXPR fn (BLOCK (EXPR_STMT (CALL_EXPR (NAME_REF print) (ARG_LIST ( {A} ))))) end)"));
    assert_eq!(rhs("async fn(a) a end"), format!("(FN_EXPR async fn (PARAM_LIST ( (PARAM (BIND_PAT (NAME a))) )) (BLOCK (EXPR_STMT {A})) end)"));
    assert_eq!(
        tree("struct Monster : Entity\n  id: Number\n  type: String = \"x\"\n  count = 0\n  bare\nend"),
        "(STRUCT_DECL struct (NAME Monster) (STRUCT_PARENT : (NAMED_TYPE (QUAL_NAME Entity))) (FIELD_DECL (NAME id) : (NAMED_TYPE (QUAL_NAME Number))) (FIELD_DECL (NAME type) : (NAMED_TYPE (QUAL_NAME String)) = (LITERAL \"x\")) (FIELD_DECL (NAME count) = (LITERAL 0)) (FIELD_DECL (NAME bare)) end)"
    );
    assert_eq!(tree("pub struct Foo end"), "(STRUCT_DECL pub struct (NAME Foo) end)");
    assert!(tree("struct Box<T>\n  value: T\nend").contains("(TYPE_PARAMS < (TYPE_PARAM (NAME T)) >)"));
    assert_eq!(
        tree("interface Shape\n  fn area: Number\n  name: String\n  fn: Function\nend"),
        "(INTERFACE_DECL interface (NAME Shape) (IFACE_METHOD fn (NAME area) (RETURN_TYPE : (NAMED_TYPE (QUAL_NAME Number)))) (IFACE_FIELD (NAME name) : (NAMED_TYPE (QUAL_NAME String))) (IFACE_FIELD (NAME fn) : (NAMED_TYPE (QUAL_NAME Function))) end)"
    );
    assert!(tree("interface From<T>\n  fn from(v: T): Self\nend").starts_with("(INTERFACE_DECL interface (NAME From) (TYPE_PARAMS"));
    assert!(tree("interface To<String> for Celsius\n  fn to: String\n    \"x\"\n  end\nend").starts_with("(INTERFACE_IMPL interface (NAMED_TYPE (QUAL_NAME To) (TYPE_ARGS"));
    assert_eq!(
        tree("enum Team\n  Red = 1\n  Blue = -2\n  Green = 'g'\nend"),
        "(ENUM_DECL enum (NAME Team) (VARIANT (NAME Red) = (LITERAL 1)) (VARIANT (NAME Blue) = (PREFIX_EXPR - (LITERAL 2))) (VARIANT (NAME Green) = (LITERAL 'g')) end)"
    );
    assert_eq!(tree("module Foo.Bar\n  let x = 1\nend"), "(MODULE_DECL module (QUAL_NAME Foo . Bar) (BLOCK (LET_DECL let (BIND_PAT (NAME x)) = (EXPR_LIST (LITERAL 1)))) end)");
    assert_eq!(tree("module Foo end"), "(MODULE_DECL module (QUAL_NAME Foo) end)");
    assert_eq!(tree("global Score: Number = 0"), "(GLOBAL_DECL global (NAME Score) : (NAMED_TYPE (QUAL_NAME Number)) = (LITERAL 0))");
    assert_eq!(tree("global C"), "(GLOBAL_DECL global (NAME C))");
    assert_eq!(tree("type Pair<A, B> = {first: A}"), "(TYPE_DECL type (NAME Pair) (TYPE_PARAMS < (TYPE_PARAM (NAME A)) , (TYPE_PARAM (NAME B)) >) = (RECORD_TYPE { (RECORD_FIELD first : (NAMED_TYPE (QUAL_NAME A))) }))");
}

#[test]
fn declaration_errors() {
    one_error("struct Foo\n  fn bar\n  end\nend", "a struct body holds fields only");
    one_error("struct Foo\n  a: Number, b: Number\nend", "one per line");
    one_error("struct foo\nend", "a struct name is capitalized: write `Foo`");
    one_error("global score = 1", "a global's name is capitalized");
    one_error("fn Foo.!=(o)\nend", "`!=` cannot be defined");
    one_error("fn Foo.+<T>(o)\nend", "an operator method takes no type parameters");
    one_error("fn f<T: Shape>(x)\nend", "generic constraints");
    one_error("enum E<T>\n  A\nend", "an enum takes no type parameters");
    one_error("const x", "a `const` needs a value");
    one_error("pub const {a} = t", "`pub const` takes names only");
    one_error("interface I\n  fn f\n    x\n  end\nend", "has no body");
    one_error("interface I for T\n  a: Number\nend", "holds method definitions only");
    one_error("fn f(*a, b)\nend", "must be the last parameter");
    one_error("f(*a, b)", "must be the last positional argument");
}

// ---- Targeted errors ---------------------------------------------------------

#[test]
fn targeted_errors() {
    // `then` is an ordinary name and `do` an ordinary keyword; after a
    // header they are unexpected, and the error carries a hint.
    const HINT: &str = "Hint: do not write `then` or `do` after a header";
    one_error("if ready then\n  go()\nend", "unexpected identifier `then`: the header of `if`");
    one_error("if ready then\n  go()\nend", HINT);
    one_error("elseif_test = 1\nif a\nelseif b then\nend", "unexpected identifier `then`: the header of `elseif`");
    one_error("case x\nwhen 1 then foo()\nend", "unexpected identifier `then`: the header of `when`");
    one_error("unless done then\nend", "unexpected identifier `then`");
    one_error("while x do\n  y()\nend", "unexpected `do`: the header of `while`");
    one_error("while x do\n  y()\nend", HINT);
    one_error("for x in xs do\nend", "unexpected `do`");
    one_error("if a do\nend", "unexpected `do`");
    one_error("while a do; b; end", "unexpected `do`");
    assert!(errors("if check(then)\nend").is_empty(), "`then` in parentheses is a name");
    one_error("while cond\n  x()\nend", "`cond` is a keyword");
    one_error("let x = cond", "`cond` is a keyword");
    one_error("if cond\nend", "`cond` is a keyword");
    one_error("x ~= y", "use `!=`");
    assert!(errors("x = a ?? b\nx = a?.b\nx ??= 1").is_empty(), "the nil-safe operators parse");
    one_error("a?.b, c = 1, 2", "a nil-safe target needs an assignment of its own");
    one_error("c, a?.[1] = 1, 2", "a nil-safe target needs an assignment of its own");
    one_error("c, :a?.b = 1, 2", "a nil-safe target needs an assignment of its own");
    one_error("x = o:m?.(1)", "`x:m?.(…)` is not part of NL");
    one_error("x = :o:m?.(1)", "`x:m?.(…)` is not part of NL");
    one_error("x = a ? .b", "written `?.`, without a space inside");
    one_error("let t = spawn none?.m(f())", "a `spawn` callee cannot be nil-safe");
    one_error("x = ?? b", "expected an expression");
    one_error("x = a ? b", "unexpected `?`");
    one_error("while let x = f()\nend", "`while let` is not part of NL yet");
    one_error("if x =~ re\nend", "there is no match operator");
    one_error("local x = 1", "NL declares variables with `let`");
    one_error("local function f()\nend", "NL declares functions with `fn`");
    one_error("function f()\nend", "NL declares functions with `fn`");
    one_error("let getHealth = 1", "is not an NL name");
    one_error("x = obj.getHealth", "Lua path");
    one_error("x = {getHealth: 1}", "written as a string");
    one_error("x = @getHealth", "field names are snake_case (`@get_health`)");
    assert!(errors("x = @type").is_empty());
    // Where Lua-style names are allowed.
    assert!(errors(":player.getHealth()").is_empty());
    assert!(errors("conn:sendMessage(1)").is_empty());
    assert!(errors("x = t[\"someKey\"]").is_empty());
}

#[test]
fn accepted_forms_rejected_later() {
    // These parse without error; later stages reject them.
    for src in [
        "x.len = v",
        "x.len += v",
        "f(x.len!)",
        "x.len()",
        "x.len a",
        "x.len do end",
        "@len",
        "struct S\n  len: Number\nend",
        "fn f\n  true\n  1\nend",
        "fn f(a = 1, b)\nend",
        "fn f(a: Number, b)\nend",
        "Foo = 1",
        "MAX_HP = 2",
    ] {
        assert!(errors(src).is_empty(), "{src:?}: {:?}", errors(src));
    }
}

// ---- Recovery ----------------------------------------------------------------

#[test]
fn several_independent_errors() {
    let src = "let a = (1 + )\nfn f()\n  x = = 2\nend\nprint getHealth\nlet b = 3\nwhile cond\n  y()\nend\n";
    let errs = errors(src);
    assert_eq!(errs.len(), 4, "{errs:#?}");
    // The declarations after the errors are intact.
    let t = statements(&checked(src));
    assert!(t.contains("(FN_DECL fn (NAME f)"));
    assert!(t.contains("(LET_DECL let (BIND_PAT (NAME b))"));
    assert!(t.contains("(WHILE_STMT while"));
}

#[test]
fn missing_end_is_found_by_indentation() {
    let src = "fn a()\n  if x\n    foo()\nend\n\nfn b()\n  bar()\nend\n";
    let errs = errors(src);
    assert_eq!(errs.len(), 1, "{errs:#?}");
    assert!(errs[0].contains("the `if` on line 2 has no `end`"), "{}", errs[0]);
    let p = checked(src);
    let fns = p.syntax().children().flat_map(|b| b.children()).filter(|n| n.kind() == SyntaxKind::FN_DECL).count();
    assert_eq!(fns, 2, "`fn b` stays at the top level");

    let src = "fn a()\n  foo()\n\nfn b()\n  bar()\nend\n";
    let errs = errors(src);
    assert_eq!(errs.len(), 1, "{errs:#?}");
    assert!(errs[0].contains("the `fn` on line 1 has no `end`"));
    let p = checked(src);
    assert_eq!(p.diagnostics()[0].range.start(), 16.into(), "reported where `fn b` starts");

    let src = "struct Foo\n  a: Number\n\nfn f()\n  x\nend\n";
    assert_eq!(errors(src).len(), 1);

    // Two missing `end`s, each reported where it is missing.
    let src = "fn a()\n  x\nfn b()\n  if y\n    z\nfn c()\nend\n";
    let errs = errors(src);
    assert_eq!(errs.len(), 2, "{errs:#?}");
    assert!(errs[0].contains("the `fn` on line 1 has no `end`"), "{}", errs[0]);
    assert!(errs[1].contains("on line 3 has no `end`") || errs[1].contains("on line 4 has no `end`"), "{}", errs[1]);

    // Without a better place, at the end of the file.
    one_error("fn a()\n  x", "the file ends first");
}

#[test]
fn stray_closers() {
    one_error("end", "there is no open block");
    one_error("x = 1\nelse\ny = 2", "there is no `if` here");
    one_error("when 1", "belongs inside `case");
    one_error(")", "no open bracket");
    one_error("if a\n  b\nelse\n  c\nelse\n  d\nend", "`else` comes last");
}

#[test]
fn unclosed_brackets() {
    one_error("print(a\nlet y = 2", "expected `)` to close this `(`");
    one_error("foo(1, 2\nbar()", "expected `)` to close this `(`");
    one_error("x = {\n  a: 1\n  b: 2\n}", "expected `,` or `}`");
    one_error("x = (1 + \nlet y = 2", "expected an expression");
}

// ---- Doc comments and the tree -----------------------------------------------

#[test]
fn doc_comments() {
    let p = checked("## Adds two numbers.\n## Second line.\nfn add(a, b)\nend\n\n## Not attached.\n\nfn g end\n# plain\n## attached\nlet x = 1\nstruct S\n  ## The id.\n  id: Number\nend\n## handler\non E do\nend\nx = 1 ## trailing\nfn h end\n");
    let docs: Vec<(SyntaxKind, Option<String>)> =
        p.syntax().descendants().filter(|n| n.kind() != SyntaxKind::NAME).filter_map(|n| match n.kind() {
            SyntaxKind::FN_DECL | SyntaxKind::LET_DECL | SyntaxKind::FIELD_DECL | SyntaxKind::ON_EXPR => {
                Some((n.kind(), doc_comment(&n)))
            }
            _ => None,
        }).collect();
    assert_eq!(
        docs,
        vec![
            (SyntaxKind::FN_DECL, Some("Adds two numbers.\nSecond line.".to_string())),
            (SyntaxKind::FN_DECL, None),
            (SyntaxKind::LET_DECL, Some("attached".to_string())),
            (SyntaxKind::FIELD_DECL, Some("The id.".to_string())),
            (SyntaxKind::ON_EXPR, Some("handler".to_string())),
            (SyntaxKind::FN_DECL, None),
        ]
    );
    // The `#` lines after a `##` line belong to the doc comment; a `#`
    // line before the first `##` does not, and a blank line detaches all.
    let doc_of = |src: &str| {
        let p = checked(src);
        p.syntax().descendants().find(|n| n.kind() == SyntaxKind::FN_DECL).and_then(|n| doc_comment(&n))
    };
    assert_eq!(doc_of("## aaa\n# bbb\n# ccc\nfn foo; end"), Some("aaa\nbbb\nccc".to_string()));
    assert_eq!(doc_of("# plain\n## aaa\n#bbb\n## ccc\n# ddd\nfn foo; end"), Some("aaa\nbbb\nccc\nddd".to_string()));
    assert_eq!(doc_of("x = 1\n  ## aaa\n  # bbb\nfn foo; end"), Some("aaa\nbbb".to_string()));
    assert_eq!(doc_of("## aaa\n# bbb\n\nfn foo; end"), None);
    assert_eq!(doc_of("## aaa\n\n# bbb\nfn foo; end"), None);
    assert_eq!(doc_of("x = 1 ## aaa\n# bbb\nfn foo; end"), None);
    assert_eq!(doc_of("# aaa\n# bbb\nfn foo; end"), None);
}

#[test]
fn trivia_and_error_tokens_are_in_the_tree() {
    for src in ["x = 1 $ 2\n", "# only a comment", "", "\u{FEFF}x", "  \n\n", "x = \"abc\ny = 1", "f(\"#{}\")"] {
        let p = checked(src);
        assert_eq!(p.syntax().text().to_string(), src);
    }
    // A lexer error is not reported twice.
    let p = checked("x = 1 $\n");
    assert_eq!(p.diagnostics().len(), 1);
    let p = checked("f(\"#{}\")");
    assert_eq!(p.diagnostics().len(), 1, "{:?}", p.diagnostics());
}

#[test]
fn diagnostics_are_ordered() {
    let p = checked("x = $\nlet getX = 1\ny = )\n");
    let starts: Vec<u32> = p.diagnostics().iter().map(|d| d.range.start().into()).collect();
    let mut sorted = starts.clone();
    sorted.sort_unstable();
    assert_eq!(starts, sorted);
    assert!(starts.len() >= 3);
}

#[test]
fn dump_format() {
    let dump = nl_syntax::dump_parse("let x = 1\n");
    assert_eq!(
        dump,
        "SOURCE_FILE@0..10\n  BLOCK@0..10\n    LET_DECL@0..9\n      LET_KW@0..3 \"let\"\n      WHITESPACE@3..4 \" \"\n      BIND_PAT@4..5\n        NAME@4..5\n          LNAME@4..5 \"x\"\n      WHITESPACE@5..6 \" \"\n      EQ@6..7 \"=\"\n      WHITESPACE@7..8 \" \"\n      EXPR_LIST@8..9\n        LITERAL@8..9\n          NUMBER@8..9 \"1\"\n    NEWLINE@9..10 \"\\n\"\n  EOF@10..10 \"\"\n"
    );
    let dump = nl_syntax::dump_parse("if a then\nend");
    assert!(dump.ends_with("---\nerror@5..9 1:6: unexpected identifier `then`: the header of `if` ends at the end of its line. Hint: do not write `then` or `do` after a header; put the body on the next line\n"), "{dump}");
}
