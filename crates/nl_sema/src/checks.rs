//! Structural checks on the finished tree that need no names:
//! parameter lists and discarded values.

use nl_syntax::{SyntaxKind, SyntaxNode};

use crate::resolver::Resolver;
use crate::util::*;

use SyntaxKind::*;

/// Parameters with defaults come after those without, and if
/// any parameter is annotated, all must be.
pub(crate) fn check_param_list(r: &mut Resolver<'_>, list: &SyntaxNode) {
    let mut seen_default = false;
    let mut annotated = Vec::new();
    for p in list.children() {
        match p.kind() {
            PARAM => {
                let has_default = expr_child(&p).is_some();
                if seen_default && !has_default {
                    r.error(trimmed_range(&p), "default-before-required", "parameters with defaults come after those without");
                }
                seen_default |= has_default;
                let typed = pattern_children(&p).next().is_some_and(|pat| pat.kind() == TYPED_PAT);
                annotated.push((typed, trimmed_range(&p)));
            }
            REST_PARAM | KEYED_PARAM => {
                // An unannotated `*args` or `**opts` beside annotated
                // parameters falls under the all-or-none rule.
                annotated.push((type_child(&p).is_some(), trimmed_range(&p)));
            }
            _ => {}
        }
    }
    let any = annotated.iter().any(|(t, _)| *t);
    if any {
        if let Some((_, range)) = annotated.iter().find(|(t, _)| !*t) {
            r.error(*range, "partial-annotations", "if any parameter is annotated, all must be");
        }
    }
}

/// An expression statement whose value is
/// discarded must be a call or an assignment. Reported on the value
/// (the line of the value, not of the enclosing construct).
pub(crate) fn check_discarded(r: &mut Resolver<'_>, root: &SyntaxNode) {
    for b in root.children().filter(|c| c.kind() == BLOCK) {
        check_block(r, &b, true);
    }
}

/// `tail_used`: the value of the block's last statement is used (the
/// tail of a function body, of a branch of a block expression whose value
/// is used).
///
/// This is also where it is decided, for the whole compiler, whether an
/// expression statement's value is discarded ([`Resolver::mark_discarded`];
/// the lowering calls a bare name or member there). The rule
/// is taken literally: discarded means "a statement that is
/// not last" (or one with a loop modifier, `x while c`; the last
/// statement `x if c` is the tail `x` run only when `c` holds, as
/// `if c x end` would be). That leaves open
/// the last statement of a loop body, of a function declared `: nil`,
/// of an `on` handler, of a `do` block or `finally` block, and of the file:
/// those are last, so their value counts as used (a variable is read, a
/// function-kind name or a method is called) and then dropped, and they
/// are not checked as discarded either.
fn check_block(r: &mut Resolver<'_>, block: &SyntaxNode, tail_used: bool) {
    let stmts: Vec<SyntaxNode> = statements(block).collect();
    let n = stmts.len();
    for (i, s) in stmts.iter().enumerate() {
        let last = i + 1 == n;
        if s.kind() == EXPR_STMT {
            let loop_modifier = modifier(s).is_some_and(|m| first_token(&m).is_some_and(|t| matches!(t.kind(), WHILE_KW | UNTIL_KW)));
            let discarded = !(last && tail_used) || loop_modifier;
            if let Some(list) = child(s, EXPR_LIST) {
                // `a, b` as a statement returns several values from a
                // function; anywhere else it is not a call or
                // an assignment.
                if discarded {
                    r.error(trimmed_range(&list), "value-list-not-tail", "a list of values `a, b` is a statement only as the last statement of a function, where it returns them all; here its values would be discarded");
                }
                visit(r, &list, true);
                if let Some(m) = modifier(s) {
                    visit(r, &m, true);
                }
                continue;
            }
            let Some(e) = expr_child(s) else { continue };
            if discarded {
                r.mark_discarded(s.text_range());
                check_discarded_expr(r, &e);
            } else {
                visit(r, &e, true);
            }
            if let Some(m) = modifier(s) {
                visit(r, &m, true);
            }
        } else {
            visit(r, s, true);
        }
    }
}

fn check_discarded_expr(r: &mut Resolver<'_>, e: &SyntaxNode) {
    match e.kind() {
        // Calls, and what counts as a call in statement position.
        CALL_EXPR | COMMAND_CALL | METHOD_CALL | NAME_REF | FIELD_EXPR | AT_EXPR | LUA_PATH | PIPE_EXPR | SPAWN_EXPR
        | ON_EXPR => visit(r, e, true),
        PAREN_EXPR => {
            if let Some(inner) = expr_child(e) {
                check_discarded_expr(r, &inner);
            }
        }
        // A discarded block construct: the tail of each branch is
        // discarded too.
        IF_EXPR | UNLESS_EXPR | CASE_EXPR | COND_EXPR | TRY_EXPR => visit_construct(r, e, false),
        _ => {
            r.error(
                trimmed_range(e),
                "discarded-value",
                "an expression statement that is not last must be a call or an assignment; this value would be discarded",
            );
            visit(r, e, true);
        }
    }
}

/// Visits a block construct, checking its branch blocks with `tail_used`.
fn visit_construct(r: &mut Resolver<'_>, e: &SyntaxNode, tail_used: bool) {
    for c in e.children() {
        match c.kind() {
            BLOCK => check_block(r, &c, tail_used),
            ELSEIF_CLAUSE | ELSE_CLAUSE | WHEN_CLAUSE | COND_CLAUSE | CATCH_CLAUSE => {
                for cc in c.children() {
                    if cc.kind() == BLOCK {
                        check_block(r, &cc, tail_used);
                    } else {
                        visit(r, &cc, true);
                    }
                }
            }
            // A `finally` block's value is never used.
            FINALLY_CLAUSE => {
                for cc in c.children() {
                    if cc.kind() == BLOCK {
                        check_block(r, &cc, true);
                    }
                }
            }
            _ => visit(r, &c, true),
        }
    }
}

/// Visits any node, finding the blocks inside it.
fn visit(r: &mut Resolver<'_>, node: &SyntaxNode, tail_used: bool) {
    match node.kind() {
        IF_EXPR | UNLESS_EXPR | CASE_EXPR | COND_EXPR | TRY_EXPR => visit_construct(r, node, tail_used),
        BLOCK => check_block(r, node, true),
        _ => {
            for c in node.children() {
                visit(r, &c, true);
            }
        }
    }
}
