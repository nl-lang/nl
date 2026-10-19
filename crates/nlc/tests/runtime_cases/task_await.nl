# `.await` outside a task raises, whether or not
# the task has finished.
let held = {}
let done = Task.new do |resolve, reject| resolve(9) end
let failed = Task.new do |resolve, reject| reject("bad") end
let waiting = Task.new do |resolve, reject| held.resolve = resolve end
for t in [done, failed, waiting]
  try
    print t.await
  catch e
    print "#{typeof(e)}: #{e.message}"
  end
end
failed.on_error do |e| end
held.resolve(nil)
# Inside a task it works.
async fn inside
  print "in a task: #{done.await}"
end
inside()

# When the yield of an `.await` fails (here across the C function
# Table.sort), its waiter is withdrawn: finishing the task later resumes
# nothing and reports nothing.
include("lib/unhandled.lua")
let gate = {}
let slow = Task.new do |resolve, reject| gate.open = resolve end
async fn across_c
  try
    Table.sort([2, 1]) do |a, b|
      slow.await
      a < b
    end
  catch e
    print "await across a C call: #{typeof(e)}"
  end
  "task done"
end
across_c().on_done do |v| print v end
gate.open(1)
print "opened"

include("lib/unhandled_reset.lua")
