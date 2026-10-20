# Helper for the worker_ tests: named functions and structs that workers can load. No top-level effects.
pub fn square(x: Number): Number
  x * x
end

pub fn add(a, b = 10)
  a + b
end

pub fn answer
  42
end

pub fn identity(v)
  v
end

pub fn mutate(t)
  t.changed = true
  t.len
end

pub fn same_table(msg)
  msg.a == msg.b
end

pub fn has_cycle(t)
  t.again == t
end

pub fn describe(v)
  typeof(v)
end

pub fn range_sum(r)
  let total = 0
  for i in r
    total += i
  end
  total
end

pub fn apply_fn(f, x)
  f(x)
end

pub fn make_closure
  let f = fn(x) x end
  f!
end

pub async fn double_square(x)
  let s = spawn square(x).await
  s * 2
end

pub struct JobError : Exception
  reason: String
end

pub fn fail_with(reason)
  raise JobError { reason: reason }
end

pub struct Rect
  w: Number
  h: Number
end

fn Rect.area
  @w * @h
end

pub fn spread_count(t)
  :select("#", *t)
end

pub fn arg_count(*args)
  args.len
end
