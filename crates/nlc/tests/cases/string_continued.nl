# A "…" string continued with a backslash has the value
# of the heredoc `nlc fmt --fix` rewrites it to: one `\n` per continuation,
# and the next line's indentation as content (crates/nlc/tests/fmt.rs runs
# this program before and after the rewrite).
let plain = "aaa\
bbb\
ccc"
print plain
print plain.len
let name = "fox"
let interp = "hello #{name}\
  and #{name.upcase}\
bye"
print interp
print interp.len
let tail = "one\
two".len
print tail
print("x\
y" == "x\ny")
