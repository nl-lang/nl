# values typed Table, [T] and {[K]: V} are indexed directly, with the meaning untyped code gives them
struct Bag
  items: [Number]
end

fn Bag.len
  @items.len + 100
end

fn Bag.label
  @items[1]
end

fn first(xs: [Number])
  xs[1]
end

fn size(t: Table)
  t.len
end

fn field(t: Table)
  t.label
end

fn raw(t: Table)
  t.label!
end

fn count(m: {[String]: Number}, k: String)
  m[k] ?? 0
end

fn attempt(label: String, f: Function)
  try
    print label, f()
  catch e: TypeError
    print label, e.message
  end
end

let bag = Bag { items: [7, 8] }
let s: any = "text"
let n: any = 5
print first([4, 5]), size([1, 2, 3]), size({}), count({a: 2}, "a"), count({}, "b")
print size(bag), field(bag), typeof(raw(bag))
print field({label: "plain"}), typeof(field({label: x -> x}))
let proxy = :setmetatable({}, {__index: {label: "inherited"}})
print field(proxy), size(proxy)
attempt "string", || -> first(s)
attempt "number", || -> size(n)
let none: any = nil
attempt "nil", || -> field(none)
let t: Table = {}
attempt "assign" do
  t = s
end
print t.len
