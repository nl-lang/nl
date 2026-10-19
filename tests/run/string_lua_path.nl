# Lua's string functions are reached through a Lua path and count bytes
let s = "héllo"
print :string.upper(s)
print :string.len(s)
print :string.sub("hello", 2, 3)
let h = "hello"
print h[2..3]
let first, last = :string.find("abc123", "%d+")
print "#{first} #{last}"
print :string.rep("ab", 2)
