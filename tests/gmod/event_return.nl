# Event.run is hook.Run: a non-nil first value ends the run, six values come back, else the gamemode runs
on CanJump as 'nl.jump' do |who|
  return "no" if who == "bob"
end

# A handler that returns nothing, or nil first, lets the run go on to the gamemode;
# a run that a handler ends gives six values, padded with nil.
:GAMEMODE.CanJump = fn(gm, who) "gamemode decides for #{who}" end
print Event.run("CanJump", "bob")
print Event.run("CanJump", "ann")

on Skip as 'nil.first' do
  return nil, "ignored"
end
print select("#", Event.run("Skip"))

# At most six values come back.
on Many as 'm' do
  return 1, 2, 3, 4, 5, 6, 7
end
print select("#", Event.run("Many"))
print Event.run("Many")

# Event.run reaches a gamemode function with no handler at all.
:GAMEMODE.Ping = fn(gm, x) x + 1 end
print Event.run("Ping", 41)
