# Table's functions check their arguments; on a value of no known type, `x.map(f)` is a member call, which finds nothing
let not_a_table: any = 5
let not_a_function: any = "f"

try
  Table.map(not_a_table, x -> x)
catch err: TypeError
  print "map of a number: #{typeof(err)}"
end

try
  Table.filter([1, 2], not_a_function)
catch err: TypeError
  print "filter with a string: #{typeof(err)}"
end

try
  Table.take([1, 2], 1.5)
catch err: TypeError
  print "take of 1.5: #{typeof(err)}"
end

try
  Table.zip([1], not_a_table)
catch err: TypeError
  print "zip with a number: #{typeof(err)}"
end

# a callable struct is a function to Table
struct Doubler
  factor: Number
end

fn Doubler.()(x)
  x * @factor
end

let triple: any = Doubler { factor: 3 }
print Table.map([1, 2], triple).join(",")

# the lint `table-method-untyped` warns about this call: at runtime a plain table has no `map`
let untyped: any = [1, 2, 3]
try
  untyped.map(x -> x * 2)
  print "not reached"
catch err
  print "untyped.map: #{typeof(err)}"
end
print Table.map(untyped, x -> x * 2).join(","), (untyped |> Table.map(x -> x + 1)).len
