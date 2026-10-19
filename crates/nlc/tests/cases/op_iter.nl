# `fn Name.iter` defines what `for v in x` iterates over for instances: it
# returns what Lua's generic `for` takes.
struct Bag
  items: [String]
end
fn Bag.iter
  ipairs(@items)
end
let bag = Bag { items: ["a", "b", "c"] }
for i, v in bag
  print i, v
end
struct Countdown
  from: Number
end
fn Countdown.iter
  let n = @from + 1
  fn()
    n -= 1
    return n if n > 0
  end
end
for n in Countdown { from: 3 }
  print n
end
