# byteslice: bytes i to j (inclusive), or from i to the end with one number, or a range
let s = "héllo"
print s.byteslice(1, 1)
print s.byteslice(4)
print s.byteslice(2, 3)
print s.byteslice(-3)
print s.byteslice(-3, -2)
print s.byteslice(4..5)
print s.byteslice(4...6)
print "[" <> s.byteslice(10) <> "]"
print "[" <> s.byteslice(3, 2) <> "]"
print s.byteslice(0)
print s.byteslice(2, 3).as_bytes.len
print s.byteslice(2, 2).valid_encoding
