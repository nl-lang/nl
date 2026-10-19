# `s:m(args)` in an NL file finds `m` in NL's
# String library when the receiver is a string, as `s.m` does;
# for any other receiver it is Lua's method call.
let s = "héllo"
print s:upcase()
print s:sub("l", "L")
print s:center(9, "*")
try
  s:upper()
catch e
  print "s:upper(): #{e.message}"
end
try
  s:sub(1, 2)
catch e
  print "s:sub(1, 2): #{typeof(e)} #{e.message}"
end
# A Lua path that indexes a string value gets NL's library too; one that
# names the `string` table gets Lua's.
print :s:reverse()
print :string.upper("abc"), :string.sub(s, 1, 3)
fn String.shout
  self.upcase <> "!"
end
print s:shout()

# Other receivers: Lua's method call, the receiver passed first, in any
# naming style, evaluated once.
let calls = {n: 0}
let obj = {v: 41}
obj["getValue"] = fn(this, d) this.v + d end
obj["end"] = fn(this) "end of #{this.v}" end
fn make
  calls.n += 1
  obj
end
print make():getValue(1), make():end(), calls.n
let n = 2.5
print n:floor()
IO.stdout:write("written through a file handle\n")
let nothing = nil
try
  nothing:upcase()
catch e
  print "nil receiver: #{e.message}"
end
try
  obj:missing()
catch e
  print "missing method: #{e.message}"
end
