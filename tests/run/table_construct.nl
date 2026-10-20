# Table's constructors: range, fill, from_iter, pack and unpack
print Table.range(1, 5).join(","), Table.range(0, 10, 5).join(","), Table.range(5, 1, -2).join(",")
print Table.range(3, 1).len, Table.range(0, 1, 0.25).join(" ")

print Table.fill(3, "x").join(""), Table.fill(0, 1).len
print Table.fill(4, i -> i * i).join(",")

# from_iter collects the first values of an iterator
let keys = Table.from_iter(pairs({b: 2, a: 1, c: 3}))
print Table.sorted(keys).join(",")
print Table.from_iter(ipairs(["p", "q", "r"])).join(",")
let n = 0
let countdown = fn
  n += 1
  if n <= 3
    4 - n
  else
    nil
  end
end
print Table.from_iter(countdown!).join(",")

# pack keeps the number of arguments, nils included; unpack honours it
let packed = Table.pack(1, nil, 3, nil)
print packed.n, packed[1], packed[3]
let a, b, c, d = Table.unpack(packed)
print a, b, c, d
print select("#", Table.unpack(packed)), select("#", Table.unpack([1, 2, 3], 2))
print Table.unpack([1, 2, 3], 2, 3)

try
  Table.range(1, 5, 0)
catch err
  print typeof(err)
end
