# In NL code the left operand of `==`
# decides. Its type's `==` runs whatever the right operand's type, `!=`
# is its negation, values of different types are unequal without a call,
# and a comparison with a literal is Lua's.
struct A
  n: Number
end
struct B
  n: Number
end
struct C
  n: Number
end
struct Kid : A
end
fn A.==(r)
  print "A.== with #{typeof(r)}"
  r.n == @n
end
fn B.==(r)
  print "B.== with #{typeof(r)}"
  false
end
let a = A { n: 1 }
print(a == B { n: 1 })
print(a == C { n: 1 })
print(a == {n: 1})
print(a != {n: 2})
print(Kid { n: 3 } == C { n: 3 })
print(C { n: 1 } == a)
print({n: 1} == a)
print(B { n: 1 } == a)
# Different Lua types: unequal without a call.
let s: any = "x"
print(a == s)
print(a != 5)
print(a == nil)
# A value is equal to itself.
print(a == a)
# A `when` value is compared with the subject on the left.
let plain = {n: 1}
case a
when (plain)
  print "case: matched"
else
  print "case: no match"
end
# A metamethod written in Lua keeps Lua's rule.
let mt = {__eq: fn(x, y) true end}
let l1 = :setmetatable({}, mt)
let l2 = :setmetatable({}, mt)
print(l1 == l2)
print(l1 == {})
