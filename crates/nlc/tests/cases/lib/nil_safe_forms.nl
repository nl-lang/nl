# The nil-safe operators lower to a local, a test and a jump
# (nl_ir lower/nilsafe.rs): no closure and no runtime call of their own.
# `nil_safe_lowering` in cases.rs builds this file and reads the Lua.
struct Point
  x: Number
end

fn chain(a, d)
  a?.b.c(d)?.e
end

fn field(p: Point?)
  p?.x
end

fn call(f, x)
  f?.(x)
end

fn method(o)
  o?.:m(1)
end

fn path()
  :game?.players?.[1]
end

fn coalesce(a, b)
  a ?? b
end

fn assign(a, v)
  a?.b = v
  a?.n ??= v
end

# `x.m?.(args)` calls the member its guard read, without reading it
# again: under the member-call rule at runtime, or as the declared type
# says.
fn Point.scaled(k)
  @x * k
end

fn held(t)
  t.m?.("x")
end

fn held_typed(p: Point)
  p.scaled?.(3)
end

print chain(nil, 1), field(Point { x: 2 }), call(nil, 3), method(nil), path(), coalesce(nil, 4)
print held({m: fn(s) s end}), held_typed(Point { x: 2 })
