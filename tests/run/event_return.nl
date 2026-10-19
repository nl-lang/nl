# Tail values are discarded; the first handler that returns a value ends the run
on Calc as 'a' do |x|
  print "a sees #{x}"
  x + 100
end

on Calc as 'b' do |x|
  return x * 2
end

on Calc as 'c' do |x|
  print "c never runs"
end

let r = Event.run("Calc", 5)
print r

on Damage as 'armor' do |target, amount|
  return amount / 2 if target.armored
end

let full = Event.run("Damage", {armored: false}, 10)
print(full == nil)
let halved = Event.run("Damage", {armored: true}, 10)
print halved

on Multi as 'm' do
  return 1, 2
end

let p, q = Event.run("Multi")
print "#{p} #{q}"

on Quiet as 'q' do
  "tail values are discarded"
end

print select("#", Event.run("Quiet"))
