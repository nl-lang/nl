# `on` and Event go through gmod's hook library: one set of handlers for NL and Lua
on PlayerSpawn as 'nl.spawn' do |who|
  print "NL handler: #{who} spawned"
end

# The engine and Lua code reach NL handlers through hook.Run / hook.Call.
:hook.Run("PlayerSpawn", "bob")
:hook.Call("PlayerSpawn", nil, "ann")

# Lua handlers are reached through Event.run.
:hook.Add("Greet", "lua.greet", fn(name) print("Lua handler greets #{name}") end)
Event.run "Greet", "kim"

# The handler name is the hook identifier; an unnamed `on` uses the file's name.
on Unnamed do
  print "unnamed handler"
end
let table = :hook.GetTable()
print(table.PlayerSpawn["nl.spawn"] != nil, table.Unnamed["event_hook.nl"] != nil)

# Lua code can remove an NL handler, and NL a Lua one.
:hook.Remove("PlayerSpawn", "nl.spawn")
:hook.Run("PlayerSpawn", "nobody")
Event.remove "Greet", "lua.greet"
Event.run "Greet", "nobody"
print "after removal"

# Adding a handler again under its name replaces it.
on Replace as 'r' do
  print "old"
end
on Replace as 'r' do
  print "new"
end
Event.run "Replace"

# Event.on_run is hook.Add; a number name stands for its string form.
let seven: any = 7
Event.on_run("Manual", seven, fn(x) print("manual #{x}") end)
print(table.Manual["7"] != nil)
Event.run "Manual", 5

# Bad arguments raise, as NL's own event library does.
let bad: any = {}
try
  Event.on_run("E", bad, fn() end)
catch e
  print "on_run: #{e.message}"
end
try
  Event.run bad
catch e
  print "run: #{e.message}"
end
