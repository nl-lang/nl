# on handlers, Event.run and handler parameters
on Greet do |name|
  print "hello #{name}"
end

on Hit do |{target, amount}|
  print "#{target} takes #{amount}"
end

on Typed do |a: Number, b: String|: nil
  print "#{b}#{a}"
end

Event.run "Greet", "bob"
Event.run("Hit", {target: "orc", amount: 3})
Event.run "Typed", 1, "x"
let r = Event.run("Greet", "ann")
print(r == nil)
print select("#", Event.run("Greet", "cy"))
:event.run("Greet", "from a lua path")
