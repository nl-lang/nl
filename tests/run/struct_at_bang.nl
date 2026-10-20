# @m! in a method is self.m!: a method comes bound to self, a field is its value, M["m"] is the raw method
struct Box
  v: Number
  cb: Function?
end

fn Box.get(x)
  "#{@v}:#{x}"
end

fn Box.both
  let f = @get!
  let g = self.get!
  print f(1), g(1), @get! == self.get!
  let c = @cb!
  print c(5)
  let raw = self["get"]
  print raw(self, 3)
end

let box = Box { v: 2, cb: fn(x) x * 2 end }
box.both

# Lua code calls a bound method without the receiver.
struct Sorter
  key: String
end

fn Sorter.before(a, b)
  a[@key] < b[@key]
end

fn Sorter.sort(list: [Table])
  :table.sort(list, @before!)
  list.map(fn(x) x[@key] end).join(" ")
end

print Sorter { key: "n" }.sort([{n: 3}, {n: 1}, {n: 2}])

# In a module, @f! binds the module.
module Greeter
  fn greet(name)
    "hi #{name}"
  end

  fn later
    @greet!
  end
end
print Greeter.later()("ann")
