# Default parameter values, evaluated at each call whose argument is nil
let calls = 0
fn next_id
  calls += 1
  calls
end

fn make(name, id = next_id())
  "#{name}:#{id}"
end

print make("a")
print make("b")
print make("c", 99)
print make("d", nil)
print calls

fn span(from_x, to_x = from_x + 10)
  to_x - from_x
end

print span(5)
print span(5, 6)

fn summon(kind: String, level: Number = 1): String
  "#{kind}@#{level}"
end

print summon("imp")
print summon("imp", 3)
