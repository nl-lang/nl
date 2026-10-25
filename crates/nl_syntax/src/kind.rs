//! [`SyntaxKind`]: the kinds of tokens and of syntax tree nodes.

/// Defines [`SyntaxKind`], its table of all kinds and the fixed text of each
/// kind that has one (keywords and punctuation).
macro_rules! syntax_kinds {
    ( $( $(#[$attr:meta])* $name:ident $(= $text:literal)? ),* $(,)? ) => {
        /// The kind of a token or of a syntax tree node.
        ///
        /// One `#[repr(u16)]` enum, as `rowan` expects. The discriminants are
        /// `0, 1, 2, …` in declaration order, so [`SyntaxKind::from_raw`] can
        /// convert back without `unsafe`.
        ///
        /// Token kinds come first, node kinds after [`SyntaxKind::EOF`];
        /// [`SyntaxKind::is_token`] relies on that order.
        ///
        /// In the syntax tree, a token's kind is the lexer's with two kinds of
        /// exception, both decided by position: the contextual keywords
        /// `from`, `step` and `then` become [`SyntaxKind::FROM_KW`],
        /// [`SyntaxKind::STEP_KW`] and [`SyntaxKind::THEN_KW`] where they
        /// are keywords; and a keyword
        /// used as a name (after `.`, after the `:` of a method
        /// call or a Lua path, as a table, struct literal, pattern or record
        /// key, as a declared field or method name) becomes `LNAME`, or
        /// `CNAME` for `Self`. So in `x.end`, `{type: 1}` and `fn Foo.if`
        /// the names are `LNAME` tokens, and every `END_KW` in a tree closes
        /// a block.
        ///
        /// Names follow the token classes of the grammar (`LNAME`,
        /// `STR_BEGIN`, …). Keywords end in `_KW`. Punctuation is named after its
        /// characters (`DOT3` is `...`); [`SyntaxKind::fixed_text`] gives
        /// the text of every keyword and punctuation kind.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u16)]
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        pub enum SyntaxKind {
            $( $(#[$attr])* $name, )*
        }

        impl SyntaxKind {
            /// Every kind, in discriminant order: `ALL[k as usize] == k`.
            pub const ALL: &'static [SyntaxKind] = &[ $( SyntaxKind::$name, )* ];

            /// The source text of a keyword or punctuation kind (`"end"`,
            /// `">>>="`), `None` for kinds whose text varies (names, literals,
            /// trivia, `ERROR`, `EOF`). Useful for "expected `end`" messages.
            pub const fn fixed_text(self) -> Option<&'static str> {
                match self {
                    $( SyntaxKind::$name => syntax_kinds!(@text $($text)?), )*
                }
            }
        }
    };
    (@text) => { None };
    (@text $text:literal) => { Some($text) };
}

syntax_kinds! {
    // ---- Trivia -------------------------------------------------------------
    /// A run of spaces and tabs. A byte-order mark at the very start of the
    /// file is also lexed as `WHITESPACE` (the lexer skips it).
    WHITESPACE,
    /// One line break: LF, CRLF or a lone CR (always exactly one per token).
    NEWLINE,
    /// `#` to the end of the line, without the line break. Includes `#!` on
    /// the first line.
    COMMENT,
    /// `##` to the end of the line, without the line break.
    /// Whether a run of them attaches to a declaration is decided by the
    /// parser or tree builder.
    DOC_COMMENT,

    // ---- Errors -------------------------------------------------------------
    /// Text the lexer could not make a token of: stray characters (`$`, a
    /// non-ASCII character outside strings and comments, a lone `@`). The
    /// lexer has already reported a diagnostic; the parser should skip it
    /// without reporting again. Not trivia: it carries layout flags.
    ERROR,

    // ---- Names --------------------------------------------------------------
    /// A lowercase name: a word with no uppercase letter (`health`, `x1`,
    /// `_`, `_tmp`). Also the contextual keywords `from` and `step`. In the
    /// syntax tree, also a keyword used as a name (see [`SyntaxKind`]).
    LNAME,
    /// A capitalized name: PascalCase (`Player`), all capitals (`IO`, `T1`)
    /// or SCREAMING_SNAKE_CASE (`MAX_HP`). See [`crate::name_case`] for the
    /// finer class.
    CNAME,
    /// A name in any other style (`getHealth`, `Get_Health`, `_Foo`). Only
    /// allowed in Lua paths and as a `:` method name.
    ONAME,
    /// `@` directly followed by any word, keywords included: `@health`,
    /// `@type`. The token text includes the `@`.
    AT_NAME,

    // ---- Literals -----------------------------------------------------------
    /// A number literal, without sign. [`crate::number_value`]
    /// gives its value. A malformed number (`3px`, `1__0`, `0x`) is still one
    /// `NUMBER` token, with a diagnostic.
    NUMBER,
    /// A complete string literal without interpolation, of any of the six
    /// forms (raw `'…'`, `"…"`, and the four heredocs), including its quotes.
    /// [`crate::decode_string`] gives its value.
    STRING,
    /// The start of an interpolated string, up to and including the first
    /// `#{`: `"hello #{`. The tokens of the embedded expression follow.
    STR_BEGIN,
    /// The text between two interpolations, from `}` to the next `#{`,
    /// both included: `} and #{`.
    STR_MID,
    /// The end of an interpolated string, from the last `}` to the closing
    /// quote, both included: `}!"`. When the lexer recovers from an error
    /// (end of file or of line inside an interpolation) it emits an empty
    /// `STR_END`, so every `STR_BEGIN` is matched by exactly one `STR_END`.
    STR_END,

    // ---- Keywords -----------------------------------------------------------
    AND_KW = "and",
    AS_KW = "as",
    ASYNC_KW = "async",
    BREAK_KW = "break",
    CASE_KW = "case",
    CATCH_KW = "catch",
    COND_KW = "cond",
    CONST_KW = "const",
    CONTINUE_KW = "continue",
    /// Reserved for declaration files.
    DECLARE_KW = "declare",
    DO_KW = "do",
    ELSE_KW = "else",
    ELSEIF_KW = "elseif",
    END_KW = "end",
    ENUM_KW = "enum",
    FALSE_KW = "false",
    FINALLY_KW = "finally",
    FN_KW = "fn",
    FOR_KW = "for",
    GLOBAL_KW = "global",
    IF_KW = "if",
    IMPORT_KW = "import",
    IN_KW = "in",
    INTERFACE_KW = "interface",
    IS_KW = "is",
    LET_KW = "let",
    LOOP_KW = "loop",
    MODULE_KW = "module",
    NIL_KW = "nil",
    NOT_KW = "not",
    ON_KW = "on",
    OR_KW = "or",
    PUB_KW = "pub",
    RETURN_KW = "return",
    /// `self`
    SELF_KW = "self",
    /// `Self`
    SELF_TYPE_KW = "Self",
    SPAWN_KW = "spawn",
    STRUCT_KW = "struct",
    TRUE_KW = "true",
    TRY_KW = "try",
    TYPE_KW = "type",
    UNLESS_KW = "unless",
    UNTIL_KW = "until",
    WHEN_KW = "when",
    WHILE_KW = "while",

    // ---- Operators ----------------------------------------------------------
    PLUS = "+",
    MINUS = "-",
    STAR = "*",
    SLASH = "/",
    SLASH2 = "//",
    PERCENT = "%",
    STAR2 = "**",
    /// `<>`, concatenation.
    CONCAT = "<>",
    PIPE = "|",
    AMP = "&",
    CARET = "^",
    TILDE = "~",
    SHL = "<<",
    SHR = ">>",
    /// `>>>`, logical shift right.
    USHR = ">>>",
    EQ2 = "==",
    /// `!=`. Always one token when `!` is directly followed by `=`.
    NEQ = "!=",
    LT = "<",
    LT_EQ = "<=",
    GT = ">",
    GT_EQ = ">=",
    AMP2 = "&&",
    PIPE2 = "||",
    /// Prefix `!` (logical not). See [`SyntaxKind::BANG_POSTFIX`].
    BANG = "!",
    /// Postfix `!` ("the function itself, no call"): a `!`
    /// that touches a preceding name token, `AT_NAME`, keyword directly
    /// after `.`, `)` or `]`, and is not followed by `=`.
    BANG_POSTFIX = "!",
    /// `|>`
    PIPE_GT = "|>",
    DOT2 = "..",
    /// `..<`: not an operator of NL; lexed as one token so that the parser can
    /// report it and name `...`.
    DOT2_LT = "..<",
    /// `...`: the exclusive range.
    DOT3 = "...",
    /// `??`, nil-coalescing.
    QUESTION2 = "??",
    EQ = "=",
    PLUS_EQ = "+=",
    MINUS_EQ = "-=",
    STAR_EQ = "*=",
    SLASH_EQ = "/=",
    SLASH2_EQ = "//=",
    PERCENT_EQ = "%=",
    STAR2_EQ = "**=",
    CONCAT_EQ = "<>=",
    PIPE_EQ = "|=",
    AMP_EQ = "&=",
    CARET_EQ = "^=",
    SHL_EQ = "<<=",
    SHR_EQ = ">>=",
    USHR_EQ = ">>>=",
    PIPE2_EQ = "||=",
    AMP2_EQ = "&&=",
    /// `??=`: assigns when the target is `nil`.
    QUESTION2_EQ = "??=",

    // ---- Brackets and punctuation -------------------------------------------
    L_PAREN = "(",
    R_PAREN = ")",
    L_BRACK = "[",
    R_BRACK = "]",
    /// `{`. Inside an interpolation it also raises the brace depth.
    L_CURLY = "{",
    /// `}`. The `}` that closes an interpolation is not an `R_CURLY`: it is
    /// the first character of the following `STR_MID` or `STR_END`.
    R_CURLY = "}",
    COMMA = ",",
    /// `;`, which the parser treats like a significant line break.
    SEMI = ";",
    DOT = ".",
    /// Every `:`. The parser classifies it (method call, Lua path,
    /// annotation, key) from its position and the `sp_before` flags of the
    /// colon and of the next token.
    COLON = ":",
    /// `?` alone: the optional-type marker `T?`.
    QUESTION = "?",
    /// `->`, between the parameters and the body of a short function.
    ARROW = "->",
    /// `?.`, the nil-safe step. In the tree it stands where
    /// the step's `.` would, or before its `[`, `(` or `:`, inside the
    /// step's usual node: a `FIELD_EXPR`, `INDEX_EXPR`, `CALL_EXPR`,
    /// `METHOD_CALL`, or a `LUA_FIELD`, `LUA_INDEX`, `LUA_CALL` or
    /// `LUA_METHOD` step of a Lua path.
    QUESTION_DOT = "?.",

    // ---- Contextual keywords ------------------------------------------------
    // The lexer never produces these: it emits `from`, `step` and `then` as
    // `LNAME`.
    // The parser gives the token this kind in the tree where it recognizes
    // the word as a keyword by position.
    /// `from` in an `import` statement.
    FROM_KW = "from",
    /// `step` in a `for` header.
    STEP_KW = "step",
    /// `then` after the condition of `if`, `unless` or `elseif`, or after
    /// the values of a `when`: the body follows on the same line.
    THEN_KW = "then",

    /// End of file: an empty token at the end of the input, always the last
    /// token, with `nl_before` and `sp_before` set.
    EOF,

    // ---- Node kinds ---------------------------------------------------------
    // Children are listed in source order;
    // trivia and `ERROR` tokens may appear between any two of them.

    /// The whole file: `BLOCK`, then `EOF`. Tokens the parser had to
    /// give up on after a fatal problem (nesting too deep) sit in an
    /// `ERROR_NODE` before `EOF`.
    SOURCE_FILE,
    /// A sequence of statements: a file, a body, a branch.
    BLOCK,
    /// Tokens the parser could not fit into the grammar; a diagnostic was
    /// reported for them.
    ERROR_NODE,

    // -- Names
    /// A name being declared or bound: one name token (or a keyword where any
    /// word is allowed).
    NAME,
    /// A name read in an expression: one `LNAME`, `CNAME` or `ONAME` token.
    NAME_REF,
    /// A capitalized name path `Foo.Bar.Baz` (`CNAME`/`DOT` tokens; `Self` may
    /// start it in a struct pattern): module names, `fn` owners, event names,
    /// struct pattern names, named types.
    QUAL_NAME,

    // -- Declarations. `pub`, when present, is the first token.
    /// `[pub] [async] fn NAME …` or `fn QUAL_NAME . (NAME | OP_NAME) …`,
    /// then `TYPE_PARAMS?`, `PARAM_LIST?`, `RETURN_TYPE?`, `BLOCK`, `end`.
    /// Also the methods of an `interface … for`.
    FN_DECL,
    /// An operator method name after `fn Type.`: `+`, `[` `]` `=`, …
    OP_NAME,
    /// `( PARAM, … )`.
    PARAM_LIST,
    /// A pattern, optionally `= expr` (a default).
    PARAM,
    /// `*name [: type]`.
    REST_PARAM,
    /// `**name [: type]`: the keyed arguments.
    KEYED_PARAM,
    /// `: type` after a function header, block parameters or a `Function(…)`.
    RETURN_TYPE,
    /// `< TYPE_PARAM, … >`.
    TYPE_PARAMS,
    /// One type parameter: `NAME`.
    TYPE_PARAM,
    /// `let` patterns `[= EXPR_LIST]`.
    LET_DECL,
    /// `[pub] const` patterns `= EXPR_LIST`.
    CONST_DECL,
    /// `global NAME [: type] [= expr]`.
    GLOBAL_DECL,
    /// `[pub] struct NAME TYPE_PARAMS? STRUCT_PARENT? FIELD_DECL* end`.
    STRUCT_DECL,
    /// `: type` naming the parent struct.
    STRUCT_PARENT,
    /// `NAME [: type] [= expr]`.
    FIELD_DECL,
    /// `[pub] interface NAME TYPE_PARAMS? (IFACE_METHOD | IFACE_FIELD)* end`.
    INTERFACE_DECL,
    /// A method signature in an interface: `[async] fn NAME TYPE_PARAMS?
    /// PARAM_LIST? RETURN_TYPE?`, no body.
    IFACE_METHOD,
    /// A field requirement in an interface: `NAME : type`.
    IFACE_FIELD,
    /// `interface type for type FN_DECL* end`.
    INTERFACE_IMPL,
    /// `[pub] enum NAME VARIANT* end`.
    ENUM_DECL,
    /// `NAME [= value]`; the value is a `LITERAL` or a `PREFIX_EXPR` (`-1`).
    VARIANT,
    /// `[pub] type NAME TYPE_PARAMS? = type`.
    TYPE_DECL,
    /// `[pub] module QUAL_NAME BLOCK end`. There is no `BLOCK` in
    /// `module Foo end` written on one line.
    MODULE_DECL,
    /// `import NAME from STRING`, `import IMPORT_LIST from STRING`, or
    /// `import from STRING as NAME`.
    IMPORT_DECL,
    /// `{ NAME, … }` of an import.
    IMPORT_LIST,

    // -- Statements. A simple statement may end in a MODIFIER.
    /// An expression used as a statement, or an `EXPR_LIST` of several
    /// values, which is valid only as the tail of a function body.
    EXPR_STMT,
    /// `target = value`.
    ASSIGN_STMT,
    /// `target op= value`.
    COMPOUND_ASSIGN_STMT,
    /// `TARGET_LIST = EXPR_LIST`.
    MULTI_ASSIGN_STMT,
    /// The targets of a multiple assignment, separated by `,`.
    TARGET_LIST,
    /// Values separated by `,` (`let`, `const`, `return`, multiple
    /// assignment).
    EXPR_LIST,
    /// `return [EXPR_LIST]`.
    RETURN_STMT,
    /// `break`.
    BREAK_STMT,
    /// `continue`.
    CONTINUE_STMT,
    /// A statement modifier: `if`/`unless`/`while`/`until` and a condition.
    MODIFIER,
    /// A scope block `do BLOCK end`.
    DO_BLOCK,
    /// `while cond BLOCK end`.
    WHILE_STMT,
    /// `until cond BLOCK end`.
    UNTIL_STMT,
    /// `loop BLOCK end`.
    LOOP_STMT,
    /// `for patterns in expr FOR_STEP? BLOCK end`.
    FOR_STMT,
    /// `step expr` in a `for` header.
    FOR_STEP,

    // -- Block expressions
    /// `if cond BLOCK ELSEIF_CLAUSE* ELSE_CLAUSE? end`. The condition is an
    /// expression or a `LET_CONDITION`.
    IF_EXPR,
    /// `let pattern = expr` as the condition of `if`/`elseif`.
    LET_CONDITION,
    /// `elseif cond BLOCK`.
    ELSEIF_CLAUSE,
    /// `else BLOCK`.
    ELSE_CLAUSE,
    /// `unless cond BLOCK ELSE_CLAUSE? end`.
    UNLESS_EXPR,
    /// `case expr WHEN_CLAUSE* ELSE_CLAUSE? end`.
    CASE_EXPR,
    /// `when item, … WHEN_GUARD? BLOCK`. Each item is a pattern node (a
    /// `when_pattern`) or an expression node.
    WHEN_CLAUSE,
    /// `if cond` after the items of a `when`.
    WHEN_GUARD,
    /// `cond COND_CLAUSE* ELSE_CLAUSE? end`.
    COND_EXPR,
    /// `when cond BLOCK` of a `cond`.
    COND_CLAUSE,
    /// `try BLOCK CATCH_CLAUSE* FINALLY_CLAUSE? end`.
    TRY_EXPR,
    /// `catch [NAME [: type]] BLOCK`.
    CATCH_CLAUSE,
    /// `finally BLOCK`.
    FINALLY_CLAUSE,

    // -- Expressions
    /// `NUMBER`, `STRING`, `true`, `false` or `nil`.
    LITERAL,
    /// `STR_BEGIN expr (STR_MID expr)* STR_END`.
    INTERP_STRING,
    /// `self` or `Self` used as a value.
    SELF_EXPR,
    /// `@name`.
    AT_EXPR,
    /// `( expr )`.
    PAREN_EXPR,
    /// `[ expr | SPREAD_ARG, … ]`: a list.
    LIST_EXPR,
    /// `{ TABLE_PAIR, … }`: a map. A `TABLE_ITEM` in it is an
    /// error that the parser reported.
    TABLE_EXPR,
    /// A positional table item: an expression. Only in an erroneous map.
    TABLE_ITEM,
    /// `TABLE_KEY : expr`.
    TABLE_PAIR,
    /// A word, `NUMBER` or `STRING` key; `[ expr ]` is an error that the
    /// parser reported.
    TABLE_KEY,
    /// `path { FIELD_INIT, … }`; the path is a `NAME_REF`,
    /// `SELF_EXPR` or `FIELD_EXPR` chain of capitalized names.
    STRUCT_LIT,
    /// `name : expr` in a struct literal.
    FIELD_INIT,
    /// `[async] fn PARAM_LIST? RETURN_TYPE? BLOCK end`.
    FN_EXPR,
    /// A short function: `BLOCK_PARAMS RETURN_TYPE? -> BLOCK`
    /// or `BLOCK_PARAMS RETURN_TYPE? -> do BLOCK end`. Its parameters are a
    /// `BLOCK_PARAMS` in every spelling (`|a, b|`, `||`, and the bare `PARAM`
    /// of `x -> …`), and its body is a `BLOCK` in both forms: for `-> expr`
    /// one holding a single `EXPR_STMT`, the expression, which is the
    /// block's tail value. So the resolver, the analyzer and lowering take
    /// it exactly as the `FN_EXPR` `fn(params) expr end`, while the formatter
    /// and the language server can tell the spellings apart.
    LAMBDA_EXPR,
    /// `spawn CALL_EXPR`, `spawn COMMAND_CALL` or `spawn do BLOCK end`.
    SPAWN_EXPR,
    /// `on QUAL_NAME [as STRING] [async] do BLOCK_PARAMS? RETURN_TYPE? BLOCK
    /// end`.
    ON_EXPR,
    /// A Lua path with all its steps: `COLON`, the first name,
    /// then any number of `LUA_FIELD`, `LUA_INDEX`, `LUA_CALL` and
    /// `LUA_METHOD` steps in source order. The chain is flat, one node, and
    /// every step in it has Lua semantics: a step node is never an NL member
    /// or call. The expressions inside its steps (arguments, indexes,
    /// blocks) are ordinary NL nodes. Parentheses end a path:
    /// `(:f()).name` is a `FIELD_EXPR` over a `PAREN_EXPR` holding the
    /// `LUA_PATH`.
    LUA_PATH,
    /// `. name` step of a Lua path: a plain field read.
    LUA_FIELD,
    /// `[ expr ]` step of a Lua path.
    LUA_INDEX,
    /// A call step of a Lua path: `ARG_LIST`, `TRAILING_BLOCK`, or both. A
    /// trailing block after the first name or a `.name` step is a call with
    /// the block as its only argument.
    LUA_CALL,
    /// `: name ARG_LIST? TRAILING_BLOCK?` step of a Lua path: Lua's method
    /// call, with its arguments; with neither, a call without arguments.
    LUA_METHOD,
    /// A unary operator and its operand.
    PREFIX_EXPR,
    /// A binary operator: left operand, operator token, right operand.
    BIN_EXPR,
    /// `expr is type`.
    IS_EXPR,
    /// `expr as type`.
    AS_EXPR,
    /// `expr |> target`; the target is a `NAME_REF`, `FIELD_EXPR`,
    /// `AT_EXPR`, `LUA_PATH`, `METHOD_CALL` or `CALL_EXPR`.
    PIPE_EXPR,
    /// A call with parentheses and/or a trailing block: callee, `ARG_LIST?`,
    /// `TRAILING_BLOCK?`.
    CALL_EXPR,
    /// `( arg, … )`; each argument is an expression, a `SPREAD_ARG`, a
    /// `KEYED_ARG` or a `KEYED_SPREAD_ARG`.
    ARG_LIST,
    /// `* expr`.
    SPREAD_ARG,
    /// `name: expr`: a keyed argument. The key is the
    /// `LNAME` token.
    KEYED_ARG,
    /// `** expr`: a table passed as keyed arguments.
    KEYED_SPREAD_ARG,
    /// A call without parentheses: callee, `COMMAND_ARGS`, `TRAILING_BLOCK?`.
    COMMAND_CALL,
    /// The arguments of a command call, separated by `,`.
    COMMAND_ARGS,
    /// `expr [ expr ]`.
    INDEX_EXPR,
    /// `expr . word`.
    FIELD_EXPR,
    /// `receiver : name ARG_LIST? TRAILING_BLOCK?`.
    METHOD_CALL,
    /// `expr !`: the function itself, not called.
    BANG_EXPR,
    /// `[async] do BLOCK_PARAMS? RETURN_TYPE? BLOCK end`.
    TRAILING_BLOCK,
    /// `| PARAM, … |` or `||`; in a `LAMBDA_EXPR` also one `PARAM` without
    /// pipes (`x -> …`).
    BLOCK_PARAMS,

    // -- Types
    /// `QUAL_NAME TYPE_ARGS?`, or one of `Self`, `nil`, `any`, `unknown`,
    /// `never`.
    NAMED_TYPE,
    /// `< type, … >`.
    TYPE_ARGS,
    /// `Function FN_TYPE_PARAMS RETURN_TYPE?`.
    FN_TYPE,
    /// `( FN_TYPE_PARAM, … )`.
    FN_TYPE_PARAMS,
    /// `[name :] type`, `* name : type` or `...`.
    FN_TYPE_PARAM,
    /// `type | type | …`.
    UNION_TYPE,
    /// `~ type`.
    NOT_TYPE,
    /// `type ?`.
    OPTIONAL_TYPE,
    /// `( type )`.
    PAREN_TYPE,
    /// `( type , type , … )`: several return values; only as a return type.
    TUPLE_TYPE,
    /// `{ RECORD_FIELD, … }`.
    RECORD_TYPE,
    /// `word : type` or `STRING : type`.
    RECORD_FIELD,
    /// `{ [ type ] : type }`.
    MAP_TYPE,
    /// `[ type ]`.
    ARRAY_TYPE,

    // -- Patterns
    /// A binding: `NAME` (an `LNAME`; a `CNAME` only as a `const` binding).
    BIND_PAT,
    /// `[-] NUMBER`, `STRING`, `true`, `false`, `nil`.
    LITERAL_PAT,
    /// `LITERAL_PAT (.. | ...) LITERAL_PAT`.
    RANGE_PAT,
    /// `is type`.
    IS_PAT,
    /// `{ item, … }`: `PAT_FIELD`s (keyed) or patterns (positional).
    BRACE_PAT,
    /// A keyed item `key : value` (the value is a pattern or, after a
    /// `key:`, a type node), or the shorthand `NAME` with no colon. A
    /// `QUESTION` after the key or the name marks an optional field
    /// (`{count?}`, `{pos?: {x, y}}`), which matches when it is `nil`.
    PAT_FIELD,
    /// `[ pattern, …, REST_PAT? ]`.
    BRACKET_PAT,
    /// `* [NAME]`.
    REST_PAT,
    /// `QUAL_NAME { PAT_FIELD, … }`.
    STRUCT_PAT,
    /// A dotted capitalized name in a pattern (`Direction.North`), resolved
    /// later: `QUAL_NAME`.
    QUALIFIED_PAT,
    /// `pattern as NAME`.
    AS_PAT,
    /// `pattern : type`.
    TYPED_PAT,

    // -- Declaration files. The root is a `SOURCE_FILE` holding
    // the items directly (no `BLOCK`), then `EOF`.
    /// `declare module NAME LUA_ROOT? members end`, or nested in a
    /// module `module NAME LUA_KEY? members end`.
    DECL_MODULE,
    /// `declare struct NAME LUA_ROOT? members end`: a Lua class; nested,
    /// `struct NAME LUA_KEY? …`.
    DECL_STRUCT,
    /// `[declare] type NAME STRUCT_PARENT? LUA_KEY? members end`: a host
    /// type (the key only when nested).
    DECL_TYPE,
    /// `declare fn NAME TYPE_PARAMS? PARAM_LIST? RETURN_TYPE? LUA_ROOT?`, or
    /// a member `fn (NAME | OP_NAME) … LUA_KEY?`.
    DECL_FN,
    /// `declare (let | const) NAME : type LUA_ROOT?`, or a member
    /// `const NAME : type LUA_KEY?`.
    DECL_VALUE,
    /// A field member: `NAME : type LUA_KEY?`.
    DECL_FIELD,
    /// `self` as the first parameter of a member `fn`: a method.
    SELF_PARAM,
    /// `= LUA_PATH`: the Lua value a declaration stands for.
    LUA_ROOT,
    /// `as STRING`: the Lua key of a member.
    LUA_KEY,
}

impl SyntaxKind {
    /// Converts a raw `u16` (from `rowan`) back to a kind; `None` when out
    /// of range.
    pub fn from_raw(raw: u16) -> Option<SyntaxKind> {
        Self::ALL.get(usize::from(raw)).copied()
    }

    /// The raw `u16` value.
    pub const fn to_raw(self) -> u16 {
        self as u16
    }

    /// Whitespace, line breaks and comments. Trivia carry no layout flags of
    /// their own; their effect is in the flags of the next non-trivia token.
    pub const fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::WHITESPACE
                | SyntaxKind::NEWLINE
                | SyntaxKind::COMMENT
                | SyntaxKind::DOC_COMMENT
        )
    }

    /// A token kind (as opposed to a node kind added later by the parser).
    pub fn is_token(self) -> bool {
        self <= SyntaxKind::EOF
    }

    /// One of the reserved keywords (not `from`, `step`).
    pub fn is_keyword(self) -> bool {
        (SyntaxKind::AND_KW..=SyntaxKind::WHILE_KW).contains(&self)
    }

    /// `LNAME`, `CNAME` or `ONAME`.
    pub const fn is_name(self) -> bool {
        matches!(self, SyntaxKind::LNAME | SyntaxKind::CNAME | SyntaxKind::ONAME)
    }

    /// `STRING`, `STR_BEGIN`, `STR_MID` or `STR_END`.
    pub const fn is_string_part(self) -> bool {
        matches!(
            self,
            SyntaxKind::STRING | SyntaxKind::STR_BEGIN | SyntaxKind::STR_MID | SyntaxKind::STR_END
        )
    }

    /// One of the compound-assignment operators (`+=` … `&&=`, `??=`).
    pub fn is_compound_assign(self) -> bool {
        (SyntaxKind::PLUS_EQ..=SyntaxKind::QUESTION2_EQ).contains(&self)
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        rowan::SyntaxKind(kind as u16)
    }
}

impl TryFrom<rowan::SyntaxKind> for SyntaxKind {
    type Error = rowan::SyntaxKind;

    fn try_from(raw: rowan::SyntaxKind) -> Result<Self, Self::Error> {
        SyntaxKind::from_raw(raw.0).ok_or(raw)
    }
}

/// The `rowan` language of NL, for `rowan::SyntaxNode<NlLanguage>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NlLanguage {}

impl rowan::Language for NlLanguage {
    type Kind = SyntaxKind;

    /// Unknown raw values (which only a foreign tree could contain) map to
    /// `ERROR` rather than panicking.
    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        SyntaxKind::from_raw(raw.0).unwrap_or(SyntaxKind::ERROR)
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        kind.into()
    }
}

/// The keyword kind of a word, `None` for every other word (including the
/// contextual keywords `from` and `step`).
pub fn keyword_kind(word: &str) -> Option<SyntaxKind> {
    use SyntaxKind::*;
    Some(match word {
        "and" => AND_KW,
        "as" => AS_KW,
        "async" => ASYNC_KW,
        "break" => BREAK_KW,
        "case" => CASE_KW,
        "catch" => CATCH_KW,
        "cond" => COND_KW,
        "const" => CONST_KW,
        "continue" => CONTINUE_KW,
        "declare" => DECLARE_KW,
        "do" => DO_KW,
        "else" => ELSE_KW,
        "elseif" => ELSEIF_KW,
        "end" => END_KW,
        "enum" => ENUM_KW,
        "false" => FALSE_KW,
        "finally" => FINALLY_KW,
        "fn" => FN_KW,
        "for" => FOR_KW,
        "global" => GLOBAL_KW,
        "if" => IF_KW,
        "import" => IMPORT_KW,
        "in" => IN_KW,
        "interface" => INTERFACE_KW,
        "is" => IS_KW,
        "let" => LET_KW,
        "loop" => LOOP_KW,
        "module" => MODULE_KW,
        "nil" => NIL_KW,
        "not" => NOT_KW,
        "on" => ON_KW,
        "or" => OR_KW,
        "pub" => PUB_KW,
        "return" => RETURN_KW,
        "self" => SELF_KW,
        "Self" => SELF_TYPE_KW,
        "spawn" => SPAWN_KW,
        "struct" => STRUCT_KW,
        "true" => TRUE_KW,
        "try" => TRY_KW,
        "type" => TYPE_KW,
        "unless" => UNLESS_KW,
        "until" => UNTIL_KW,
        "when" => WHEN_KW,
        "while" => WHILE_KW,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_round_trip() {
        for (i, &k) in SyntaxKind::ALL.iter().enumerate() {
            assert_eq!(k as usize, i);
            assert_eq!(SyntaxKind::from_raw(k.to_raw()), Some(k));
            let raw: rowan::SyntaxKind = k.into();
            assert_eq!(SyntaxKind::try_from(raw), Ok(k));
            assert_eq!(<NlLanguage as rowan::Language>::kind_from_raw(raw), k);
        }
        assert_eq!(SyntaxKind::from_raw(u16::MAX), None);
    }

    #[test]
    fn keywords_round_trip() {
        let mut count = 0;
        for &k in SyntaxKind::ALL {
            if k.is_keyword() {
                count += 1;
                let text = k.fixed_text().unwrap();
                assert_eq!(keyword_kind(text), Some(k));
            }
        }
        assert_eq!(count, 45);
        assert_eq!(keyword_kind("from"), None);
        assert_eq!(keyword_kind("step"), None);
        assert_eq!(keyword_kind("then"), None);
    }

    #[test]
    fn classes() {
        assert!(SyntaxKind::COMMENT.is_trivia());
        assert!(!SyntaxKind::ERROR.is_trivia());
        assert!(SyntaxKind::EOF.is_token());
        assert!(SyntaxKind::AMP2_EQ.is_compound_assign());
        assert!(SyntaxKind::PLUS_EQ.is_compound_assign());
        assert!(SyntaxKind::QUESTION2_EQ.is_compound_assign());
        assert!(!SyntaxKind::QUESTION2.is_compound_assign());
        assert!(!SyntaxKind::EQ.is_compound_assign());
        assert!(!SyntaxKind::LNAME.is_keyword());
    }
}
