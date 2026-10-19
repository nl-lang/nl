# `?.` tests the value before it; when that is nil, the rest of the chain is skipped and its value is nil
struct Point
  x: Number
  y: Number = 0
end

fn Point.sum
  @x + @y
end

fn Point.scaled(k)
  @x * k
end

fn Bool.label
  if self
    "yes"
  else
    "no"
  end
end

# Every argument that is evaluated passes through `note`.
let seen = []
fn note(v)
  seen[seen.len + 1] = v
  v
end

# Members, indexes and `len`.
let some = {name: "ann", inner: {deep: 5}, list: [10, 20]}
let none = nil
print some?.name
print none?.name
print some?.inner?.deep
print none?.inner.deep
print some?.missing?.deep
print some?.list?.[2]
print none?.[note(1)]
print some?.name?.len
print none?.len

# Calls: the function itself is tested, then called as it would be.
let handlers = {greet: fn(who) "hi #{who}" end}
print handlers.greet?.("bo")
print handlers.missing?.(note(2))
let f = nil
print f?.(note(3))
let run = fn(g: Function) g(5) end
print(run?.() do |x| x * 2 end)
let skip = nil
print(skip?.() do |x| x * 2 end)
fn tag(text: String, **opts: any): String
  "#{text}:#{opts.size}"
end
let tagger = tag!
print tagger?.("t", size: 4)

# Lua's method call on a receiver that may be nil.
let counter = {n: 5}
counter.add = fn(this, k) this.n + k end
print counter?.:add(1)
print none?.:add(note(4))

# Structs: with a declared type the member is resolved statically; a
# method without parameters is called, as after `.`.
fn first(p: Point?): Number?
  p?.x
end
fn total(p: Point?)
  p?.sum
end
print first(Point { x: 3 })
print first(nil)
print total(Point { x: 1, y: 2 })
print total(nil)
let loose: any = Point { x: 4 }
let gone: any = nil
print loose?.sum
print loose?.scaled(3)
print loose.scaled?.(5)
print loose.missing?.(note(11))
print gone?.scaled(note(5))

# A chain: `a?.b.c(d)` evaluates neither `.c` nor `d` when `a` is nil.
let full = {b: {c: fn(v) v + 1 end}}
let empty = nil
print full?.b.c(note(41))
print empty?.b.c(note(6))

# Lua paths.
let cfg = {inner: {v: 7}}
print :cfg?.inner?.v
print :empty?.inner.v
print :string?.upper("abc")
print :string.upper?.("abc")
print :string.nothing?.("abc")
print :counter?.:add(2)
print :empty?.:add(note(7))

# Postfix `!` after a nil-safe member: the function or nil.
print typeof(handlers?.greet!)
print none?.greet! == nil

# Command calls whose callee is nil-safe.
let speaker = {say: fn(a, b) print("say #{a} #{b}") end}
speaker?.say "a", "b"
none?.say note(8), "b"

# Assignment: nothing is stored, and the value is not evaluated, when the
# receiver is nil.
let box = {count: 1}
box?.count = note(10)
none?.count = note(9)
box?.count += 5
none?.count += note(9)
box?.["k"] = "set"
none?.["k"] = note(9)
:box?.label = "lua"
:empty?.label = note(9)
print box.count
print box.k
print box.label

# `false` is not nil: its members are read.
print false?.label

# Each tested value is read once: an `__index` that counts its reads sees
# one per line, also where the tested value is the member called.
let reads = {n: 0}
let counted = :setmetatable({}, {__index: fn(_, k)
  reads.n += 1
  if k == "w"
    {x: "deep"}
  else
    fn(*a) "called #{k} with #{a.len}" end
  end
end})
print counted.m?.("x")
print counted?.m?.("x", "y")
print :counted.w?.x
print reads.n

# A chain gives one value: a call at its end is truncated, as `(f())` is.
let pair = {two: fn() return 1, 2 end}
print pair.two()
print pair?.two()

print seen.len
print seen[1], seen[2], seen[3]
