# split with a string separator, strip and padding
let parts = "a,b,c".split(",")
print parts.len
print Table.concat(parts, "|")
print "[" <> "  hi  ".strip <> "]"
print "[" <> "  hi  ".lstrip <> "]"
print "[" <> "  hi  ".rstrip <> "]"
print "[" <> "ab".center(6) <> "]"
print "[" <> "ab".ljust(5) <> "]"
print "[" <> "ab".rjust(5) <> "]"
print "[" <> "é".rjust(3) <> "]"
