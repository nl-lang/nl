# Number to text in a base (to_s, to_hex) and counting loops (times, up_to, down_to), also from Lua
print 255.to_hex, 255.to_hex(4), (-255).to_hex(4), 0.to_hex(2), 4096.to_hex(2)
let big = 2 ** 40
print 10.to_s(2), 35.to_s(36), (-8).to_s(8), 0.to_s(2), big.to_s(16)
print 3.5.to_s, 3.to_s, 1e20.to_s

try
  3.5.to_s(2)
catch err: TypeError
  print "3.5 in base 2: TypeError"
end
try
  let base: any = 37
  3.to_s(base)
catch err: TypeError
  print "base 37: TypeError"
end

let seen = []
3.times do |i|
  Table.insert(seen, i)
end
0.times do |i|
  Table.insert(seen, "never")
end
2.up_to(4, i -> Table.insert(seen, i * 10))
4.down_to(2, i -> Table.insert(seen, -i))
5.up_to(4, i -> Table.insert(seen, "never"))
print Table.concat(seen, " ")

# Lua code calls them as methods of numbers
let f = :loadstring("local x = 7; return x:clamp(0, 5), (2.5):round(), (255):to_hex(4), x:odd()")
print f()
