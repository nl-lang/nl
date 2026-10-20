//! Lexer, parser and syntax tree of the NL language.
//!
//! # Tokens
//!
//! [`lex`] turns source text into a [`Lexed`]: a `Vec<`[`Token`]`>` and a
//! `Vec<`[`Diagnostic`]`>`. Each token has a [`SyntaxKind`], a byte
//! [`TextRange`] and two layout flags. The token list is lossless: trivia
//! (whitespace, line breaks, comments, doc comments) and `ERROR` tokens are
//! included, and the token texts concatenate to exactly the source, for any
//! input. The last token is always an empty `EOF`. The lexer never panics and
//! never stops early; every error becomes a diagnostic plus tokens that still
//! cover the text.
//!
//! The parser reads the non-trivia tokens ([`Lexed::significant`] gives
//! their indices) and puts the trivia before each one into the tree.
//!
//! # Layout
//!
//! The lexer produces no newline or indentation tokens for the parser to
//! consume. Instead every non-trivia token carries:
//!
//! - [`Token::nl_before`]: a line break lies between it and the previous
//!   non-trivia token;
//! - [`Token::sp_before`]: any trivia lies between them ("touches" is
//!   `!sp_before`).
//!
//! The parser decides whether a line break is significant,
//! classifies every `:` (`COLON`) from these flags, and recognizes the
//! contextual keywords `from` and `step`, which are always `LNAME`.
//!
//! What the lexer decides from local context:
//!
//! - postfix versus prefix `!`: `BANG_POSTFIX` when the `!` touches a
//!   name token, an `AT_NAME`, a keyword directly after `.` (whatever the
//!   spacing between `.` and the keyword), `)` or `]`; `NEQ` whenever `!` is
//!   followed by `=`; `BANG` otherwise;
//! - `AT_NAME`: `@` and a word, any keyword included;
//! - name classes: `LNAME`, `CNAME`, `ONAME` (see [`name_case`]).
//!
//! The lexer never splits a token: closing type arguments on `>>` is
//! the parser's job.
//!
//! # Strings and interpolation
//!
//! A string without interpolation, of any of the six forms, is one `STRING`
//! token with its quotes. A string with interpolation is a sequence
//!
//! ```text
//! STR_BEGIN  expression tokens  { STR_MID  expression tokens }  STR_END
//! "a #{      x + 1              } b #{     f("c")             } d"
//! ```
//!
//! where `STR_BEGIN` ends with `#{`, `STR_MID` starts with `}` and ends with
//! `#{`, and `STR_END` starts with `}` and ends with the closing quote. The
//! expression tokens in between are ordinary tokens with ordinary layout
//! flags, so the parser parses them with its normal expression parser, in
//! bracket context. Strings inside interpolations nest to any depth
//! (`"a#{ "b#{c}" }d"`); `{` and `}` inside an interpolation nest too.
//!
//! The sequence is always balanced, even after errors: when an
//! interpolation is cut off (a line break in a one-line string, the end of
//! the file), the lexer reports it and emits an empty `STR_END`. Errors the
//! lexer reports about interpolation: a line break inside `#{ … }` of a
//! `"…"` string, a `"""` opened inside an interpolation, and `#{}` with
//! nothing inside. For `#{}` the parser should not report a second "expected
//! an expression" error: a `STR_BEGIN`/`STR_MID` directly followed by
//! `STR_MID`/`STR_END` has already been reported.
//!
//! # String values
//!
//! [`decode_string`] takes the texts of one literal's tokens (one `STRING`,
//! or `STR_BEGIN … STR_END` without the expression tokens) and returns the
//! byte string of each text piece: escapes decoded, heredoc line breaks
//! normalized to `\n`, the first and last line break of a heredoc dropped,
//! and the `-` dedent applied. Dedent is done there, not in the lexer: it
//! needs all pieces, and it works on the source text before escapes.
//! [`string_form`] tells the six forms apart; [`number_value`] gives the value
//! of a `NUMBER`.
//!
//! Dedent and interpolations spanning lines: a heredoc line that starts
//! inside a multi-line interpolation is code, not content, so it does not
//! take part in computing the common indentation. A content line whose only non-blank text is an
//! interpolation is not blank.
//!
//! # Parsing
//!
//! [`parse`] lexes and parses a source text into a [`Parse`]: a `rowan`
//! tree ([`SyntaxNode`], root `SOURCE_FILE`) and every diagnostic, the
//! lexer's and the parser's, ordered by position. The grammar is
//! implemented in `parser/`, one module per part of the grammar.
//!
//! The tree is lossless: every lexer token, trivia and `ERROR` tokens
//! included, is in it, and `parse(src).syntax().to_string() == src` for any
//! input. Node kinds follow the productions; [`SyntaxKind`] documents the
//! children of each. Conventions a later stage relies on:
//!
//! - trivia before a node's first token is outside the node, except that a
//!   `BLOCK` holds the trivia before its first statement and before the
//!   token that ends it;
//! - a doc comment (a `##` line and the comment lines
//!   after it) is inside the node of the declaration it documents, before
//!   its first token; [`doc_comment`] reads it;
//! - `from`, `step` and `then` are `FROM_KW`, `STEP_KW` and `THEN_KW` in the
//!   tree where they are keywords; a `>>` that closed two type argument lists is two `GT` tokens;
//! - a command call is a `COMMAND_CALL`, a call with parentheses or a
//!   trailing block a `CALL_EXPR`; a method call `x:m(…)` is one
//!   `METHOD_CALL` holding its arguments and block;
//! - a Lua path is one flat `LUA_PATH` node holding all its steps as
//!   `LUA_FIELD`, `LUA_INDEX`, `LUA_CALL` and `LUA_METHOD` nodes: a
//!   step node always has Lua semantics, and nothing outside the node does.
//!   An NL member after a parenthesized path is a `FIELD_EXPR` over the
//!   `PAREN_EXPR`;
//! - a construct the parser could not fit into the grammar is an
//!   `ERROR_NODE` (with a diagnostic); a construct with a missing part is its
//!   usual node with that part absent.
//!
//! The parser recovers at statement boundaries and at block structure and
//! reports one diagnostic per mistake. When a block lacks its `end`, the
//! indentation of the following lines decides where it was probably missing.
//! It never panics and always terminates.
//!
//! # Limits
//!
//! The parser bounds its recursion, so that it fits in the 2 MiB stack of a
//! spawned thread even in a debug build:
//!
//! - **128 levels of nesting.** Each open bracket (`(`, `[`, `{`, `#{`, the
//!   `<…>` of type arguments or parameters, the `|…|` of block parameters)
//!   and each construct closed by `end` is one level. So 128 nested `if`s,
//!   loops, functions, tables or parentheses parse, and 64 nested
//!   `f(fn() … end)`, which are two levels each. Measured in a debug build,
//!   parsing at this limit uses at most about 760 KiB of stack (nested
//!   `case`; `if`, `try`, `fn` about 730 KiB, parentheses and calls about
//!   440 KiB), well within 2 MiB.
//! - **About 1000 chained operators or postfix steps in one expression**
//!   (`a + b + …`, `x.a.b.…`, `f()()…`), together with the nesting around
//!   them: the depth of the tree, which `rowan` frees recursively. Parsing
//!   and freeing a tree of this depth takes about 460 KiB of stack in a
//!   debug build.
//! - **448 levels of recursion of the parser's functions**, a safety net
//!   that ordinary nesting does not reach before the 128 levels above.
//!
//! Past a limit, the parser reports one error ("nested too deeply" or "too
//! long") and skips the rest of the construct it was in, as one balanced
//! group of tokens (brackets and block keywords matched with their closers),
//! into an `ERROR_NODE`; it then goes on with the enclosing construct. At
//! most one such error is reported per statement of the file's top level.
//!
//! # Printing
//!
//! [`dump_tree`] prints a tree one element per line (`KIND@start..end`,
//! tokens with their text); [`dump_diagnostics`] prints diagnostics with line
//! and column; [`dump_parse`] does both for a source text, for snapshot
//! tests. `nlc parse` uses the same format.
//!
//! # Formatting
//!
//! [`format`] prints a tree again with normalized spacing and indentation,
//! keeping every line break, and reports the `"…"` strings continued with a
//! backslash; with [`FormatOptions::fix`] it rewrites them
//! as heredocs. [`format_decl`] does the same for a declaration file.
//! `nlc fmt` is built on them; the module docs of `format` state the rules.
//!
//! # Invalid UTF-8
//!
//! [`lex`] takes `&str`. A driver that reads bytes calls
//! [`source_from_bytes`] first, which returns a diagnostic pointing at the
//! first invalid byte; such a file is not lexed.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing))]

mod diagnostic;
mod format;
mod kind;
mod lexer;
mod line_index;
mod literal;
mod names;
mod parser;
mod printer;
mod syntax;

#[cfg(test)]
mod tests;

pub use diagnostic::{Diagnostic, Severity};
pub use format::{Continued, FormatOptions, Formatted, format, format_decl};
pub use kind::{NlLanguage, SyntaxKind, keyword_kind};
pub use lexer::{Lexed, Token, lex, source_from_bytes};
pub use literal::{StringForm, decode_string, number_value, string_form};
pub use line_index::{LineCol, LineIndex};
pub use names::{NameCase, name_case};
pub use printer::{DumpOptions, dump_diagnostics, dump_parse, dump_parse_decl, dump_tree};
pub use syntax::{Parse, SyntaxElement, SyntaxNode, SyntaxToken, doc_comment, parse, parse_decl_file};
pub use rowan::{TextRange, TextSize};
