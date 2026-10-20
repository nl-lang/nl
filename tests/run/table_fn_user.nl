# `fn Table.name` adds a function to Table: a method of typed tables, and `Table.name(t)` anywhere
fn Table.second
  self[2]
end

# self is a Table, so the library's functions are its methods
fn Table.sum_of_squares
  self.map(x -> x * x).sum
end

fn Table.pluck(key: String): [any]
  self.map(row -> row[key])
end

fn Table.histogram: Table
  Self.tally(self)
end

let xs: [Number] = [1, 2, 3]
print xs.second, xs.sum_of_squares, Table.second(xs)
print Table.sum_of_squares([4]), [[9, 8]].first.len

let rows: [Table] = [{name: "a", n: 1}, {name: "b", n: 2}]
print rows.pluck("name").join(","), Table.pluck(rows, "n").sum
print xs |> Table.second, rows |> Table.pluck("n") |> Table.join("+")
let h = ["x", "y", "x"].histogram
print h.x, h.y

# an untyped value reaches it through Table
let any_list: any = [7, 8, 9]
print Table.second(any_list), any_list |> Table.second

# Lua code sees it in the global Table, next to Lua's table functions
print loadstring("return Table.second({5, 6}), Table.concat({1, 2}, '.')")()

# the first definition stays: another file cannot redefine it, nor a library function
print Table.second(xs)
try
  include("include/table_redefine.nl")
  print "not reached"
catch
  print "redefinition from another file rejected"
end
print Table.second(xs)
try
  load_nl("fn Table.map(f)\n  nil\nend\n", "redefine_map")()
  print "not reached"
catch
  print "redefinition of a library function rejected"
end
print xs.map(x -> x + 1).join(",")
