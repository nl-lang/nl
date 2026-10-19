# Finishing a task does not grow the stack with
# the length of the chain of tasks waiting on it. Chains of 100,000 finish,
# in the order the waiters were added, and a failure reaches the end.
const N = 100000

let roots = {}
fn root
  Task.new do |resolve, reject|
    roots.resolve = resolve
    roots.reject = reject
  end
end

# A chain of tasks, each awaiting the one before.
async fn relay(t)
  t.await + 1
end
let last = {t: root()}
for i in 1..N
  last.t = relay(last.t)
end
last.t.on_done do |v| print "await chain: #{v}" end
roots.resolve(0)
print "after resolve"

# A failure travels down the whole chain and is handled at its end.
last.t = root()
for i in 1..N
  last.t = relay(last.t)
end
last.t.on_error do |e| print "failed chain: #{e.message}" end
roots.reject("root failed")

# Task.all of one task, nested.
last.t = root()
for i in 1..N
  last.t = Task.all([last.t])
end
last.t.on_done do |v| print "Task.all chain: #{typeof(v)}" end
roots.resolve(1)

# Callbacks that resolve the next task.
let first = root()
let start = roots.resolve
last.t = first
for i in 1..N
  let prev = last.t
  last.t = Task.new do |resolve, reject|
    prev.on_done do |v| resolve(v + 1) end
  end
end
last.t.on_done do |v| print "callback chain: #{v}" end
start(0)

# Waiters run in the order they were added; a task finished by a waiter
# has its own waiters run before the next waiter of the first task.
let order = []
let gate = root()
let open = roots.resolve
async fn waiter(name)
  gate.await
  Table.insert(order, name)
  name
end
let a = waiter("a")
a.on_done do |v| Table.insert(order, "a done") end
gate.on_done do |v| Table.insert(order, "callback") end
let b = waiter("b")
b.on_done do |v| Table.insert(order, "b done") end
open(nil)
print Table.concat(order, ", ")

# An error in a callback is reported and the other waiters still run.
include("lib/unhandled.lua")
let g2 = root()
g2.on_done do |v| raise "callback failed" end
g2.on_done do |v| print "second callback: #{v}" end
roots.resolve("ok")
