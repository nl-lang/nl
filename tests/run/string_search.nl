# index, start_with, end_with (character positions)
let s = "héllo"
print s.index("l")
print s.index("l", 4)
print s.index("z") == nil
print s.index("é")
print s.start_with("hé")
print s.start_with("x")
print s.end_with("lo")
print s.end_with("h")
