# Patterns as function, block and for parameters
struct Monster
  name: String
  level: Number = 1
end

fn describe({name, level}: Monster)
  "#{name} (level #{level})"
end

print describe(Monster { name: "bat" })

fn hit({target, amount: Number} as event)
  "#{target} takes #{amount}, event has #{event.extra}"
end

print hit({target: "orc", amount: 3, extra: "x"})

fn first_two([a, b, *])
  a + b
end

print first_two([1, 2, 3, 4])

fn each(list: any, f: Function)
  for v in list
    f(v)
  end
end

let rows = [[1, 2], [3, 4]]
each rows do |[a, b]|
  print a * b
end
for [k, v] in rows
  print "#{k}->#{v}"
end
