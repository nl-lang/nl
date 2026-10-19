# The Lua stand-in of the `event` library follows the rules
# of the C library.
include("lib/event_rules.lua")

# Any value a handler returns, nil included, ends the run.
on Ret as 'a' do
  print "a"
end
on Ret as 'b' do
  return nil
end
on Ret as 'c' do
  print "c not reached"
end
print "run returned: #{Event.run("Ret")}"

# During a run: an added handler is not called, a removed one is skipped,
# a replaced one runs as replaced, and a re-added name keeps its place.
on Run as 'one' do
  print "one"
  on Run as 'added' do print "added" end
  Event.remove "Run", "two"
  on Run as 'three' do print "three, replaced" end
end
on Run as 'two' do print "two" end
on Run as 'three' do print "three" end
Event.run "Run"
print "second run:"
Event.run "Run"
on Run as 'one' do print "one, re-added" end
print "third run:"
Event.run "Run"

# `on` yields the handler name.
let name = on Named as 'n' do end
print "on yields: #{name}"

# A plain handler may await when the run is in a task.
let gate = {}
on Wait do
  print "handler waits"
  Task.new(fn(resolve, reject) gate.open = resolve end).await
  print "handler resumed"
  return "handler value"
end
async fn main
  let v = Event.run("Wait")
  print "run in a task returned: #{v}"
end
main()
gate.open(nil)
