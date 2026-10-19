# checked-only
# backends: lua,bc
# A struct test reads the raw metatable, in the inline test
# of generated code as in the runtime's (`is`, `rt.check` behind a failed
# inline test): with a `__metatable` field on the struct, its instances
# are still instances, and a table whose metatable names the struct in
# `__metatable` is not one (runtime_cases/locked_metatable.nl).
struct S
  x: Number
end
struct Box
  s: S?
end

fn try_it(name, a, f)
  try
    let r = f(a)
    print "#{name}: ok #{r}"
  catch e
    print "#{name}: #{typeof(e)}: #{e.message}"
  end
end

fn take(s: S) "taken"
end
fn take_opt(s: S | Number) "taken"
end

let real = S { x: 1 }
:rawset(S, "__metatable", "locked")
let spoof = setmetatable({}, {__metatable: S})

for pair in [["real", real], ["spoof", spoof]]
  let name, v = pair[1], pair[2]
  print "#{name} is S: #{v is S}"
  try_it("#{name} param", v, fn(a) take(a) end)
  try_it("#{name} union param", v, fn(a) take_opt(a) end)
  try_it("#{name} let", v, fn(a)
    let t: S = a
    "stored"
  end)
  try_it("#{name} field", v, fn(a) typeof(Box { s: a }) end)
end
