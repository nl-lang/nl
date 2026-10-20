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

# The rest table holds the count in `n`, as Table.pack does; a spread of
# any table that has a numeric `n` uses it.
fn rest_of(*args)
  args
end
let packed: any = rest_of("a", nil)
print packed.n, count(*packed)
print count(*Table.pack(nil, nil, nil))
print count(*[1, 2])

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
