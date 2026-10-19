# Parser snapshot tests

Snapshot tests of the NL lexer and parser (`crates/nl_syntax`), one construct or one rule per case.

## Layout

- `ok/<area>_<construct>.nl`: valid input. It must parse with no diagnostic (warnings included) and no `ERROR_NODE`.
- `err/<area>_<mistake>.nl`: invalid input. It must produce at least one diagnostic. Recovery cases are named `err/rec_*`.
- A case named `*.d.nl` is a declaration file and is parsed with `nl_syntax::parse_decl_file` (`decl_*` cases).
- Beside each `.nl` is a `.tree` of the same name: the expected output of `nl_syntax::dump_parse`, which is the tree with trivia, then a line `---` and the diagnostics when there are any. The format is described in `crates/nl_syntax/src/printer.rs`; `nlc parse <file>` prints the same thing.

Every `.nl` starts with a `#` comment that says what the case checks. A case for an ambiguity names the construct and the reading it checks (`postfix_paren_after_expression`, `if_modifier_or_expression`, `names_as_every_role`). Areas in use: `lex`, `names`, `num`, `str`, `heredoc`, `op`, `bang` (tokens and names), `nl` (line breaks and statement ends), `cmd`, `block`, `mod` (command calls, trailing blocks and modifiers), one prefix per production (`fn_decl`, `param`, `struct`, `iface`, `case`, `pat`, `type`, …), `later` (forms that parse and are rejected by a later pass), and `rec` (error recovery). The cases for the targeted errors, mistakes that get a message of their own, are named after the mistake (`then_after_if`, `tilde_equals`, `warning_lua_path_after_fn_header`).

The runner is `crates/nl_syntax/tests/syntax_snapshots.rs`. It also checks the round-trip property for every file here and under `tests/run/`: the tree's text is the source, and parsing it again gives the same tree and diagnostics.

## Running

```sh
cargo test -p nl_syntax --test syntax_snapshots
```

Every mismatch is reported with a line diff (`-` expected, `+` actual), along with every `.nl` without a `.tree` (and the reverse), every `ok/` case with a diagnostic and every `err/` case without one.

## Adding a case

1. Write the `.nl` file under `ok/` or `err/`, a few lines long, starting with its `#` comment.
2. Look at the tree the parser builds: `cargo run -p nl_syntax --example parse_dump -- --no-trivia tests/syntax/ok/<name>.nl`.
3. Decide whether it is the tree the grammar calls for. For an error case, check that the diagnostic is at the right place, says something a user can act on, is reported once, and that what follows the mistake parses normally. If the parser is wrong, fix the parser (or leave the case out and report it); do not snapshot the wrong behaviour.
4. Write the `.tree` with `UPDATE_SNAPSHOTS=1` (below) and read it.

## Updating snapshots

```sh
UPDATE_SNAPSHOTS=1 cargo test -p nl_syntax --test syntax_snapshots
```

This rewrites every `.tree` that differs, creates missing ones and deletes `.tree` files whose `.nl` is gone (add `-- --nocapture` to see the list). The other checks still apply. A snapshot locks in whatever the parser does, so review `git diff tests/syntax` before committing.
