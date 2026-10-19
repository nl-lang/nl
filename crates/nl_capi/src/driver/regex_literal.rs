//! Compile-time validation of literal patterns: when the argument of
//! `Regex.new` is a string literal, the pattern is parsed with
//! `regex-syntax`, the parser of the engine that will compile it at run
//! time, and a bad pattern is an error diagnostic.
//!
//! A small pass of its own, run after the resolver so that `Regex` is known
//! to be the prelude's `Regex` and not a local of that name, and before
//! lowering. It looks at `Regex.new(lit)` and `Regex.new lit` alone: a
//! pattern built at run time (interpolation, a variable) is checked by
//! `Regex.new` itself, which raises `RegexError`. The size cap of the
//! compiled pattern is not checked here, since it needs the compiler of
//! the engine and depends on the VM's limit.

use nl_sema::util::{child, expr_child, field_name, name_text, token};
use nl_sema::{Ref, Resolution};
use nl_syntax::SyntaxKind::{ARG_LIST, CALL_EXPR, COMMAND_ARGS, COMMAND_CALL, FIELD_EXPR, LITERAL, NAME_REF, STRING};
use nl_syntax::{SyntaxNode, TextRange};

/// The diagnostic code of a bad literal pattern.
pub(crate) const CODE: &str = "invalid-regex";

/// Every literal pattern passed to `Regex.new` that the regex engine would
/// reject: the range of the literal and the message.
pub(crate) fn check(root: &SyntaxNode, res: &Resolution) -> Vec<(TextRange, String)> {
    let mut out = Vec::new();
    for node in root.descendants().filter(|n| matches!(n.kind(), CALL_EXPR | COMMAND_CALL)) {
        let Some(callee) = expr_child(&node) else { continue };
        if !is_prelude_regex_new(&callee, res) {
            continue;
        }
        let Some(list) = child(&node, ARG_LIST).or_else(|| child(&node, COMMAND_ARGS)) else { continue };
        let Some(arg) = list.children().next() else { continue };
        if arg.kind() != LITERAL {
            continue;
        }
        let Some(tok) = token(&arg, STRING) else { continue };
        let bytes = nl_syntax::decode_string(&[tok.text()]).into_iter().next().unwrap_or_default();
        let message = match std::str::from_utf8(&bytes) {
            Err(_) => "invalid regex pattern: the pattern is not valid UTF-8".to_owned(),
            Ok(pattern) => match regex_syntax::Parser::new().parse(pattern) {
                Ok(_) => continue,
                Err(e) => format!("invalid regex pattern: {}", describe(&e)),
            },
        };
        out.push((arg.text_range(), message));
    }
    out
}

/// `Regex.new` where `Regex` is the prelude's.
fn is_prelude_regex_new(callee: &SyntaxNode, res: &Resolution) -> bool {
    if callee.kind() != FIELD_EXPR || !field_name(callee).is_some_and(|t| t.text() == "new") {
        return false;
    }
    let Some(recv) = expr_child(callee) else { return false };
    if recv.kind() != NAME_REF || name_text(&recv).as_deref() != Some("Regex") {
        return false;
    }
    matches!(res.reference(recv.text_range()), Some(Ref::Prelude(p)) if p.name == "Regex")
}

/// The reason, on one line: the engine's full message quotes the pattern
/// over several lines, which a diagnostic already points at.
fn describe(e: &regex_syntax::Error) -> String {
    match e {
        regex_syntax::Error::Parse(e) => e.kind().to_string(),
        regex_syntax::Error::Translate(e) => e.kind().to_string(),
        _ => e.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn errors(src: &str) -> Vec<(TextRange, String)> {
        let parse = nl_syntax::parse(src);
        assert!(!parse.has_errors(), "{:?}", parse.diagnostics());
        let root = parse.syntax();
        let res = nl_sema::resolve(&root, &nl_sema::NoImports);
        check(&root, &res)
    }

    #[test]
    fn bad_literal_is_reported_at_the_literal() {
        let src = "let re = Regex.new('(')\n";
        let errs = errors(src);
        assert_eq!(errs.len(), 1);
        let (range, msg) = &errs[0];
        assert_eq!(&src[usize::from(range.start())..usize::from(range.end())], "'('");
        assert!(msg.contains("unclosed group"), "{msg}");
        // The command form, and an unsupported feature.
        let errs = errors("let re = Regex.new '(?<=a)b'\n");
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].1.contains("look-around"), "{}", errs[0].1);
        let errs = errors("Regex.new(\"\\xff\")\n");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].1.contains("UTF-8"));
    }

    #[test]
    fn good_and_unchecked_patterns_pass() {
        assert!(errors("let re = Regex.new('^(?<year>\\d{4})-(\\d{2})$')\n").is_empty());
        assert!(errors("let re = Regex.new \"a#{1}(\"\n").is_empty(), "interpolation is not checked");
        assert!(errors("let p = '('\nlet re = Regex.new(p)\n").is_empty());
        assert!(errors("struct Regex\n  p: String\nend\nfn Regex.new(p) p end\nlet re = Regex.new('(')\n").is_empty(), "a struct Regex is not the prelude's");
        assert!(errors("let re = Regex.new()\n").is_empty());
    }
}
