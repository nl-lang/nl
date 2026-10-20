# Table's list transforms: map, filter, reject, reduce, each, flatten, uniq, zip, compact, join
let xs: [Number] = [1, 2, 3, 4, 5]

print Table.map(xs, x -> x * 10).join(",")
print Table.map(xs, |x, i| -> "#{i}:#{x}").join(" ")
print Table.filter(xs, x -> x % 2 == 1).join(",")
print Table.select(xs, x -> x > 3).join(",")
print Table.reject(xs, x -> x % 2 == 1).join(",")

# reduce from an initial value, or from the first element
print Table.reduce(xs, 100, |acc, x| -> acc + x)
print Table.reduce(xs, |acc, x| -> acc * x)
print Table.fold(["a", "b", "c"], "", |acc, s, i| -> acc <> s <> i)
print Table.reduce([], |acc, x| -> acc + x)

# each returns its list
let seen = []
let same = Table.each(xs, |x, i| -> Table.push(seen, x * i))
print seen.join(","), same == xs
Table.each_with_index(["a", "b"], |s, i| -> print("#{i}=#{s}"))

# nested lists
let nested = [1, [2, [3, [4]]], {k: "map"}, []]
let flat = Table.flatten(nested)
print flat.len, flat[1], flat[4], typeof(flat[5])
print Table.flatten(nested, 1).len
print Table.flat_map([1, 2, 3], x -> [x, x * 100]).join(",")
print Table.flat_map([1, 2], x -> x + 1).join(",")

# uniq keeps the first of each value, or of each key
print Table.uniq([3, 1, 3, 2, 1]).join(",")
print Table.uniq(["apple", "avocado", "banana", "blueberry", "cherry"], s -> s[1]).join(",")

print Table.reverse(xs).join(",")
print xs.join(",")

let pairs = Table.zip([1, 2, 3], ["a", "b", "c"], [true, false])
for p in pairs
  print p[1], p[2], p[3]
end

# compact reads up to the highest index, past the holes
let holes = [1, nil, 2, nil, nil, 3]
print Table.compact(holes).join(",")

print Table.join([1, "two", true, nil == nil], "|")
print Table.join(["x", "y"])
