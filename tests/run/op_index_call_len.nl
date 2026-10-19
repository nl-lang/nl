# len, [], []= and () definitions
struct Bag
  items: Table
  log: String = ""
end

fn Bag.len
  @items.len
end

fn Bag.[](key)
  "missing #{key}"
end

fn Bag.[]=(key, value)
  @log = @log <> key
end

fn Bag.describe
  "a bag"
end

let b = Bag { items: [1, 2, 3] }
print b.len
print b[1]
print b.other
print b.describe
print b.log == ""
b.extra = 1
b.more = 2
print b.log
b.log = "reset"
print b.log
print(b.extra == "missing extra")

struct Adder
  base: Number
end

fn Adder.()(*args)
  @base + args[1]
end

let add5 = Adder { base: 5 }
print add5(10)
