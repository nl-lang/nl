# case with expression items compared by ==
enum Direction
  North
  South
end

fn describe(v)
  let x = 42
  case v
  when 1, 2
    "one or two"
  when "s"
    "the string s"
  when x
    "equal to x"
  when Direction.North
    "north"
  when true
    "true"
  else
    "other"
  end
end

print describe(1)
print describe(2)
print describe("s")
print describe(42)
print describe("North")
print describe(true)
print describe(3)

fn no_else(v)
  case v
  when 1
    "one"
  end
end

print no_else(5) == nil
