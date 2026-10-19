# The messages of SendError, the same on stock LuaJIT's stand-in and on the fork
import { identity } from 'worker/jobs'

fn make_closure(n)
  let f = fn(x) x + n end
  f!
end

fn local_struct
  struct Inner
    a: Number
  end
  Inner { a: 1 }
end

fn message(v: any): String
  try
    let _t = Worker.run(identity!, v)
    "sent"
  catch e: SendError
    e.message
  end
end

fn main
  let closure: any = make_closure(1)
  print message({callbacks: [1, closure]})
  print message([Coroutine.create(make_closure!)])
  let done: any = Task.new do |resolve, reject|
    resolve(1)
  end
  print message({t: done})
  print message({m: setmetatable({}, {})})
  print message({r: setmetatable({}, Regex)})
  print message({s: local_struct()})
  let keyed: any = {}
  keyed["a b"] = {inner: [closure]}
  print message(keyed)
  print message({u: newproxy(true)})
end

main()
