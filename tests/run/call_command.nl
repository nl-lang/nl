# Command calls
fn add(a, b)
  a + b
end

fn five
  5
end

fn each(list: any, f: Function)
  for v in list
    f(v)
  end
end

let r = add 1, 2
print r
let r2 = add 1, add(2, 3)
print r2
let s = five -1
print s
print "a", "b"
let list = [1, 2]
each list do |v|
  print "item #{v}"
end
each list do |v| print("inline #{v}") end
