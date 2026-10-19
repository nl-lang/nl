# The forms of for ... in
for i in 1..3
  print i
end

let list = ["a", "b", nil, "d"]
for v in list
  print v
end

let map = {x: 1, y: 2, z: 3}
let keys = []
let sum = 0
for k, v in map
  Table.insert(keys, k)
  sum += v
end
Table.sort(keys)
print Table.concat(keys, ",")
print sum

for i, v in ipairs(["p", "q"])
  print "#{i}=#{v}"
end

fn triples(n)
  let i = 0
  let it = fn()
    i += 1
    return nil if i > n
    return i, i * 2, i * 3
  end
  it!
end

for a, b, c in triples(2)
  print "#{a} #{b} #{c}"
end

for ch in "héy"
  print ch
end

let units = [{name: "orc", hp: 5}, {name: "elf", hp: 7}]
for {name, hp} in units
  print "#{name} #{hp}"
end

let typed: [Number] = [4, 5]
for v in typed
  print v
end
