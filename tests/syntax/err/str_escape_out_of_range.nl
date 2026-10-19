# `\ddd` is at most 255, `\u{…}` at most 10FFFF, `\x` takes
# two hex digits.
a = "\256"
b = "\u{110000}"
c = "\x4"
d = "\u{}"
