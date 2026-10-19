# A field type that names a struct declared later in the file is
# not frozen at the first construction.
struct A
  b: B?
  c: Number | B
  d: ~B
end
print typeof(A { c: 1, d: 2 })
struct B
  x: Number = 1
end
let a = A { b: B {}, c: B {}, d: 3 }
print typeof(a.b), typeof(a.c)
let a_b: any = B {}
let five: any = 5
try
  let bad = A { c: 1, d: a_b }
catch e: TypeError
  print e.message
end
try
  let bad = A { b: five, c: 1, d: 2 }
catch e: TypeError
  print e.message
end
