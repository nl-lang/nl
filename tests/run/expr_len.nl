# .len is Lua's # (bytes of a string, border of a table)
let s = "héllo"
print s.len
print s.length
let t = [1, 2, 3]
print t.len
print "".len
let keyed = {"len": 7}
print keyed["len"]
print keyed.len
let n = []
n[n.len + 1] = "a"
n[n.len + 1] = "b"
print n.len
