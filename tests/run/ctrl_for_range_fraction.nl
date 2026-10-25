# For v in a range value with a fractional start: the numbers are the ones
# `for i = first, last` gives, 0.1 itself first, not 0.1 - 1 + 1
fn items(r)
  let out = []
  for v in r
    Table.insert(out, "#{v}:#{v == 0.1}")
  end
  Table.concat(out, " ")
end

print items(0.1..2)
print items(0.1...2.1)
print items(-0.5..1)
print items(1.5...1.5)

let r: any = 0.1..3
let hits = 0
for v in r
  hits += 1 if v == 1.1
end
print hits

let seen = []
for v in 0.1..2
  Table.insert(seen, tostring(v == 0.1))
end
print Table.concat(seen, " ")
