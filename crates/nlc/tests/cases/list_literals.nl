# `[1, 2]` is a list and `{k: v}` a map; both
# are plain tables. A spread inside a list, nil items, and the length of a
# literal, which counts the items written when the last one is not nil.
let a = [1, 2, 3]
let b = []
let m = {x: 1, "two": 2, 3: 3}
print typeof(a), typeof(b), typeof(m)
print a.len, b.len, a[1], m.x, m["two"], m[3]
print([1, nil, 3].len)
print([nil, nil, 3].len)
print([1, 2, nil].len)
let n = nil
print([1, n, 3].len)
let s = [*a, 4]
print s.len, s[4]
let both = [*a, *a]
print both.len, both[6]
let mid = [0, *a, 9]
print mid.len, mid[1], mid[2], mid[5]
let holes = [*[1, nil], 3]
print holes.len
fn pair()
  return 7, 8
end
print([pair(), pair()].len)
print([[1], [2, 3]][2][2])
let nested = {kids: [[1, 2], [3]]}
print nested.kids[2][1]
print b == b, [] == []
let [x, y] = [10, 20]
print x + y
let [first, *rest] = [1, 2, 3]
print first, rest.len
case [1, 2]
when [p, q]
  print "pattern #{p} #{q}"
end
fn takes(list)
  list.len
end
print takes([1, 2, 3])
try
  let bad = [*5]
catch e
  print e.message
end
