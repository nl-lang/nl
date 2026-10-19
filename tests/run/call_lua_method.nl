# `x:m(a)` is Lua's method call; the method name may be in any style
let obj = {n: 5}
obj["getN"] = fn(this) this.n end
obj.add = fn(this, k) this.n + k end
print obj:getN()
print obj:add(1)
print obj.add(obj, 2)

struct Box
  v: Number
end

fn Box.plus(k)
  @v + k
end

let b = Box { v: 10 }
print b:plus(5)
print b.plus(5)
