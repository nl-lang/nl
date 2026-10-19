# In a `"…"` string a backslash directly before a line
# break continues the string on the next line and writes a line break; the
# string is one token, also after an interpolation.
let foo = "aaa \
bbb \
ccc"
let bar = "a#{x} \
  b"
print foo
