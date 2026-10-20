# On a value typed as a list, a map or Table, the functions of Table are methods, resolved from the type
let xs: [Number] = [3, 1, 2]
print xs.map(x -> x * 2).filter(x -> x > 2).join(",")
print xs.sorted.reverse.join(","), xs.join(",")

# read without arguments, a method is called, as `s.upcase` is
print xs.first, xs.last, xs.sum, xs.max, xs.size
print xs.first(2).len, xs.last(1)[1]

# commands and blocks pass their arguments as in any call
xs.push 4, 5
xs.each do |x, i|
  print "#{i}:#{x}"
end
let total = xs.reduce(0) do |acc, x|
  acc + x
end
print total

let ages: {[String]: Number} = {ann: 30, bob: 25}
print ages.sorted_keys.join(","), ages.values.sum, ages.has_key("ann")
print ages.map_values(|_, a| -> a + 1).get("bob")

# a declared Table, and Lua's functions through it
let t: Table = [5, 4]
t.insert(6)
print t.concat("-"), t.size, t.is_empty

# a literal, the result of a declared function and a local list are typed too
print([1, 2, 2, 3].uniq.join(","), "a,b".split(",").reverse.join(","), [].len)

# `!` gives the function itself; `?.` skips the call on nil
let sum = xs.sum!
print sum == Table.sum, sum(xs)
let maybe: [Number]? = nil
print maybe?.first, maybe?.map(x -> x).len

# a key with the name of a Table function is read with brackets
let names: {[String]: String} = {first: "Ada", last: "Lovelace"}
print names["first"], names["last"], names.size

# a member assignment stores the key, and a compound one reads it too
let counters: Table = {}
counters.count = 1
counters.count += 1
print counters["count"], counters.count

# a record's members are its fields, also a field set later
let hooks = {}
hooks.reject = fn(why) "rejected: #{why}" end
print hooks.reject("no")

# options passed by name are read by name
fn spawn_wave(**opts)
  "#{opts.count} of #{opts.max ?? 10}"
end
print spawn_wave(count: 3, max: 5), spawn_wave(count: 2)
