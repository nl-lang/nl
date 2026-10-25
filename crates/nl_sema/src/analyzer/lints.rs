//! Lints that need the whole file: unused bindings, a Lua path that
//! names the function it is in, escapes in raw strings, and the top-level
//! statements a worker would repeat. The lints that are
//! found along the walk (a global read, a redundant `!`, a function-kind
//! name passed without `!`, shadowing in the resolver) live where they
//! are found.

use nl_syntax::{SyntaxKind, SyntaxNode, TextRange};

use super::Analyzer;
use crate::binding::*;
use crate::util::*;

use SyntaxKind::*;

impl Analyzer<'_> {
    pub(crate) fn lints(&mut self, root: &SyntaxNode) {
        self.unused_bindings(root);
        self.lua_paths_to_own_function(root);
        self.raw_escapes(root);
    }

    /// A single-quoted string is raw: only `\'` and `\\` are escapes in it,
    /// so `'\n'` is a backslash and an `n`, which trips everyone who comes
    /// from Lua, where both quotes escape. A raw literal holding what is an
    /// escape in a `"…"` string (`\n`, `\t`, `\r`, `\0`, `\xHH`, `\u{…}`,
    /// `\"`) is reported once, at its first such sequence. A raw string is
    /// also how a regex is written, and the engine reads these escapes
    /// itself, so the pattern of `Regex.new` is left alone, and so is a
    /// literal with a backslash before anything else (`\d`, `\.`, a
    /// Windows path), which is plainly meant as written. `'\\n'` spells the
    /// same backslash and `n` without the lint.
    fn raw_escapes(&mut self, root: &SyntaxNode) {
        for tok in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
            if tok.kind() != STRING || nl_syntax::string_form(tok.text()) != Some(nl_syntax::StringForm::Raw) {
                continue;
            }
            let Some(at) = raw_escape_at(tok.text()) else { continue };
            if tok.parent().is_some_and(|lit| self.is_regex_pattern(&lit)) {
                continue;
            }
            let text = tok.text();
            let seq = text.get(at..).map_or("", |r| if r.starts_with("\\u{") { r.get(..3).unwrap_or(r) } else { r.get(..2).unwrap_or(r) });
            let start = tok.text_range().start() + nl_syntax::TextSize::from(u32::try_from(at).unwrap_or(0));
            let range = TextRange::at(start, nl_syntax::TextSize::from(u32::try_from(seq.len()).unwrap_or(0)));
            let rest = seq.get(1..).unwrap_or("");
            self.lint(
                range,
                "raw-escape",
                format!("single-quoted strings are raw: `{seq}` here is a backslash and `{rest}`, not an escape; for the escape use a double-quoted string (`\"{seq}\"`), or write `\\\\{rest}` if the backslash is meant"),
            );
        }
    }

    /// `lit` is the first argument of the prelude's `Regex.new`.
    fn is_regex_pattern(&self, lit: &SyntaxNode) -> bool {
        let Some(list) = lit.parent().filter(|l| matches!(l.kind(), ARG_LIST | COMMAND_ARGS)) else { return false };
        if list.children().next().as_ref() != Some(lit) {
            return false;
        }
        let Some(callee) = list.parent().and_then(|c| expr_child(&c)) else { return false };
        if callee.kind() != FIELD_EXPR || !field_name(&callee).is_some_and(|t| t.text() == "new") {
            return false;
        }
        expr_child(&callee).is_some_and(|r| {
            r.kind() == NAME_REF && name_text(&r).as_deref() == Some("Regex") && matches!(self.res.reference(r.text_range()), Some(Ref::Prelude(p)) if p.name == "Regex")
        })
    }

    /// A Lua path whose first name is the function it is written in:
    /// `pub fn f(x) :f(x) end`. The first name binds the NL function, not
    /// the Lua global of that name, so a wrapper of a host function of the
    /// same name calls itself forever. Calls are reported, in any position
    /// and in closures inside the function, and so are the other steps,
    /// which read a field of a function; a bare `:f` whose value is used
    /// is the function itself, as `f!` is, and is left alone.
    fn lua_paths_to_own_function(&mut self, root: &SyntaxNode) {
        for path in root.descendants().filter(|n| n.kind() == LUA_PATH) {
            let Some(first) = crate::resolver::lua_path_root(&path) else { continue };
            let Some(Ref::Binding(id)) = self.res.reference(first.text_range()) else { continue };
            let id = *id;
            let has_step = path.children().any(|c| matches!(c.kind(), LUA_FIELD | LUA_INDEX | LUA_CALL | LUA_METHOD));
            // A statement whose value is discarded calls the path.
            let statement = path.parent().is_some_and(|p| p.kind() == EXPR_STMT && self.res.value_discarded(p.text_range()));
            if !has_step && !statement {
                continue;
            }
            if !path.ancestors().skip(1).any(|a| self.is_function_of(&a, id)) {
                continue;
            }
            let name = first.text().to_string();
            let range = TextRange::new(path.text_range().start(), first.text_range().end());
            self.lint(range, "lua-path-own-function", format!("`:{name}` is this function itself, not the Lua global `{name}`: the first name of a Lua path is an NL name when one is in scope; write `:_G.{name}` to reach the global"));
        }
    }

    /// `node` is the function bound to `id`: the `fn` declaration of the
    /// name, or a `fn` or short function assigned to it (`f = fn … end`).
    fn is_function_of(&self, node: &SyntaxNode, id: BindingId) -> bool {
        match node.kind() {
            FN_DECL => child(node, NAME).is_some_and(|n| self.res.declaration(n.text_range()) == Some(id)),
            FN_EXPR | LAMBDA_EXPR => node.parent().filter(|p| p.kind() == ASSIGN_STMT).and_then(|p| expr_child(&p)).is_some_and(|target| {
                target != *node && target.kind() == NAME_REF && matches!(self.res.reference(target.text_range()), Some(Ref::Binding(t)) if *t == id)
            }),
            _ => false,
        }
    }

    /// A `let`, `const`, loop or pattern variable that nothing reads.
    /// Parameters are the shape of a function and a `catch` variable is
    /// often there for its type alone: neither is reported; a name
    /// starting with `_` is meant to be unused.
    fn unused_bindings(&mut self, root: &SyntaxNode) {
        let exported: std::collections::HashSet<BindingId> = self.res.export_bindings().map(|(id, _)| id).collect();
        let mut referenced: std::collections::HashSet<BindingId> = self.used.clone();
        for n in root.descendants() {
            match n.kind() {
                NAME_REF | SELF_EXPR | AT_EXPR => {
                    if let Some(Ref::Binding(id)) = self.res.reference(n.text_range()) {
                        referenced.insert(*id);
                    }
                }
                LUA_PATH => {
                    if let Some(tok) = crate::resolver::lua_path_root(&n) {
                        if let Some(Ref::Binding(id)) = self.res.reference(tok.text_range()) {
                            referenced.insert(*id);
                        }
                    }
                }
                QUAL_NAME => {
                    if let Some(Ref::Binding(id)) = self.res.type_ref(n.text_range()) {
                        referenced.insert(*id);
                    }
                }
                _ => {}
            }
        }
        for (i, b) in self.res.bindings.iter().enumerate() {
            let id = BindingId(u32::try_from(i).unwrap_or(u32::MAX));
            if referenced.contains(&id) || exported.contains(&id) || b.name.starts_with('_') {
                continue;
            }
            let what = match b.decl {
                DeclKind::Let => "variable",
                DeclKind::Const => "constant",
                DeclKind::LoopVar => "loop variable",
                DeclKind::PatternVar => "pattern variable",
                _ => continue,
            };
            // A module `const` is a member read as `M.name`.
            if b.decl == DeclKind::Const && self.res.module_const_of(id) {
                continue;
            }
            let name = b.name.clone();
            let range = b.range;
            self.lint(range, "unused-binding", format!("the {what} `{name}` is never used; prefix it with `_` if that is intended"));
        }
    }

    /// Finds the top-level statements a worker would repeat when it loads
    /// this file: expression statements, assignments and
    /// control flow at file level, except handlers and a statement
    /// guarded by `unless Worker.parent`.
    pub(crate) fn find_top_level_effects(&mut self, root: &SyntaxNode) {
        let Some(b) = block(root) else { return };
        for s in statements(&b) {
            let effect = match s.kind() {
                EXPR_STMT => {
                    let guarded = modifier(&s).is_some_and(|m| first_token(&m).is_some_and(|t| t.kind() == UNLESS_KW) && expr_child(&m).is_some_and(|e| is_worker_parent(&e)));
                    let handler = expr_child(&s).is_some_and(|e| e.kind() == ON_EXPR);
                    !guarded && !handler
                }
                ASSIGN_STMT | COMPOUND_ASSIGN_STMT | MULTI_ASSIGN_STMT | WHILE_STMT | UNTIL_STMT | LOOP_STMT | FOR_STMT => true,
                _ => false,
            };
            if effect {
                self.top_level_effects = true;
                return;
            }
        }
    }
}

/// `Worker.parent`.
fn is_worker_parent(e: &SyntaxNode) -> bool {
    e.kind() == FIELD_EXPR
        && field_name(e).is_some_and(|t| t.text() == "parent")
        && expr_child(e).is_some_and(|r| r.kind() == NAME_REF && name_text(&r).as_deref() == Some("Worker"))
}

/// The byte offset in a raw literal's text of the first backslash sequence
/// that would be an escape in a `"…"` string, when every backslash of the
/// literal starts either such a sequence or one of the raw escapes `\\`
/// and `\'` (see `raw_escapes`).
fn raw_escape_at(text: &str) -> Option<usize> {
    let b = text.as_bytes();
    let body_end = b.len().saturating_sub(1);
    let mut found = None;
    let mut i = 1;
    while i < body_end {
        if b.get(i) != Some(&b'\\') {
            i += 1;
            continue;
        }
        match b.get(i + 1).copied() {
            Some(b'\\' | b'\'') => {}
            Some(b'n' | b't' | b'r' | b'0' | b'"') => {
                found = found.or(Some(i));
            }
            Some(b'x') if b.get(i + 2).is_some_and(u8::is_ascii_hexdigit) && b.get(i + 3).is_some_and(u8::is_ascii_hexdigit) => {
                found = found.or(Some(i));
            }
            Some(b'u') if b.get(i + 2) == Some(&b'{') => {
                found = found.or(Some(i));
            }
            // A backslash meant as written: a regex class, an escaped
            // punctuation character, a path.
            _ => return None,
        }
        i += 2;
    }
    found
}
