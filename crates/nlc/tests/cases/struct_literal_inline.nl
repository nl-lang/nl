# checked-only
# backends: lua,bc
# Struct literals of a struct the compiler sees whole are built
# inline: the values in the order written, then field by field
# in declaration order (the parent's first) the default of an omitted or
# nil field and the check of a typed one, with the runtime's messages.
# The instance is the one `rt.new` builds: same metatable, same fields.
# Literals of an imported struct and in a `spawn do` block still go
# through `rt.new`.
import { Box, Mode } from 'lib/kinds'

let counter = 0
fn tick(tag)
  counter += 1
  print "tick #{tag} #{counter}"
  counter
end

struct Base
  id: Number
  label: String = "base"
  stamp: Number = tick("base.stamp")
end

struct Child : Base
  level: Number = 1
  extra: Number? = nil
  note = tick("child.note")
end

enum Color
  Red
  Green
end

struct Pixel
  c: Color
  p: Base?
  any_thing
end

fn Base.describe
  "#{@id}:#{@label}:#{@stamp}"
end

fn Base.make(n: Number): Self
  Self { id: n }
end

let b = Base { id: tick("id"), label: "L" }
print b.describe, typeof(b), b is Base
let c = Child { id: 7, level: tick("level") }
print c.describe, typeof(c), c is Base, c is Child, c.level, c.extra, c.note
print Base.make(4).describe

fn keys_of(o)
  let keys = []
  for k, v in o
    Table.insert(keys, "#{k}=#{v}")
  end
  Table.sort(keys)
  Table.concat(keys, ",")
end
print keys_of(Child { id: 1, label: "x", stamp: 2, level: 3, extra: 4, note: 5 })
print keys_of(Base { id: 1 })
print keys_of(Child { id: 1, stamp: 9 })

let px = Pixel { c: Color.Red, p: c }
print px.c, typeof(px.p)

fn try_it(name, a, f)
  try
    let r = f(a)
    print "#{name}: ok #{r}"
  catch e
    print "#{name}: #{typeof(e)}: #{e.message}"
  end
end

try_it("no id", nil, fn(v) Base { label: "no id" }.label end)
try_it("id str", "str", fn(v) Base { id: v }.id end)
try_it("id nil", nil, fn(v) Base { id: v } end)
try_it("label nil", nil, fn(v) Base { id: 1, label: v }.label end)
try_it("label num", 5, fn(v) Base { id: 1, label: v }.label end)
try_it("stamp nil", nil, fn(v) Base { id: 1, stamp: v }.stamp end)
try_it("stamp str", "s", fn(v) Base { id: tick("id2"), stamp: v }.stamp end)
try_it("extra str", "x", fn(v) Child { id: 1, extra: v } end)
try_it("extra nil", nil, fn(v) Child { id: 1, extra: v }.extra end)
try_it("level bad", false, fn(v) Child { id: 1, level: v } end)
try_it("color bad", "Blue", fn(v) Pixel { c: v } end)
try_it("color ok", "Green", fn(v) Pixel { c: v }.c end)
try_it("p bad", 5, fn(v) Pixel { c: Color.Green, p: v } end)
try_it("p child", c, fn(v) typeof(Pixel { c: Color.Green, p: v }.p) end)
try_it("make bad", "q", fn(v) Base.make(v) end)
try_it("both bad", "q", fn(v) Base { id: v, label: v } end)
print counter

fn takes_box(b: Box): Number b.v end
fn takes_mode(m: Mode) m end
try_it("box", 3, fn(v) takes_box(Box { v: v }) end)
try_it("box default", nil, fn(v) Box {}.v end)
try_it("box bad", 3, fn(v) takes_box(v) end)
try_it("box field bad", "s", fn(v) Box { v: v } end)
try_it("mode", "Hard", fn(v) takes_mode(v) end)
try_it("mode bad", "Medium", fn(v) takes_mode(v) end)

struct Outer
  a: Number = 1
end

fn make(n)
  struct Inner : Outer
    b: Number = n
    o: Outer? = nil
  end
  fn Inner.sum: Number
    @a + @b
  end
  let i = Inner { o: Outer {} }
  print typeof(i), i.sum, i is Outer, typeof(i.o)
  fn check_inner(x: Inner) x.b end
  try_it("inner bad", Outer {}, fn(v) check_inner(v) end)
  try_it("inner field bad", 5, fn(v) Inner { o: v } end)
  i
end
print make(5).sum
print make(6).sum

module Shapes
  struct Sq
    side: Number
  end
  fn Sq.area: Number
    @side * @side
  end
  fn Sq.make(s: Number): Self
    Self { side: s }
  end
  fn build(s) Sq { side: s } end
  fn mk(s) Sq.make(s) end
end
print Shapes.build(4).area, Shapes.mk(3).area
try_it("sq bad", "x", fn(v) Shapes.mk(v) end)
try_it("sq lit bad", "x", fn(v) Shapes.build(v) end)

struct Pt
  x: Number
  y: Number = 0
end
fn Pt.len2(o: Pt): Number
  @x * o.x + @y * o.y
end
let t = spawn do
  let p = Pt { x: 3 }
  fn local_f(q: Pt) q.x end
  "#{p.len2(p)} #{local_f(p)} #{typeof(p)}"
end
async fn show(task)
  print task.await
end
show(t)
fn spawned(n: Number) Pt { x: n }.x end
let t2 = spawn spawned(7)
show(t2)
