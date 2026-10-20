# global fn, global module and global struct are stored in the globals of the chunk's environment (getfenv), where Lua code finds them
global fn greet(name: String): String
  "hello #{name}"
end

global module Counter
  fn twice(x: Number): Number
    x * 2
  end
end

global struct Pair
  a: Number
  b: Number
end

fn Pair.sum: Number
  @a + @b
end

print greet("nl"), Counter.twice(4), Pair { a: 1, b: 2 }.sum
print include("global_reader.lua")
print :greet == greet!, :Counter == Counter, :Pair == Pair
