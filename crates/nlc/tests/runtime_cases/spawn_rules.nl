let anon: any = fn() end  # Tasks review: the `spawn` stand-in.
module M
  fn helper(v, w)
    w
  end
  fn bare
    spawn helper(1, anon)
  end
end

# Finding 8: both forms of a call number the written arguments alike.
try
  M.bare
catch e
  print "bare name: #{e.message}"
end
try
  spawn M.helper(1, anon)
catch e
  print "qualified: #{e.message}"
end
print "bare name with sendable arguments: #{M.helper(1, 2)}"

# Finding 9: the line that started a job is that of the `spawn`, also when
# the `spawn` is a function's last expression.
fn boom(x)
  raise "boom #{x}"
end
fn starter(x)
  spawn boom(x)
end

# Design concern: a very long or very deep structure is copied without
# overflowing the stack, and keeps its shape.
fn count(l)
  let n = 0
  while l != nil
    n += 1
    l = l.next
  end
  n
end
fn depth(t)
  let n = 0
  while t.inner != nil
    n += 1
    t = t.inner
  end
  n
end
fn same(a, b) a == b end

async fn main
  try
    starter(1).await
  catch e
    let lines = e.trace.split("\n")
    print lines[lines.len]
    # `raise` as a function's last statement keeps its line in the trace.
    print e.trace.index("spawn_rules.nl:27:") != nil
  end
  let list = nil
  for i in 1..100000
    list = {v: i, next: list}
  end
  print "long list: #{spawn count(list).await}"
  let root = {}
  let t = root
  for i in 1..100000
    t.inner = {}
    t = t.inner
  end
  print "deep nesting: #{spawn depth(root).await}"
  let shared = {}
  let cyc = {a: shared, b: shared}
  cyc.self = cyc
  let c = spawn ident(cyc).await
  print "shape kept: #{c.a == c.b} #{c.self == c} #{c != cyc}"
  t.inner = {f: fn() end}
  try
    spawn depth(root)
  catch e
    print "#{typeof(e)}: #{e.message.sub(".inner".rep(100000), "<.inner x 100000>")}"
  end
end
fn ident(v) v end
main()
