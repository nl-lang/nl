# {name?} and {key?: p}: an optional field matches when it is nil, and binds nil
struct DiceNode
  count: Number?
  sides: Number
end

fn roll_text(node)
  case node
  when DiceNode {count?, sides}
    "#{count ?? 1}d#{sides}"
  else
    "not dice"
  end
end

print roll_text(DiceNode { count: 3, sides: 6 })
print roll_text(DiceNode { sides: 20 })
print roll_text({sides: 4})

# without `?` a nil field is a mismatch, as before
fn strict(node)
  case node
  when DiceNode {count, sides}
    "#{count}d#{sides}"
  else
    "no count"
  end
end

print strict(DiceNode { sides: 8 })

# in a record pattern, with a nested pattern: a missing field matches, and
# the names of its pattern are nil
fn describe(msg)
  case msg
  when {kind: "move", to?: {x, y}}
    let to_text = if x then "#{x},#{y}" else "nowhere" end
    "move to #{to_text}"
  else
    "other"
  end
end

print describe({kind: "move", to: {x: 1, y: 2}})
print describe({kind: "move"})
print describe({kind: "move", to: {x: 1}})

# a typed optional field: nil or the type
if let {name?: String, hp} = {hp: 10}
  print name == nil, hp
end
if let {name?: String} = {name: 5}
  print "matched #{name}"
else
  print "a field that is there must have the type"
end
