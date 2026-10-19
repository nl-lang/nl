# Every pattern row in must-match position (let), when it matches
let unit = {name: "orc", hp: 5, pos: {x: 1, y: 2}, tags: ["a", "b", "c"]}

do
  let {name, hp} = unit
  print "#{name} #{hp}"
end
do
  let {pos: {x, y}} = unit
  print "#{x} #{y}"
end
do
  let {hp: health} = unit
  print health
end
do
  let {name: String} = unit
  print name
end
do
  let {hp: life: Number} = unit
  print life
end
do
  let [first, second, third] = unit.tags
  print "#{first}#{second}#{third}"
end
do
  let [head, *tail] = unit.tags
  print "#{head} #{tail.len} #{tail[1]}"
end
do
  let [only, *] = unit.tags
  print only
end
do
  let ["a", "b", "c"] = unit.tags
  print "literal items match"
end
do
  let [{k}, 2] = [{k: "v"}, 2]
  print k
end
do
  let {pos: {x: px}} as whole = unit
  print "#{px} #{whole.name}"
end
do
  let {name: n}: Table = unit
  print n
end
do
  let {missing} = unit
  print missing
end
do
  let [1, b] = [1, "matched literal"]
  print b
end
do
  let {hp: 1..10} = unit
  print "range matches"
end
do
  let [is String, count] = ["s", 3]
  print count
end
do
  let {"someKey": some, 1: one} = {"someKey": "string key", 1: "first"}
  print "#{some} #{one}"
end
