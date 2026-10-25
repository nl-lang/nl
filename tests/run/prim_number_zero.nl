# round and truncate never give a negative zero: a result of 0 is the
# canonical 0 (as `//`), in the interpreter and in a trace alike
fn parts(x)
  "#{x.round} #{x.truncate} #{x.round(2)} #{x.round(-1)} #{x.truncate(1)}"
end

print parts(-0.4)
print parts(-0.001)
print parts(-4)
print parts(-0.0)
print parts(0.4)
print 1 / (-0.4).round, 1 / (-0.4).truncate, 1 / (-0.004).round(2)

let seen = []
for i in 1..200
  let s = tostring((-0.4).round) <> " " <> tostring((-0.3).truncate)
  Table.insert(seen, s) if i % 50 == 0
end
print Table.concat(seen, ",")
