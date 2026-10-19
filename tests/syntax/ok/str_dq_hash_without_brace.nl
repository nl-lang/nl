# A `#` not followed by `{` is an ordinary character; `\#{`
# writes a literal `#{`.
a = "a # b"
b = "\#{not interpolated}"
