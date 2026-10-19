# Finding 6: a Lua method call whose name is a Lua keyword passes the
# receiver, also through a Lua path and on a computed receiver.
let obj = {name: "o"}
obj["end"] = fn(me, x) "#{me.name} #{x}" end
print obj:end(1)
let r = :obj:end(2)
print r
let made = 0
fn mk
  made += 1
  obj
end
print mk():end(3), made
