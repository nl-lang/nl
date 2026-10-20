# A copy of a rest table keeps its count; a list made from one is a plain list
fn count(*args)
  :select("#", *args)
end

fn copies(*args)
  let c = Table.copy(args)
  let d = Table.deep_copy(args)
  print count(*c), count(*d), count(*args)
  print select("#", Table.unpack(c)), Table.unpack(d)
  # [*args] is a list: its trailing nils are cut, as in a literal.
  let l = [*args]
  print count(*l), l.len
end
copies(1, nil, nil)
copies(nil)
copies(1, 2)
copies()

# Table.pack's count is kept the same way.
let p = Table.copy(Table.pack(nil, nil))
print count(*p), p.n

# A deep copy keeps the count of each rest table inside it.
fn rest(*args)
  args
end
let nested = Table.deep_copy({a: rest(1, nil), b: [rest(nil, nil, nil)]})
print count(*nested.a), count(*nested.b[1])
