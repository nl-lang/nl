# `a ?? b` is `a` unless it is nil, then `b`; `x ??= v` assigns only when `x` is nil
let seen = []
fn note(v)
  seen[seen.len + 1] = v
  v
end

let none = nil
print none ?? "default"
print 0 ?? 1
print "" ?? "empty"

# Only nil is replaced: `false` stays, where `or` would replace it.
print false ?? true
print false or true

# The left side is evaluated once and the right side only when needed.
print note("left") ?? note("right")
print seen.len
print note(nil) ?? note("fallback")
print seen.len
print seen[2]

# One value, as in any operand position.
fn two
  return nil, 2
end
print two() ?? "first was nil"

# Left associative, at the level of `or`: `a or b ?? c` is `(a or b) ?? c`.
print none ?? none ?? "third"
print false or nil ?? "after or"
print nil ?? false or "or after"

# With a declared type, the result is the non-nil part of the left side
# or the right side.
fn count(n: Number?): Number
  n ?? 0
end
print count(4) + 1
print count(nil) + 1

# The proposal's example: pick a function.
let obj = {paint: nil}
let cls = {paint: fn(o, list) "cls #{list.len}" end}
let paint = obj.paint! ?? cls.paint! ?? fn(o, list) "default" end
print paint(obj, [1, 2])

# Across lines: `??` continues the line before.
let picked = none
  ?? "next line"
print picked

# `??=` assigns when the target is nil; the target is evaluated once.
let a = nil
a ??= note("set")
a ??= note("not set")
print a
let flag = false
flag ??= true
print flag
let t = {}
fn key
  note("key")
  "k"
end
t[key()] ??= 1
t[key()] ??= 2
print t.k
print seen.len

# With a nil-safe target: nothing happens when the receiver is nil.
let box = {v: nil}
box?.v ??= "boxed"
none?.v ??= note("never")
print box.v
print seen.len
