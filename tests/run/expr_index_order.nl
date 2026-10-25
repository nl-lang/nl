# The table of `t[k]` is read before `k`: a local that the key's code
# assigns is pinned first, on every target.
let t: Table = {a: 1}
let u: Table = {a: 2}
fn swap_t
  t = u
  "a"
end
print t[swap_t()]
print t.a

let s = "xyz"
fn swap_s
  s = "abc"
  2
end
print s[swap_s()]
print s

let l = [1, 2]
let l2 = [3, 4]
fn swap_l
  l = l2
  1
end
print l[swap_l()]
print l[1]

let m = {k: {v: "old"}}
fn swap_m
  m = {k: {v: "new"}}
  "k"
end
print m[swap_m()].v
print m.k.v
