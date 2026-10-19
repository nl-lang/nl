# Keyed arguments: `key: value` after the
# positional arguments, collected by a `**opts` parameter; `**t`
# passes a table; a written key wins over a `**` source; a block stays the
# final positional argument; Lua code gets the keyed table as its last
# argument and passes one with `keyed(t)`.
fn validates(field, *rest, **opts)
  print "#{field} rest=#{rest.len} presence=#{opts.presence} inclusion=#{opts.inclusion}"
end
validates 'role', presence: true, inclusion: 'x'
validates('role', 1, 2, presence: true)
validates 'role'
validates 'role', 1
let extra = { inclusion: 'y', presence: false }
extra[1] = 'positional item'
validates 'role', **extra, presence: true
validates('role', presence: true, **extra)
validates('role', **extra)
print extra.presence           # the source is unchanged
print typeof(extra[1])

# Defaults apply after the keyed table is taken out of its slot.
fn only(a, b = 2, **opts)
  print "a=#{a} b=#{b} k=#{opts.k}"
end
only(1, k: 3)
only 1, 5, k: 3
only 1, 5
only(1, 2, 3, k: 4)            # a function with `**opts` finds the keyed table beyond its parameters too

# Forwarding: `*rest` and `**opts` together.
fn inner(a, b, **opts)
  print "inner #{a} #{b} #{opts.k} #{opts.j}"
end
fn outer(*rest, **opts)
  inner(*rest, **opts)
  inner(*rest, **opts, j: 'written')
end
outer 1, 2, k: 'kv'
let list = [7, 8]
inner(*list, k: 'spread')
inner(*list)

# Member calls, Lua method calls and blocks.
struct Greeter
  name: String
end
fn Greeter.greet(**opts)
  print "#{opts.greeting or 'hello'}, #{@name}"
end
fn Greeter.each(list: any, callback: Function, **opts: any)
  for v in list
    callback(v * (opts.scale or 1))
  end
end
let g = Greeter { name: 'nl' }
g.greet
g.greet(greeting: 'hi')
g.greet greeting: 'hey'
g.each([1, 2], scale: 10) do |v| print v end
g.each([1, 2]) do |v| print v end

# A Lua function gets the keyed table as its last argument.
let lf = :loadstring("return function(a, t) return a, t and t.k end")()
let a, k = lf(1, k: 2)
print "#{a} #{k}"
# Lua passes keyed arguments with keyed(t).
let call_from_lua = :loadstring("return function(f) return f(1, keyed({k = 5, [2] = 'dropped'})) end")()
fn takes(a, **opts)
  "#{a} #{opts.k} #{opts[2]}"
end
print call_from_lua(takes!)
print typeof(keyed({}))

# A table built by hand is not keyed arguments; `**` only copies string keys.
fn show(**opts)
  let keys = []
  for key, _ in pairs(opts)
    Table.insert(keys, tostring(key))
  end
  Table.sort(keys)
  print Table.concat(keys, ',')
end
let mixed = {a: 1, b: 2}
mixed[1] = 'no'
show(**mixed)
show a: 1, b: 2
show()
try
  show(**5)
catch e
  print e.message
end
