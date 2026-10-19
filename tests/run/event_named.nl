# Named handlers, order of addition, replacement, removal, `on` as an expression
on Tick as 'first' do
  print "first"
end

on Tick as 'second' do
  print "second"
end

Event.run "Tick"
Event.remove "Tick", "first"
Event.run "Tick"

on Solo as 'only' do
  print "old"
end

on Solo as 'only' do
  print "replaced"
end

Event.run "Solo"

on Mixed do
  print "unnamed"
end

on Mixed as 'named' do
  print "named"
end

Event.run "Mixed"

let handler_name = on Other as 'mine' do
  print "other"
end
print handler_name

on Game.Tick as 'dotted' do
  print "dotted event"
end

Event.run "Game.Tick"
Event.on_run("Manual", "m", fn(x) print("manual #{x}") end)
Event.run "Manual", 5
