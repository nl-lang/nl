# checked-only
# backends: lua,bc
# Inline checks: parameters, keyed arguments, returns
# and annotated variables are tested in the generated code, and the
# runtime is called only when a test fails; the messages are the
# runtime's. The calls with keyed arguments go through `any` to reach the
# runtime checks (the analyzer reports them when it knows the callee).
struct P
  x: Number
end
struct Q : P
  y: Number = 0
end
enum Dir
  N = 1
  S = 2
end

fn try_it(name, a, f)
  try
    let r = f(a)
    print "#{name}: ok #{r}"
  catch e
    print "#{name}: #{typeof(e)}: #{e.message}"
  end
end

fn erase(x): any x end
fn num(a: Number, b: String) "#{a}#{b}" end
fn mixed(a: any, b: Number, c: any = print("default c")) "#{a}#{b}#{c}" end
fn st(p: P) p.x end
fn en(d: Dir) d end
fn opt(a: Number?, b: Number | String) "#{a}#{b}" end
fn tbl(t: [Number], f: Function) t.len end
fn withrest(a: Number, *rest: Table) rest.len end
fn withopts(a: Number, **opts: Table) opts.k end
fn ret1(v): Number v end
fn ret2(v): (Number, String?) return v, nil end
fn ret3(v): (Number, String) v end
fn many() return 1, "two", 3 end
fn ret4(): (Number, String) many() end
fn ret5(v): P? v end
fn ret6(v): Number
  return v if v
  7
end
fn defs(b: Number, a: any = print("default a")) "#{a}#{b}" end

try_it("num ok", "a", fn(v) num(1, v) end)
try_it("num bad a", "1", fn(v) num(v, "a") end)
try_it("num bad b", 2, fn(v) num(1, v) end)
try_it("num kw", 2, fn(v) erase(num!)(1, k: v) end)
try_it("num kw first", 2, fn(v) erase(num!)(k: v) end)
try_it("num bad+kw", "x", fn(v) erase(num!)(v, k: 2) end)
try_it("mixed kw", 1, fn(v) erase(mixed!)(v, k: 1) end)
try_it("mixed kw2", 2, fn(v) erase(mixed!)(1, v, k: 1) end)
try_it("mixed ok", 2, fn(v) mixed(1, v) end)
try_it("mixed bad", "2", fn(v) mixed(1, v) end)
try_it("defs kw", 1, fn(v) erase(defs!)(v, k: 1) end)
try_it("defs bad", "s", fn(v) defs(v) end)
try_it("st ok", 1, fn(v) st(P { x: v }) end)
try_it("st child", 2, fn(v) st(Q { x: v }) end)
try_it("st bad", {x: 1}, fn(v) st(v) end)
try_it("st nil", nil, fn(v) st(v) end)
try_it("en ok", 2, fn(v) en(v) end)
try_it("en bad", 3, fn(v) en(v) end)
try_it("en nil", nil, fn(v) en(v) end)
try_it("opt ok", "s", fn(v) opt(nil, v) end)
try_it("opt bad", true, fn(v) opt(1, v) end)
try_it("opt bad a", "1", fn(v) opt(v, 1) end)
try_it("tbl ok", [1, 2], fn(v) tbl(v, print) end)
try_it("tbl kw", [1], fn(v) erase(tbl!)(v, k: 1) end)
try_it("tbl kw2", [1], fn(v) erase(tbl!)(k: 1) end)
try_it("tbl bad", 1, fn(v) tbl(v, print) end)
try_it("rest", 1, fn(v) withrest(v, 2, 3) end)
try_it("rest kw", 1, fn(v) erase(withrest!)(v, 2, k: 3) end)
try_it("rest bad", "1", fn(v) erase(withrest!)(v, 2, k: 3) end)
try_it("opts", 1, fn(v) withopts(v, k: 3) end)
try_it("ret1", 1, fn(v) ret1(v) end)
try_it("ret1 bad", "1", fn(v) ret1(v) end)
try_it("ret2", 1, fn(v) ret2(v) end)
try_it("ret2 bad", "x", fn(v) ret2(v) end)
try_it("ret3 bad", 1, fn(v) ret3(v) end)
try_it("ret4", nil, fn(v)
  let a, b, c = ret4()
  "#{a} #{b} #{c}"
end)
try_it("ret5", 1, fn(v) ret5(Q { x: v }).x end)
try_it("ret5 bad", 5, fn(v) ret5(v) end)
try_it("ret6", false, fn(v) ret6(v) end)
try_it("ret6 bad", "s", fn(v) ret6(v) end)
let f = fn(x: Number) x * 2 end
try_it("anon", 2, fn(v) f(v) end)
try_it("anon bad", "2", fn(v) f(v) end)
try_it("anon kw", 1, fn(v) erase(f)(k: v) end)
try_it("let bad", 5, fn(v)
  let z: String = v
  z
end)
try_it("let opt", nil, fn(v)
  let z: Number? = v
  z
end)
# A marked table that is not the last argument is refused too.
let marked = :keyed({a: 1})
try_it("kw not last", nil, fn(v) erase(num!)(marked, "x") end)
try_it("kw middle", nil, fn(v) erase(mixed!)(1, marked, 3) end)
