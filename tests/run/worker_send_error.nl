# Values that cannot cross raise SendError at the sending line, or fail the task
import { identity, make_closure } from 'worker/jobs'

fn make_adder(n)
  let f = fn(x) x + n end
  f!
end

async fn main
  let closure: any = make_adder(1)
  try
    let t = Worker.run(closure, 5)
    print "not reached"
  catch err: SendError
    print "closure as callee: SendError"
  end
  try
    let t = spawn identity({callbacks: [closure]})
    print "not reached"
  catch err: SendError
    print "closure in an argument: SendError"
  end
  let done: any = Task.new do |resolve, reject|
    resolve(1)
  end
  try
    let t = spawn identity(done)
    print "not reached"
  catch err: SendError
    print "task in an argument: SendError"
  end
  let co: any = Coroutine.create(make_adder!)
  try
    let t = spawn identity(co)
    print "not reached"
  catch err: SendError
    print "coroutine in an argument: SendError"
  end
  try
    let t = spawn identity(setmetatable({}, {}))
    print "not reached"
  catch err: SendError
    print "unregistered metatable: SendError"
  end
  try
    let v = spawn make_closure().await
    print "not reached"
  catch err: SendError
    print "closure in the result: SendError"
  end
end

main()
