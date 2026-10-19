# Blocks passed as the final argument to a function with a Function parameter, and plain do ... end scopes
fn apply(x: Number, f: Function)
  f(x)
end

fn each(list: any, f: Function)
  for v in list
    f(v)
  end
end

let doubled = apply 21 do |n|
  n * 2
end
print doubled

let typed = apply 5 do |n: Number|: Number
  n + 1
end
print typed

let points = [{x: 1, y: 2}, {x: 3, y: 4}]
each points do |{x, y}|
  print "#{x},#{y}"
end

fn call_with_nil(f: Function)
  f(1, nil)
end

let with_default = call_with_nil do |a, b = 10|
  a + b
end
print with_default

let discarded = apply 1 do |n: Number|: nil
  n + 100
end
print discarded

do
  let scoped = "plain scope block"
  print scoped
end
