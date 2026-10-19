# Self in a struct body, in a primitive method and in a module
struct Node
  value: Number
  next: Self?
end

fn Node.total: Number
  if @next == nil
    return @value
  end
  @value + @next.total
end

let list = Node { value: 1, next: Node { value: 2, next: Node { value: 3 } } }
print list.total

fn Number.halve
  Self.floor(self / 2)
end

print 7.halve

module Greeter
  fn helper(name)
    "hello #{name}"
  end

  fn greet(name)
    Self.helper(name)
  end
end

print Greeter.greet("ann")
