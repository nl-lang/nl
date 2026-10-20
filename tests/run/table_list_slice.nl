# Table's slices: slice (positions, negatives, ranges, clamping), take, drop, first, last, each_slice, each_cons
let xs = ["a", "b", "c", "d", "e"]

print Table.slice(xs, 2, 4).join("")
print Table.slice(xs, 3).join("")
print Table.slice(xs, -2).join(""), Table.slice(xs, 1, -2).join("")
print Table.slice(xs, 2..3).join(""), Table.slice(xs, 2...4).join("")
print Table.slice(xs, 0, 10).join(""), Table.slice(xs, 4, 2).len

print Table.take(xs, 2).join(""), Table.take(xs, 9).join(""), Table.take(xs, 0).len
print Table.drop(xs, 3).join(""), Table.drop(xs, 9).len

let ns = [1, 3, 5, 6, 7, 9]
print Table.take_while(ns, n -> n % 2 == 1).join(",")
print Table.drop_while(ns, n -> n % 2 == 1).join(",")
print Table.take_while(ns, |_, i| -> i < 3).join(",")

# without a count, first and last are values
print Table.first(xs), Table.last(xs), Table.first([]), Table.last([])
print Table.first(xs, 2) |> Table.join(""), Table.last(xs, 2) |> Table.join(""), Table.last(xs, 9) |> Table.join("")

for s in Table.each_slice(ns, 4)
  print s.join(",")
end
print Table.chunks(ns, 2).len, Table.chunks([], 3).len

for w in Table.each_cons(ns, 3)
  print w.join(",")
end
print Table.each_cons([1, 2], 3).len

try
  Table.take(xs, -1)
catch err
  print typeof(err)
end
try
  Table.each_slice(xs, 0)
catch err
  print typeof(err)
end
