# Lua code reaches NL's String methods through Lua's string table, and Lua's own names stay Lua's
fn String.shout
  self.upcase <> "!"
end

let r = include("include/string_methods.lua")
print r.shout
print r.shout_fn
print r.snake
print r.words
print r.length
print r.bytes
print r.upper
print r.reverse_bytes
print r.missing

# The same from a chunk loaded at run time
let f = :loadstring("return ('ünïcode'):upcase(), #('a,b,,'):split(','), string.format('%d', 5)")
print f()
