# *args keeps the count of its arguments, nils included: args.len, f(*args), [*args]
fn count(*args)
  :select("#", *args)
end

fn show(*args)
  let parts = []
  for i in 1..args.len
    parts.push(tostring(args[i]))
  end
  "#{args.len}: #{parts.join(", ")}"
end

fn forward(*args)
  show(*args)
end

print show(1, nil, 3)
print forward(1, nil, 3)
print forward(nil, nil)
print forward()
print count(nil)

# The count goes with the rest table, outside it: the table holds only the
# arguments. Table.pack's count is kept the same way; a field `n` of a table
# NL did not count is only a key, and the table spreads by #t.
fn rest_of(*args)
  args
end
let packed: any = rest_of("a", nil)
print Table.size(packed), count(*packed)
print count(*Table.pack(nil, nil, nil))
print count(*[1, 2])
let made: any = {n: 3}
made[1] = "x"
print count(*made), Table.unpack(made)

# In a list.
fn listed(*args)
  let l = [0, *args, 4]
  l[5]
end
print listed(1, nil, 3)

# Keyed arguments are not counted among the rest.
fn opts(*args, **o)
  "#{args.len} #{o.k}"
end
print opts(1, nil, k: "v")

# A rest parameter that is assigned is a table like any other.
fn replaced(*args)
  args = [9, 8]
  args.len
end
print replaced(1, 2, 3)
