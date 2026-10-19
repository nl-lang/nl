# A struct satisfies the interfaces of its ancestors, wherever it is declared
interface Named
  fn title: String
end

interface Priced
  fn price: Number
end

struct Item
  name: String
end

struct Tool : Item
  cost: Number = 3
end

interface Named for Item
  fn title: String
    "item #{@name}"
  end
end

struct Hammer : Tool
end

struct Stone
  name: String
end

fn label(n: Named): String
  n.title
end

fn cost_of(p: Priced): Number
  p.price
end

fn check(x, what)
  print what, x is Named, x is Priced
end

let hammer: any = Hammer { name: "claw" }
let tool: any = Tool { name: "saw" }
let item: any = Item { name: "box" }
let stone: any = Stone { name: "flint" }

check(item, "item")
check(tool, "tool")
check(hammer, "hammer")
check(stone, "stone")
check(5, "number")
check(nil, "nil")
print label(item), label(tool), label(hammer)

try
  cost_of(hammer)
catch err: TypeError
  print "a Hammer is not Priced yet"
end

interface Priced for Tool
  fn price: Number
    @cost * 2
  end
end

check(item, "item")
check(tool, "tool")
check(hammer, "hammer")
print cost_of(tool), cost_of(hammer)

try
  cost_of(item)
catch err: TypeError
  print "an Item is not Priced"
end
try
  label(stone)
catch err: TypeError
  print "a Stone is not Named"
end

interface Named for Hammer
  fn title: String
    "hammer #{@name}"
  end
end
print label(hammer), label(tool)

let mixed: any = [item, stone, hammer, 7, tool]
let named = 0
for v in mixed
  named += 1 if v is Named
end
print named
