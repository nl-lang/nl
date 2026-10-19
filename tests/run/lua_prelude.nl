# NL names for Lua's libraries; file handles have marked methods
print Number.floor(2.5)
print Number.max(3, 7)
print Number.sqrt(16)
let t = ["b", "c", "a"]
Table.sort(t)
print Table.concat(t, ",")
Table.insert(t, "d")
print t.len
print Table.remove(t)
IO.write("written by IO.write\n")
IO.stdout:write("written by a Lua method call\n")
IO.stdout.write("written by a marked method\n")
print typeof(OS.time())
print Bit.band(6, 3)
print Bit.tohex(255)
print typeof(Debug.traceback)
print typeof(Package.loaded)
print typeof(Jit.version)
print typeof(Event.run)
print typeof(String.upcase)
print String.upcase("abc")

let body = fn(a)
  let b = Coroutine.yield(a + 1)
  b * 2
end
let co = Coroutine.create(body!)
let ok1, v1 = Coroutine.resume(co, 1)
print "#{ok1} #{v1}"
let ok2, v2 = Coroutine.resume(co, 5)
print "#{ok2} #{v2}"
print Coroutine.status(co)
