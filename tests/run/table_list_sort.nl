# Table's sorting: sorted and sort_by make stable sorted copies; sort is Lua's, in place; sorted_keys orders mixed keys
let xs = [5, 2, 9, 1, 7]

print Table.sorted(xs).join(","), xs.join(",")
print Table.sorted(xs, |a, b| -> a > b).join(",")
print Table.sorted([]).len, Table.sorted(["pear", "apple", "fig"]).join(",")

# stable: equal keys keep their order
let people = [
  {name: "ann", age: 30},
  {name: "bob", age: 25},
  {name: "cid", age: 30},
  {name: "dee", age: 25},
  {name: "eve", age: 41},
]
let by_age = Table.sort_by(people, p -> p.age)
print Table.map(by_age, p -> p.name).join(",")
let by_age_desc = Table.sorted(people, |a, b| -> a.age > b.age)
print Table.map(by_age_desc, p -> p.name).join(",")

# a long list, sorted by a key with many ties, against a count of each key
let long = Table.map(Table.range(1, 40), i -> {key: i % 4, order: i})
let sorted = Table.sort_by(long, e -> e.key)
let stable = true
for i in 2..sorted.len
  let a = sorted[i - 1]
  let b = sorted[i]
  stable = false if a.key > b.key or (a.key == b.key and a.order > b.order)
end
print stable, sorted.len

# Lua's sort works in place and returns nothing
let ys = [3, 1, 2]
Table.sort(ys)
print ys.join(",")

let mixed = {2: "two", b: "bee", 1: "one", a: "ay"}
mixed[true] = "yes"
mixed[false] = "no"
print Table.map(Table.sorted_keys(mixed), k -> tostring(k)).join(" ")
print Table.sorted_keys({c: 1, a: 2, b: 3}, |x, y| -> x > y).join("")
Table.each_sorted({z: 26, y: 25, x: 24}, |k, v| -> print(k, v))
