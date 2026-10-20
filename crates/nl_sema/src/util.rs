//! Small helpers for reading the `rowan` tree of `nl_syntax`.
//!
//! The resolver and the lowering both read the syntax tree directly, without
//! a typed AST layer. These helpers keep that reading short and total: they
//! return `Option` or empty iterators where a part is missing, never panic.

use nl_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextRange};

/// The first child node of `kind`.
pub fn child(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|c| c.kind() == kind)
}

/// Every child node of `kind`.
pub fn children(node: &SyntaxNode, kind: SyntaxKind) -> impl Iterator<Item = SyntaxNode> + '_ {
    node.children().filter(move |c| c.kind() == kind)
}

/// The first child token of `kind`.
pub fn token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    node.children_with_tokens().filter_map(|e| e.into_token()).find(|t| t.kind() == kind)
}

/// True when `node` has a child token of `kind`.
pub fn has_token(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    token(node, kind).is_some()
}

/// True for an item the file exports: `pub`, or `global` on an `fn`,
/// `module` or `struct` (which also stores it in the Lua global of its
/// name). `global X` alone declares a Lua global and exports nothing.
pub fn is_exported(node: &SyntaxNode) -> bool {
    has_token(node, SyntaxKind::PUB_KW) || is_global_item(node)
}

/// True for `global fn`, `global module` and `global struct`: an exported
/// item that the file also stores in the Lua global of its name.
pub fn is_global_item(node: &SyntaxNode) -> bool {
    matches!(node.kind(), SyntaxKind::FN_DECL | SyntaxKind::MODULE_DECL | SyntaxKind::STRUCT_DECL) && has_token(node, SyntaxKind::GLOBAL_KW)
}

/// The non-trivia child tokens of `node`, in order.
pub fn tokens(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> + '_ {
    node.children_with_tokens().filter_map(|e| e.into_token()).filter(|t| !t.kind().is_trivia())
}

/// The first non-trivia child token, whatever its kind.
pub fn first_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    tokens(node).next()
}

/// The first child token that is a word: a name or a keyword used as a name.
pub fn name_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    tokens(node).find(|t| {
        let k = t.kind();
        k.is_name() || k.is_keyword()
    })
}

/// The text of the name in a `NAME`, `NAME_REF` or similar node.
pub fn name_text(node: &SyntaxNode) -> Option<String> {
    name_token(node).map(|t| t.text().to_string())
}

/// The expression children of a node: every child node that is an
/// expression kind (see [`is_expr`]).
pub fn expr_children(node: &SyntaxNode) -> impl Iterator<Item = SyntaxNode> + '_ {
    node.children().filter(|c| is_expr(c.kind()))
}

/// The first expression child.
pub fn expr_child(node: &SyntaxNode) -> Option<SyntaxNode> {
    expr_children(node).next()
}

/// The statements of a `BLOCK`.
pub fn statements(block: &SyntaxNode) -> impl Iterator<Item = SyntaxNode> + '_ {
    block.children().filter(|c| c.kind() != SyntaxKind::ERROR_NODE)
}

/// The type child of a node (annotation, return type, …).
pub fn type_child(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.children().find(|c| is_type(c.kind()))
}

/// The segments of a `QUAL_NAME`: its `CNAME` (or `Self`) tokens.
pub fn qual_segments(node: &SyntaxNode) -> Vec<SyntaxToken> {
    tokens(node)
        .filter(|t| matches!(t.kind(), SyntaxKind::CNAME | SyntaxKind::SELF_TYPE_KW | SyntaxKind::LNAME))
        .collect()
}

/// The text of a `QUAL_NAME` (`Foo.Bar`).
pub fn qual_text(node: &SyntaxNode) -> String {
    qual_segments(node).iter().map(|t| t.text().to_string()).collect::<Vec<_>>().join(".")
}

/// The range of a node without leading or trailing trivia.
pub fn trimmed_range(node: &SyntaxNode) -> TextRange {
    let mut first = None;
    let mut last = None;
    for t in node.descendants_with_tokens().filter_map(|e| e.into_token()) {
        if t.kind().is_trivia() {
            continue;
        }
        if first.is_none() {
            first = Some(t.text_range());
        }
        last = Some(t.text_range());
    }
    match (first, last) {
        (Some(a), Some(b)) => TextRange::new(a.start(), b.end()),
        _ => node.text_range(),
    }
}

/// Expression node kinds, including the block expressions.
pub fn is_expr(kind: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(
        kind,
        LITERAL
            | INTERP_STRING
            | SELF_EXPR
            | AT_EXPR
            | PAREN_EXPR
            | TABLE_EXPR
            | LIST_EXPR
            | STRUCT_LIT
            | FN_EXPR
            | LAMBDA_EXPR
            | SPAWN_EXPR
            | ON_EXPR
            | LUA_PATH
            | PREFIX_EXPR
            | BIN_EXPR
            | IS_EXPR
            | AS_EXPR
            | PIPE_EXPR
            | CALL_EXPR
            | COMMAND_CALL
            | INDEX_EXPR
            | FIELD_EXPR
            | METHOD_CALL
            | BANG_EXPR
            | NAME_REF
            | IF_EXPR
            | UNLESS_EXPR
            | CASE_EXPR
            | COND_EXPR
            | TRY_EXPR
    )
}

/// Type node kinds.
pub fn is_type(kind: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(
        kind,
        NAMED_TYPE | FN_TYPE | UNION_TYPE | NOT_TYPE | OPTIONAL_TYPE | PAREN_TYPE | TUPLE_TYPE | RECORD_TYPE | MAP_TYPE | ARRAY_TYPE
    )
}

/// Pattern node kinds.
pub fn is_pattern(kind: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(
        kind,
        BIND_PAT
            | LITERAL_PAT
            | RANGE_PAT
            | IS_PAT
            | BRACE_PAT
            | BRACKET_PAT
            | STRUCT_PAT
            | QUALIFIED_PAT
            | AS_PAT
            | TYPED_PAT
    )
}

/// The patterns that are direct children of a node.
pub fn pattern_children(node: &SyntaxNode) -> impl Iterator<Item = SyntaxNode> + '_ {
    node.children().filter(|c| is_pattern(c.kind()))
}

/// The binary operator token of a `BIN_EXPR`.
pub fn bin_op(node: &SyntaxNode) -> Option<SyntaxToken> {
    tokens(node).find(|t| {
        let k = t.kind();
        !k.is_name() && k != SyntaxKind::NUMBER && !k.is_string_part() && k != SyntaxKind::ERROR
    })
}

/// The `BLOCK` child of a node.
pub fn block(node: &SyntaxNode) -> Option<SyntaxNode> {
    child(node, SyntaxKind::BLOCK)
}

/// The member name of a `FIELD_EXPR` (the word after the `.` or `?.`).
pub fn field_name(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut after_dot = false;
    for t in tokens(node) {
        if after_dot && (t.kind().is_name() || t.kind().is_keyword()) {
            return Some(t);
        }
        if matches!(t.kind(), SyntaxKind::DOT | SyntaxKind::QUESTION_DOT) {
            after_dot = true;
        }
    }
    None
}

// ---- Nil-safe chains ----------------------------------------------

/// True for a nil-safe step: a `FIELD_EXPR`, `INDEX_EXPR`, `CALL_EXPR`,
/// `METHOD_CALL` or Lua path step that holds its own `?.` token.
pub fn is_safe_step(node: &SyntaxNode) -> bool {
    has_token(node, SyntaxKind::QUESTION_DOT)
}

/// The kinds whose first expression child is the operand of a postfix step:
/// the receiver of a member, index or method call, the callee of a call or
/// command call, the function of `f!`.
fn is_chain_kind(kind: SyntaxKind) -> bool {
    use SyntaxKind::*;
    matches!(kind, FIELD_EXPR | INDEX_EXPR | CALL_EXPR | COMMAND_CALL | METHOD_CALL | BANG_EXPR)
}

/// The operand a postfix step applies to (see [`is_chain_kind`]).
pub fn chain_receiver(node: &SyntaxNode) -> Option<SyntaxNode> {
    if is_chain_kind(node.kind()) { expr_child(node) } else { None }
}

/// True when the parent of `node` is a postfix step applied to it, so that
/// the chain goes on above `node`. Parentheses end a chain.
pub fn continues_chain(node: &SyntaxNode) -> bool {
    node.parent().is_some_and(|p| chain_receiver(&p).as_ref() == Some(node))
}

/// True when `node` is the operand of a nil-safe step: the value that step
/// tests for `nil`.
pub fn is_safe_receiver(node: &SyntaxNode) -> bool {
    node.parent().is_some_and(|p| is_safe_step(&p) && chain_receiver(&p).as_ref() == Some(node))
}

/// One nil-safe step of a chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SafeStep {
    /// A postfix step (`step`) and the operand it tests (`receiver`).
    Postfix { step: SyntaxNode, receiver: SyntaxNode },
    /// A step of a Lua path, which tests the value of the path before it.
    Lua { path: SyntaxNode, step: SyntaxNode },
}

/// The nil-safe steps of the chain that ends at `top`, innermost first: its
/// postfix steps down to the first operand that is not one, and the steps
/// of a Lua path there. When the value tested by one of them
/// is `nil`, the whole chain is `nil`.
pub fn safe_steps(top: &SyntaxNode) -> Vec<SafeStep> {
    let mut out = Vec::new();
    let mut cur = top.clone();
    loop {
        if cur.kind() == SyntaxKind::LUA_PATH {
            let lua: Vec<SafeStep> = cur.children().filter(is_safe_step).map(|step| SafeStep::Lua { path: cur.clone(), step }).collect();
            out.extend(lua.into_iter().rev());
            break;
        }
        let Some(receiver) = chain_receiver(&cur) else { break };
        if is_safe_step(&cur) {
            out.push(SafeStep::Postfix { step: cur.clone(), receiver: receiver.clone() });
        }
        cur = receiver;
    }
    out.reverse();
    out
}

/// True when `node` ends a chain that holds a nil-safe step: the chain whose
/// value is `nil` when a tested value is.
pub fn is_safe_chain_top(node: &SyntaxNode) -> bool {
    !continues_chain(node) && has_safe_step(node)
}

/// True when the chain that ends at `node` holds a nil-safe step.
pub fn has_safe_step(node: &SyntaxNode) -> bool {
    let mut cur = node.clone();
    loop {
        if cur.kind() == SyntaxKind::LUA_PATH {
            return cur.children().any(|s| is_safe_step(&s));
        }
        let Some(receiver) = chain_receiver(&cur) else { return false };
        if is_safe_step(&cur) {
            return true;
        }
        cur = receiver;
    }
}

/// True when `node` is the target of an assignment or compound assignment
/// (the first expression of the statement), not of a multiple assignment.
pub fn is_single_target(node: &SyntaxNode) -> bool {
    node.parent()
        .is_some_and(|p| matches!(p.kind(), SyntaxKind::ASSIGN_STMT | SyntaxKind::COMPOUND_ASSIGN_STMT) && expr_child(&p).as_ref() == Some(node))
}

/// The modifier of a simple statement, if any.
pub fn modifier(node: &SyntaxNode) -> Option<SyntaxNode> {
    child(node, SyntaxKind::MODIFIER)
}

/// True for a type expression that is exactly the named type `name`
/// (`Function`, `Number`, …), without arguments.
pub fn is_named_type(ty: &SyntaxNode, name: &str) -> bool {
    if ty.kind() != SyntaxKind::NAMED_TYPE {
        return false;
    }
    match child(ty, SyntaxKind::QUAL_NAME) {
        Some(q) => qual_text(&q) == name && child(ty, SyntaxKind::TYPE_ARGS).is_none(),
        None => false,
    }
}

/// True for a type that makes a binding function-kind: `Function`
/// or a `Function(...)` signature, possibly in parentheses.
pub fn is_function_type(ty: &SyntaxNode) -> bool {
    match ty.kind() {
        SyntaxKind::FN_TYPE => true,
        SyntaxKind::NAMED_TYPE => is_named_type(ty, "Function"),
        SyntaxKind::PAREN_TYPE => type_child(ty).is_some_and(|t| is_function_type(&t)),
        _ => false,
    }
}

/// The method name of a `METHOD_CALL`: the word after its `:`.
pub fn method_name_of(node: &SyntaxNode) -> Option<String> {
    let mut after_colon = false;
    for t in tokens(node) {
        if t.kind() == SyntaxKind::COLON {
            after_colon = true;
        } else if after_colon && (t.kind().is_name() || t.kind().is_keyword()) {
            return Some(t.text().to_string());
        }
    }
    None
}
