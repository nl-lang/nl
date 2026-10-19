//! Unit tests of the lexer.

use crate::SyntaxKind::{self, *};
use crate::{Lexed, decode_string, lex};

/// Checks the invariants every lex result must satisfy, and returns it.
pub(crate) fn checked(src: &str) -> Lexed<'_> {
    let lexed = lex(src);
    let mut joined = String::new();
    let mut pos = 0u32;
    let mut stack = 0usize;
    for (i, t) in lexed.tokens.iter().enumerate() {
        assert_eq!(u32::from(t.range.start()), pos, "gap or overlap before token {i} in {src:?}");
        pos = t.range.end().into();
        joined.push_str(t.text(src));
        assert_eq!(t.kind == EOF, i + 1 == lexed.tokens.len(), "EOF must be last in {src:?}");
        assert!(t.kind.is_token());
        if t.range.is_empty() {
            assert!(matches!(t.kind, EOF | STR_END), "empty {:?} in {src:?}", t.kind);
        }
        if t.kind.is_trivia() {
            assert!(!t.nl_before && !t.sp_before);
        }
        if t.nl_before {
            assert!(t.sp_before);
        }
        match t.kind {
            STR_BEGIN => stack += 1,
            STR_MID => assert!(stack > 0, "STR_MID outside a string in {src:?}"),
            STR_END => {
                assert!(stack > 0, "unbalanced STR_END in {src:?}");
                stack -= 1;
            }
            _ => {}
        }
    }
    assert_eq!(stack, 0, "unclosed STR_BEGIN in {src:?}");
    assert_eq!(joined, src);
    let last = lexed.tokens.last().unwrap();
    assert!(last.nl_before && last.sp_before);
    for d in &lexed.diagnostics {
        let r = std::ops::Range::<usize>::from(d.range);
        assert!(src.get(r.clone()).is_some(), "diagnostic range {r:?} off char boundary in {src:?}");
        assert!(!d.message.is_empty());
    }
    // Every string literal decodes without panicking, one piece per part.
    let mut parts: Vec<Vec<&str>> = Vec::new();
    for (i, t) in lexed.tokens.iter().enumerate() {
        match t.kind {
            STRING => assert_eq!(decode_string(&[lexed.text(i)]).len(), 1),
            STR_BEGIN => parts.push(vec![lexed.text(i)]),
            STR_MID => parts.last_mut().unwrap().push(lexed.text(i)),
            STR_END => {
                let mut p = parts.pop().unwrap();
                p.push(lexed.text(i));
                assert_eq!(decode_string(&p).len(), p.len());
            }
            _ => {}
        }
    }
    lexed
}

/// The non-trivia tokens before EOF, as (kind, text).
fn toks(src: &str) -> Vec<(SyntaxKind, &str)> {
    let lexed = checked(src);
    lexed
        .significant()
        .filter(|&i| lexed.kind(i) != EOF)
        .map(|i| (lexed.kind(i), lexed.text(i)))
        .collect()
}

fn kinds(src: &str) -> Vec<SyntaxKind> {
    toks(src).into_iter().map(|(k, _)| k).collect()
}

/// The diagnostics' messages.
fn errors(src: &str) -> Vec<String> {
    checked(src).diagnostics.into_iter().map(|d| d.message).collect()
}

fn clean(src: &str) -> Vec<(SyntaxKind, &str)> {
    let errs = errors(src);
    assert!(errs.is_empty(), "unexpected diagnostics for {src:?}: {errs:?}");
    toks(src)
}

/// (kind, nl_before, sp_before) of the non-trivia tokens, EOF included.
fn layout(src: &str) -> Vec<(SyntaxKind, bool, bool)> {
    let lexed = checked(src);
    lexed
        .significant()
        .map(|i| {
            let t = lexed.tokens[i];
            (t.kind, t.nl_before, t.sp_before)
        })
        .collect()
}

fn decoded(src: &str) -> Vec<String> {
    let lexed = checked(src);
    let parts: Vec<&str> =
        lexed.significant().filter(|&i| lexed.kind(i).is_string_part()).map(|i| lexed.text(i)).collect();
    decode_string(&parts).iter().map(|p| String::from_utf8_lossy(p).into_owned()).collect()
}

// ---- Basic structure --------------------------------------------------------

#[test]
fn empty_file() {
    let lexed = checked("");
    assert_eq!(lexed.tokens.len(), 1);
    assert_eq!(lexed.tokens[0].kind, EOF);
    assert!(lexed.diagnostics.is_empty());
}

#[test]
fn no_final_newline() {
    assert_eq!(clean("print x"), vec![(LNAME, "print"), (LNAME, "x")]);
    assert_eq!(layout("x"), vec![(LNAME, false, false), (EOF, true, true)]);
}

#[test]
fn byte_order_mark() {
    let lexed = checked("\u{FEFF}let x");
    assert!(lexed.diagnostics.is_empty());
    assert_eq!(lexed.tokens[0].kind, WHITESPACE);
    assert_eq!(lexed.text(0), "\u{FEFF}");
    // Elsewhere it is a stray character.
    assert_eq!(errors("x\u{FEFF}").len(), 1);
}

#[test]
fn trivia_kinds() {
    let lexed = checked("a \t# c\n## doc\r\n### rule\n\rb");
    let all: Vec<_> = lexed.tokens.iter().map(|t| (t.kind, t.text(lexed.source))).collect();
    assert_eq!(
        all,
        vec![
            (LNAME, "a"),
            (WHITESPACE, " \t"),
            (COMMENT, "# c"),
            (NEWLINE, "\n"),
            (DOC_COMMENT, "## doc"),
            (NEWLINE, "\r\n"),
            (COMMENT, "### rule"),
            (NEWLINE, "\n"),
            (NEWLINE, "\r"),
            (LNAME, "b"),
            (EOF, ""),
        ]
    );
}

#[test]
fn shebang_is_a_comment() {
    assert_eq!(clean("#!/usr/bin/env nl\nprint 1"), vec![(LNAME, "print"), (NUMBER, "1")]);
}

#[test]
fn layout_flags() {
    assert_eq!(
        layout("a b\nc  # x\n\n  d.e;f"),
        vec![
            (LNAME, false, false),
            (LNAME, false, true),
            (LNAME, true, true),
            (LNAME, true, true),
            (DOT, false, false),
            (LNAME, false, false),
            (SEMI, false, false),
            (LNAME, false, false),
            (EOF, true, true),
        ]
    );
    // A comment alone sets sp_before, not nl_before.
    assert_eq!(layout("a#c\n")[1], (EOF, true, true));
    assert_eq!(layout("(a)")[1], (LNAME, false, false));
}

#[test]
fn crlf_line_endings() {
    let src = "let a = 1\r\nlet b = \"x\"\r\nprint a\r\n";
    let l = layout(src);
    assert_eq!(l.iter().filter(|(_, nl, _)| *nl).count(), 3); // `let`, `print`, EOF
    assert!(errors(src).is_empty());
    let lexed = checked("a\r\n\r\nb");
    let newlines = lexed.tokens.iter().filter(|t| t.kind == NEWLINE).count();
    assert_eq!(newlines, 2);
}

// ---- Names and keywords -----------------------------------------------------

#[test]
fn names_and_classes() {
    assert_eq!(
        clean("health Player IO MAX_HP getHealth _ _Foo x1 Get_Health"),
        vec![
            (LNAME, "health"),
            (CNAME, "Player"),
            (CNAME, "IO"),
            (CNAME, "MAX_HP"),
            (ONAME, "getHealth"),
            (LNAME, "_"),
            (ONAME, "_Foo"),
            (LNAME, "x1"),
            (ONAME, "Get_Health"),
        ]
    );
}

#[test]
fn keywords() {
    assert_eq!(
        kinds("fn end self Self not and or then from step"),
        vec![FN_KW, END_KW, SELF_KW, SELF_TYPE_KW, NOT_KW, AND_KW, OR_KW, LNAME, LNAME, LNAME]
    );
    // Keywords are lexed as keywords after `.`, `:` and as keys; the parser
    // accepts them as names there.
    assert_eq!(kinds("obj.end"), vec![LNAME, DOT, END_KW]);
    assert_eq!(kinds("{type: 1}"), vec![L_CURLY, TYPE_KW, COLON, NUMBER, R_CURLY]);
    assert_eq!(kinds("x:end()"), vec![LNAME, COLON, END_KW, L_PAREN, R_PAREN]);
    // Words are maximal: `endx` and `if_` are names.
    assert_eq!(kinds("endx if_ ifx"), vec![LNAME, LNAME, LNAME]);
}

#[test]
fn at_names() {
    assert_eq!(clean("@health @type @Foo"), vec![(AT_NAME, "@health"), (AT_NAME, "@type"), (AT_NAME, "@Foo")]);
    let errs = errors("@ x");
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("@health"));
    assert_eq!(kinds("@1"), vec![ERROR, NUMBER]);
}

// ---- Numbers ----------------------------------------------------------------

#[test]
fn number_forms() {
    for n in [
        "1", "1.1", "0xabcdef", "0XAB", "0b1010", "0B1", "1_000_000", "5e+20", "0.3e12", "4.57e-3",
        "1e5", "1E-5", "0xff_ff", "0b1_0", "1_0.2_5e1_0", "007",
    ] {
        assert_eq!(clean(n), vec![(NUMBER, n)], "{n}");
    }
}

#[test]
fn number_then_dot() {
    assert_eq!(clean("6.is_cool"), vec![(NUMBER, "6"), (DOT, "."), (LNAME, "is_cool")]);
    assert_eq!(clean("1..10"), vec![(NUMBER, "1"), (DOT2, ".."), (NUMBER, "10")]);
    assert_eq!(clean("1..<10"), vec![(NUMBER, "1"), (DOT2_LT, "..<"), (NUMBER, "10")]);
    assert_eq!(clean("2.5.floor"), vec![(NUMBER, "2.5"), (DOT, "."), (LNAME, "floor")]);
    assert_eq!(clean("1.e5"), vec![(NUMBER, "1"), (DOT, "."), (LNAME, "e5")]);
    assert_eq!(clean("1..."), vec![(NUMBER, "1"), (DOT3, "...")]);
    assert_eq!(clean("100.divisible_by(10)")[..3], [(NUMBER, "100"), (DOT, "."), (LNAME, "divisible_by")]);
    assert_eq!(clean("1."), vec![(NUMBER, "1"), (DOT, ".")]);
    // `.5` is `.` then `5`; the parser reports it.
    assert_eq!(clean(".5"), vec![(DOT, "."), (NUMBER, "5")]);
    assert_eq!(clean("-5.abs"), vec![(MINUS, "-"), (NUMBER, "5"), (DOT, "."), (LNAME, "abs")]);
}

#[test]
fn malformed_numbers() {
    for (src, text) in [
        ("1__0", "1__0"),
        ("1_", "1_"),
        ("1_.5", "1_.5"),
        ("1._5", "1._5"),
        ("1e_5", "1e_5"),
        ("0x_ff", "0x_ff"),
        ("0x_", "0x_"),
        ("0x", "0x"),
        ("0b", "0b"),
        ("3px", "3px"),
        ("0x1g", "0x1g"),
        ("1e", "1e"),
        ("0b102", "0b102"),
        ("1.5abc", "1.5abc"),
    ] {
        let lexed = checked(src);
        assert_eq!(lexed.diagnostics.len(), 1, "{src}: {:?}", lexed.diagnostics);
        assert_eq!(toks(src)[0], (NUMBER, text), "{src}");
    }
    assert!(errors("3px")[0].contains("`3px` is not a number"));
    assert!(errors("1e")[0].contains("exponent"));
    assert!(errors("0b102")[0].contains("binary digit"));
    assert!(errors("1__0")[0].contains("misplaced `_`"));
    assert!(errors("0x")[0].contains("hexadecimal digits"));
    // `_1` is a name; `1._x` is a member access.
    assert_eq!(clean("_1"), vec![(LNAME, "_1")]);
    assert_eq!(clean("1._x"), vec![(NUMBER, "1"), (DOT, "."), (LNAME, "_x")]);
    // `1e+` without digits: `1e` is malformed, `+` stays an operator.
    assert_eq!(kinds("1e+x"), vec![NUMBER, PLUS, LNAME]);
}

// ---- Operators --------------------------------------------------------------

#[test]
fn longest_match() {
    let ops = ">>>= >>> >>= >> >= > <<= << <= <>= <> < ... ..< .. . **= ** *= * //= // /= / \
               ||= || |= |> | &&= && &= & == = += -= -> %= ^= ~ ^ % + - ( ) [ ] { } , ; : ??= ?? ?. ?";
    let lexed = toks(ops);
    let texts: Vec<&str> = lexed.iter().map(|(_, t)| *t).collect();
    let expected: Vec<&str> = ops.split_whitespace().collect();
    assert_eq!(texts, expected);
    for (k, t) in lexed {
        assert_eq!(k.fixed_text(), Some(t), "{k:?}");
    }
    assert_eq!(kinds("a..<b"), vec![LNAME, DOT2_LT, LNAME]);
    assert_eq!(kinds("a.. <b"), vec![LNAME, DOT2, LT, LNAME]);
    assert_eq!(kinds("x<>y"), vec![LNAME, CONCAT, LNAME]);
    assert_eq!(kinds("~="), vec![TILDE, EQ]);
    assert_eq!(kinds("=~"), vec![EQ, TILDE]);
    // The nil-safe tokens: `??=` before `??` before `?.`
    // before `?`; with a space inside they are separate tokens.
    assert_eq!(kinds("a?.b a??b x??=1"), vec![LNAME, QUESTION_DOT, LNAME, LNAME, QUESTION2, LNAME, LNAME, QUESTION2_EQ, NUMBER]);
    assert_eq!(kinds("? . ? ? T?? T?"), vec![QUESTION, DOT, QUESTION, QUESTION, CNAME, QUESTION2, CNAME, QUESTION]);
    assert_eq!(kinds("a???.b"), vec![LNAME, QUESTION2, QUESTION_DOT, LNAME]);
    assert_eq!(kinds("x?.end!"), vec![LNAME, QUESTION_DOT, END_KW, BANG_POSTFIX]);
    // `->` is the arrow of a short function; `- >` and `-->` are not.
    assert_eq!(kinds("a->b"), vec![LNAME, ARROW, LNAME]);
    assert_eq!(kinds("a - >b"), vec![LNAME, MINUS, GT, LNAME]);
    assert_eq!(kinds("a-->b"), vec![LNAME, MINUS, ARROW, LNAME]);
    assert_eq!(kinds("||->|a|->"), vec![PIPE2, ARROW, PIPE, LNAME, PIPE, ARROW]);
    assert_eq!(kinds("Box<Box<T>>"), vec![CNAME, LT, CNAME, LT, CNAME, SHR]);
    assert_eq!(kinds("x>>>=1"), vec![LNAME, USHR_EQ, NUMBER]);
    assert_eq!(kinds("x||=y&&=z"), vec![LNAME, PIPE2_EQ, LNAME, AMP2_EQ, LNAME]);
    assert_eq!(kinds("a|>f"), vec![LNAME, PIPE_GT, LNAME]);
    assert_eq!(kinds("do ||"), vec![DO_KW, PIPE2]);
}

#[test]
fn bang_forms() {
    assert_eq!(kinds("a != b"), vec![LNAME, NEQ, LNAME]);
    assert_eq!(kinds("a!= b"), vec![LNAME, NEQ, LNAME]);
    assert_eq!(kinds("a!=b"), vec![LNAME, NEQ, LNAME]);
    assert_eq!(kinds("a! = b"), vec![LNAME, BANG_POSTFIX, EQ, LNAME]);
    assert_eq!(kinds("f!"), vec![LNAME, BANG_POSTFIX]);
    assert_eq!(kinds("ON_HIT!"), vec![CNAME, BANG_POSTFIX]);
    assert_eq!(kinds("getX!"), vec![ONAME, BANG_POSTFIX]);
    assert_eq!(kinds("@handler!"), vec![AT_NAME, BANG_POSTFIX]);
    assert_eq!(kinds("x.m!"), vec![LNAME, DOT, LNAME, BANG_POSTFIX]);
    assert_eq!(kinds("x.end!"), vec![LNAME, DOT, END_KW, BANG_POSTFIX]);
    assert_eq!(kinds("x.\n  end!"), vec![LNAME, DOT, END_KW, BANG_POSTFIX]);
    assert_eq!(kinds("f()!"), vec![LNAME, L_PAREN, R_PAREN, BANG_POSTFIX]);
    assert_eq!(kinds("t[1]!"), vec![LNAME, L_BRACK, NUMBER, R_BRACK, BANG_POSTFIX]);
    assert_eq!(kinds("!a"), vec![BANG, LNAME]);
    assert_eq!(kinds("f(!a)"), vec![LNAME, L_PAREN, BANG, LNAME, R_PAREN]);
    assert_eq!(kinds("x && !y"), vec![LNAME, AMP2, BANG, LNAME]);
    assert_eq!(kinds("f !x"), vec![LNAME, BANG, LNAME]);
    assert_eq!(kinds("a !b"), vec![LNAME, BANG, LNAME]);
    assert_eq!(kinds("if!done"), vec![IF_KW, BANG, LNAME]);
    assert_eq!(kinds("return!ok"), vec![RETURN_KW, BANG, LNAME]);
    assert_eq!(kinds("self!"), vec![SELF_KW, BANG]);
    assert_eq!(kinds("1!"), vec![NUMBER, BANG]);
    assert_eq!(kinds("x#c\n!y"), vec![LNAME, BANG, LNAME]);
    assert_eq!(kinds("!!x"), vec![BANG, BANG, LNAME]);
    assert_eq!(kinds("\"#{f!}\""), vec![STR_BEGIN, LNAME, BANG_POSTFIX, STR_END]);
    assert_eq!(kinds("\"#{!f}\""), vec![STR_BEGIN, BANG, LNAME, STR_END]);
}

#[test]
fn colons_are_one_token() {
    assert_eq!(
        layout(":print a:b c :d"),
        vec![
            (COLON, false, false),
            (LNAME, false, false),
            (LNAME, false, true),
            (COLON, false, false),
            (LNAME, false, false),
            (LNAME, false, true),
            (COLON, false, true),
            (LNAME, false, false),
            (EOF, true, true),
        ]
    );
}

#[test]
fn stray_characters() {
    let lexed = checked("a $ b");
    assert_eq!(lexed.diagnostics.len(), 1);
    assert_eq!(kinds("a $ b"), vec![LNAME, ERROR, LNAME]);
    // A run of stray characters is one token and one diagnostic.
    assert_eq!(kinds("a $`$ b"), vec![LNAME, ERROR, LNAME]);
    assert_eq!(errors("a $`$ b").len(), 1);
    assert!(errors("x = \\")[0].contains("backslash"));
    assert!(errors("\u{7}")[0].contains("U+0007"));
}

#[test]
fn non_ascii() {
    // Inside strings and comments: fine.
    assert!(errors("print \"héllo wörld ✓\" # ça va 😀\nprint 'ünï'").is_empty());
    assert_eq!(decoded("\"héllo\""), vec!["héllo"]);
    // Outside: an error covering the whole character.
    let lexed = checked("let café = 1");
    assert_eq!(lexed.diagnostics.len(), 1);
    let errtok = lexed.tokens.iter().find(|t| t.kind == ERROR).unwrap();
    assert_eq!(errtok.text(lexed.source), "é");
    assert!(lexed.diagnostics[0].message.contains("U+00E9"));
    assert_eq!(kinds("x 😀😀 y"), vec![LNAME, ERROR, LNAME]);
    assert!(errors("print “hi”")[0].contains("typographic"));
    assert!(errors("a\u{A0}b")[0].contains("U+00A0"));
}

// ---- Raw strings ------------------------------------------------------------

#[test]
fn raw_strings() {
    assert_eq!(clean(r"'\d+'"), vec![(STRING, r"'\d+'")]);
    assert_eq!(decoded(r"'\d+'"), vec![r"\d+"]);
    assert_eq!(decoded(r"'it\'s'"), vec!["it's"]);
    assert_eq!(decoded(r"'a\\'"), vec![r"a\"]);
    assert_eq!(decoded(r"'#{x}'"), vec!["#{x}"]);
    assert_eq!(clean(r"'(?<year>\d{4})-(?<month>\d{2})'").len(), 1);
    let errs = errors("'abc\nx");
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("unterminated"));
    assert_eq!(kinds("'abc\nx"), vec![STRING, LNAME]);
    assert_eq!(kinds(r"'abc\'"), vec![STRING]);
    assert_eq!(errors(r"'abc\'").len(), 1);
}

// ---- Quoted strings ---------------------------------------------------------

#[test]
fn quoted_strings() {
    assert_eq!(clean("\"hello\""), vec![(STRING, "\"hello\"")]);
    assert_eq!(clean("\"\""), vec![(STRING, "\"\"")]);
    assert_eq!(decoded(r#""quote \" and backslash \\""#), vec![r#"quote " and backslash \"#]);
    assert_eq!(decoded(r#""line1\nline2""#), vec!["line1\nline2"]);
    assert_eq!(decoded(r#""a # b""#), vec!["a # b"]);
    assert_eq!(decoded(r#""\x41\u{263A}\065""#), vec!["A☺A"]);
}

#[test]
fn escaped_interpolation() {
    assert_eq!(clean(r#""literal \#{name}""#), vec![(STRING, r#""literal \#{name}""#)]);
    assert_eq!(decoded(r#""literal \#{name}""#), vec!["literal #{name}"]);
    assert_eq!(kinds(r#""\\#{x}""#), vec![STR_BEGIN, LNAME, STR_END]);
}

#[test]
fn bad_escapes() {
    for (src, needle) in [
        (r#""\d""#, "unknown escape `\\d`"),
        (r#""\x4""#, "two hexadecimal digits"),
        (r#""\256""#, "larger than 255"),
        (r#""\u41""#, "braces"),
        (r#""\u{}""#, "between the braces"),
        (r#""\u{110000}""#, "10FFFF"),
        (r#""\u{41""#, "not closed"),
        ("\"\\é\"", "unknown escape `\\é`"),
    ] {
        let errs = errors(src);
        assert_eq!(errs.len(), 1, "{src}: {errs:?}");
        assert!(errs[0].contains(needle), "{src}: {errs:?}");
        assert_eq!(kinds(src), vec![STRING], "{src}");
    }
}

#[test]
fn backslash_line_break() {
    // In a `"…"` string a backslash before a line break continues the string
    // on the next line and writes one `\n`, whatever the line break.
    assert_eq!(clean("\"aaa \\\nbbb \\\nccc\""), vec![(STRING, "\"aaa \\\nbbb \\\nccc\"")]);
    assert_eq!(decoded("\"aaa \\\nbbb \\\nccc\""), vec!["aaa \nbbb \nccc"]);
    assert_eq!(decoded("\"a\\\r\nb\""), vec!["a\nb"]);
    assert_eq!(kinds("\"a\\\nb\"\ny"), vec![STRING, LNAME]);
    // After an interpolation too.
    assert_eq!(kinds("\"a#{x}\\\nb\""), vec![STR_BEGIN, LNAME, STR_END]);
    assert_eq!(decoded("\"a#{x}\\\nb\""), vec!["a", "\nb"]);
    // The next line must still close the string.
    let errs = errors("\"abc\\\nx\ny");
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("unterminated"));
    // Not in a raw string, and not in a heredoc.
    assert_eq!(errors("'abc\\\nx'").len(), 2);
    let errs = errors("\"\"\"\na\\\nb\n\"\"\"");
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("in a heredoc"));
}

#[test]
fn unterminated_quoted() {
    let errs = errors("print \"abc\nprint 1");
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("unterminated") && errs[0].contains("\"\"\""));
    assert_eq!(kinds("print \"abc\nprint 1"), vec![LNAME, STRING, LNAME, NUMBER]);
    assert_eq!(kinds("\"abc"), vec![STRING]);
    assert_eq!(kinds("\"a#{x}b\nc"), vec![STR_BEGIN, LNAME, STR_END, LNAME]);
}

#[test]
fn interpolation() {
    assert_eq!(
        clean("\"hello, #{name}!\""),
        vec![(STR_BEGIN, "\"hello, #{"), (LNAME, "name"), (STR_END, "}!\"")]
    );
    assert_eq!(
        clean("\"#{n} + 1 = #{n + 1}\""),
        vec![
            (STR_BEGIN, "\"#{"),
            (LNAME, "n"),
            (STR_MID, "} + 1 = #{"),
            (LNAME, "n"),
            (PLUS, "+"),
            (NUMBER, "1"),
            (STR_END, "}\""),
        ]
    );
    assert_eq!(decoded("\"#{n} + 1 = #{n + 1}\""), vec!["", " + 1 = ", ""]);
    // Braces nest inside an interpolation.
    assert_eq!(
        kinds("\"#{ {a: 1}.a }\""),
        vec![STR_BEGIN, L_CURLY, LNAME, COLON, NUMBER, R_CURLY, DOT, LNAME, STR_END]
    );
    // Strings inside interpolations inside strings.
    assert_eq!(
        clean("\"a#{ \"b#{c}\" }d\""),
        vec![
            (STR_BEGIN, "\"a#{"),
            (STR_BEGIN, "\"b#{"),
            (LNAME, "c"),
            (STR_END, "}\""),
            (STR_END, "}d\""),
        ]
    );
    assert_eq!(kinds("\"#{f(\"x\")}\""), vec![STR_BEGIN, LNAME, L_PAREN, STRING, R_PAREN, STR_END]);
    assert_eq!(kinds("\"#{ '}' }\""), vec![STR_BEGIN, STRING, STR_END]);
    assert_eq!(kinds("\"#{@name} died\""), vec![STR_BEGIN, AT_NAME, STR_END]);
    // Three levels.
    assert_eq!(
        kinds("\"#{\"#{\"#{x}\"}\"}\""),
        vec![STR_BEGIN, STR_BEGIN, STR_BEGIN, LNAME, STR_END, STR_END, STR_END]
    );
    // A `}` outside any interpolation is an ordinary token.
    assert_eq!(kinds("}"), vec![R_CURLY]);
    // The layout flags of embedded tokens are ordinary.
    assert_eq!(
        layout("\"#{ a}\""),
        vec![(STR_BEGIN, false, false), (LNAME, false, true), (STR_END, false, false), (EOF, true, true)]
    );
}

#[test]
fn empty_interpolation() {
    let errs = errors("\"a#{}b\"");
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("empty interpolation"));
    assert_eq!(kinds("\"a#{}b\""), vec![STR_BEGIN, STR_END]);
    assert_eq!(errors("\"a#{ }b\"").len(), 1);
    assert_eq!(errors("\"#{x}#{}\"").len(), 1);
}

#[test]
fn line_break_in_interpolation_of_one_line_string() {
    let src = "print \"a #{b\nprint c";
    let errs = errors(src);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("must close on the line"));
    assert_eq!(kinds(src), vec![LNAME, STR_BEGIN, LNAME, STR_END, LNAME, LNAME]);
    // Nested one-line strings are all closed.
    let src = "\"a#{ \"b#{ c\nd";
    assert_eq!(kinds(src), vec![STR_BEGIN, STR_BEGIN, LNAME, STR_END, STR_END, LNAME]);
    // A comment inside the interpolation runs to the end of the line.
    let src = "\"#{ x # c }\"\ny";
    assert_eq!(kinds(src), vec![STR_BEGIN, LNAME, STR_END, LNAME]);
    assert_eq!(errors(src).len(), 1);
    // When the next line closes the interpolation and the string, the
    // expression goes on there: one error.
    let src = "a = \"x #{ 1 +\n2 }\"\ny = 1";
    assert_eq!(errors(src).len(), 1, "{:?}", errors(src));
    assert_eq!(kinds(src), vec![LNAME, EQ, STR_BEGIN, NUMBER, PLUS, NUMBER, STR_END, LNAME, EQ, NUMBER]);
    // Only the next line counts: there the string is cut off as before.
    let src = "\"#{ f(\n1,\n2) }\"";
    assert_eq!(kinds(src)[..3], [STR_BEGIN, LNAME, L_PAREN]);
}

#[test]
fn unclosed_interpolation_at_eof() {
    let src = "\"a#{b";
    assert_eq!(kinds(src), vec![STR_BEGIN, LNAME, STR_END]);
    assert_eq!(errors(src).len(), 1);
    let src = "\"\"\"\n#{\"#{";
    assert_eq!(kinds(src), vec![STR_BEGIN, STR_BEGIN, STR_END, STR_END]);
}

// ---- Heredocs ---------------------------------------------------------------

#[test]
fn heredoc_plain() {
    let src = "let plain = \"\"\"\nmulti\nline\n\"\"\"\nprint plain";
    assert_eq!(kinds(src), vec![LET_KW, LNAME, EQ, STRING, LNAME, LNAME]);
    assert!(errors(src).is_empty());
    assert_eq!(decoded("\"\"\"\nmulti\nline\n\"\"\""), vec!["multi\nline"]);
    // Blanks after the opener are allowed.
    assert_eq!(decoded("\"\"\"  \t\nx\n\"\"\""), vec!["x"]);
    // Empty heredoc.
    assert_eq!(decoded("\"\"\"\n\"\"\""), vec![""]);
    // `"` needs no escape; `"""` not at the start of a line is text.
    assert_eq!(decoded("\"\"\"\nsay \"hi\" and \"\"\" ok\n\"\"\""), vec!["say \"hi\" and \"\"\" ok"]);
    // A line starting with `\"""` is content.
    assert_eq!(decoded("\"\"\"\n\\\"\"\"\n\"\"\""), vec!["\"\"\""]);
    // Code may follow the closer.
    assert_eq!(kinds("f(\"\"\"\nx\n  \"\"\".strip, y)"), vec![LNAME, L_PAREN, STRING, DOT, LNAME, COMMA, LNAME, R_PAREN]);
}

#[test]
fn heredoc_flags() {
    let src = "\"\"\"-\n  first\n    indented\n  last\n\"\"\"";
    assert!(errors(src).is_empty());
    assert_eq!(decoded(src), vec!["first\n  indented\nlast"]);
    let src = "\"\"\"=\nno #{a} and no \\n escape\n\"\"\"";
    assert!(errors(src).is_empty());
    assert_eq!(kinds(src), vec![STRING]);
    assert_eq!(decoded(src), vec!["no #{a} and no \\n escape"]);
    let src = "\"\"\"-=\n    kept #{a}\n    as written \\q\n\"\"\"";
    assert!(errors(src).is_empty());
    assert_eq!(decoded(src), vec!["kept #{a}\nas written \\q"]);
    // `=-` is an error, read as `-=`.
    let src = "\"\"\"=-\n  x\n\"\"\"";
    let errs = errors(src);
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("`-=`"));
    assert_eq!(decoded(src), vec!["x"]);
}

#[test]
fn heredoc_dedent_details() {
    // The closer may stand at any column; blank lines become empty.
    let src = "  \"\"\"-\n    a\n      \n    b\n  \"\"\"";
    assert_eq!(decoded(src), vec!["a\n\nb"]);
    // Tabs and spaces compared character for character.
    assert_eq!(decoded("\"\"\"-\n\t\ta\n\t b\n\"\"\""), vec!["\ta\n b"]);
    // Escapes are decoded after dedent, so `\t` does not count as indentation.
    assert_eq!(decoded("\"\"\"-\n  \\tx\n  y\n\"\"\""), vec!["\tx\ny"]);
    // Without `-`, indentation is kept.
    assert_eq!(decoded("\"\"\"\n  a\n\"\"\""), vec!["  a"]);
}

#[test]
fn heredoc_interpolation() {
    let src = "\"\"\"\nmulti\nline #{a}\n\"\"\"";
    assert_eq!(
        clean(src),
        vec![(STR_BEGIN, "\"\"\"\nmulti\nline #{"), (LNAME, "a"), (STR_END, "}\n\"\"\"")]
    );
    assert_eq!(decoded(src), vec!["multi\nline ", ""]);
    let src = "\"\"\"-\n  first\n    indented\n  last #{a}\n\"\"\"";
    assert_eq!(decoded(src), vec!["first\n  indented\nlast ", ""]);
    // An interpolation in a heredoc may span lines; its tokens get nl_before.
    let src = "\"\"\"\nx #{f(a,\n  b)} y\n\"\"\"";
    assert!(errors(src).is_empty());
    let l = layout(src);
    let newline_tokens: Vec<_> = l.iter().filter(|(_, nl, _)| *nl).map(|t| t.0).collect();
    assert_eq!(newline_tokens, vec![LNAME, EOF]); // `b`, then EOF
    assert_eq!(decoded(src), vec!["x ", " y"]);
    // Non-ASCII inside an interpolation is outside the string literal.
    assert_eq!(kinds("\"#{ é }\""), vec![STR_BEGIN, ERROR, STR_END]);
    // A one-line string inside a heredoc interpolation.
    let src = "\"\"\"\n#{ \"a#{b}c\" }\n\"\"\"";
    assert!(errors(src).is_empty());
    assert_eq!(kinds(src), vec![STR_BEGIN, STR_BEGIN, LNAME, STR_END, STR_END]);
    // `\#{` in a heredoc.
    assert_eq!(decoded("\"\"\"\n\\#{x}\n\"\"\""), vec!["#{x}"]);
}

#[test]
fn heredoc_errors() {
    // Text after the opener.
    let errs = errors("\"\"\" x\nabc\n\"\"\"");
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("nothing may follow"));
    let errs = errors("\"\"\" # note\nabc\n\"\"\"");
    assert!(errs[0].contains("comment"));
    // Python-style one-line heredoc.
    let src = "x = \"\"\"abc\"\"\"\ny";
    assert_eq!(kinds(src), vec![LNAME, EQ, STRING, LNAME]);
    assert!(errors(src)[0].contains("for one line"));
    // Unterminated.
    let errs = errors("\"\"\"\nabc\n");
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("unterminated heredoc"));
    assert_eq!(errors("\"\"\"").len(), 1);
    assert_eq!(errors("\"\"\"-").len(), 1);
    // Heredoc inside an interpolation.
    let src = "\"#{ \"\"\"\nx\n\"\"\" }\"";
    let errs = errors(src);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("inside an interpolation"));
    assert_eq!(kinds(src), vec![STR_BEGIN, STRING, STR_END]);
    // The closer line ends a heredoc whose interpolation was left open.
    let src = "\"\"\"\nhello #{name\n\"\"\"\nprint 1";
    let errs = errors(src);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("not closed"));
    assert_eq!(kinds(src), vec![STR_BEGIN, LNAME, STR_END, LNAME, NUMBER]);
    assert_eq!(decoded("\"\"\"\nhello #{name\n\"\"\""), vec!["hello ", ""]);
    // A backslash before a line break.
    assert_eq!(errors("\"\"\"\na\\\nb\n\"\"\"").len(), 1);
}

#[test]
fn significant_indices() {
    let lexed = checked("a # c\n b");
    let idx: Vec<usize> = lexed.significant().collect();
    assert_eq!(idx.len(), 3);
    assert_eq!(lexed.kind(idx[1]), LNAME);
    assert_eq!(lexed.text(idx[1]), "b");
    assert_eq!(lexed.kind(999), EOF);
    assert_eq!(lexed.text(999), "");
}

#[test]
fn invalid_utf8() {
    assert_eq!(crate::source_from_bytes(b"ok"), Ok("ok"));
    let err = crate::source_from_bytes(b"ab\xffcd").unwrap_err();
    assert_eq!(u32::from(err.range.start()), 2);
    assert!(err.message.contains("UTF-8"));
    let err = crate::source_from_bytes(b"ab\xe2\x82").unwrap_err();
    assert_eq!(u32::from(err.range.start()), 2);
}

/// The lexer and `decode_string` scan strings separately (the lexer to find
/// the tokens, the decoder to find the text of each token). For every string
/// in the conformance programs and the valid snapshot cases, the decoder
/// stops exactly where the lexer ended the part: at the closing quote, at the
/// `#{` of an interpolation, or at the heredoc's closer line.
#[test]
fn lexer_and_decoder_agree_on_string_parts() {
    use crate::literal::content_ends;
    use crate::{StringForm, string_form};
    fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.map(|e| e.unwrap().path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                files(&p, out);
            } else if p.extension().is_some_and(|e| e == "nl") {
                out.push(p);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let mut paths = Vec::new();
    files(&root.join("run"), &mut paths);
    files(&root.join("syntax/ok"), &mut paths);
    assert!(paths.len() > 300, "{}", paths.len());
    let mut literals = 0;
    for path in &paths {
        let src = std::fs::read_to_string(path).unwrap();
        let lexed = lex(&src);
        // Parts of each literal, outermost strings last (they close last).
        let mut open: Vec<Vec<(SyntaxKind, &str)>> = Vec::new();
        let mut done: Vec<Vec<(SyntaxKind, &str)>> = Vec::new();
        for (i, t) in lexed.tokens.iter().enumerate() {
            let text = lexed.text(i);
            match t.kind {
                STRING => done.push(vec![(STRING, text)]),
                STR_BEGIN => open.push(vec![(STR_BEGIN, text)]),
                STR_MID => open.last_mut().unwrap().push((STR_MID, text)),
                STR_END => {
                    let mut parts = open.pop().unwrap();
                    parts.push((STR_END, text));
                    done.push(parts);
                }
                _ => {}
            }
        }
        for parts in done {
            literals += 1;
            let texts: Vec<&str> = parts.iter().map(|p| p.1).collect();
            let ends = content_ends(&texts);
            let form = string_form(texts[0]).unwrap();
            for ((kind, text), end) in parts.iter().zip(&ends) {
                let expected = match (form, kind) {
                    (_, STR_BEGIN | STR_MID) => Some(text.len() - 2),
                    (StringForm::Heredoc { .. }, _) => {
                        let closer = text.rfind(['\n', '\r']).map_or(0, |i| i + 1);
                        Some(closer)
                    }
                    _ => Some(text.len() - 1),
                };
                assert_eq!(*end, expected, "{}: part {text:?} of {texts:?}", path.display());
            }
        }
    }
    assert!(literals > 500, "{literals}");
}
