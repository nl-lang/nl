# fmt: named placeholders from keyed arguments or a table, positions, specifiers, escaped braces
let who = "Ayla"
print "{name} has {n} coins".fmt(name: who, n: 3)
print "{name} has {n} coins".fmt({name: "Bjørn", n: 12})
print "{1} and {2}, then {1}".fmt("héllo", "wörld")
print "{1}: {name}".fmt("first", name: "keyed")
print "[{1}]".fmt(["from a list"])

# A Lua format specifier after a colon
print "{price:.2f} {id:04d} {hex:x} {label:8s}".fmt(price: 2.5, id: 7, hex: 255, label: "x")
print "{w:-6s}|".fmt(w: "ab")

# {{ and }} are braces; a brace that starts no placeholder stays
print "{{literal}} {{{v}}}".fmt(v: 1)
print "{ x } {} } {".fmt(x: 1)
print "no placeholders".fmt(x: 1)

# Values go through tostring
struct Point
  x: Number
  y: Number
end
interface To<String> for Point
  fn to: String
    "(#{@x}, #{@y})"
  end
end
print "at {p}".fmt(p: Point { x: 1, y: 2 })
print "{ok} {none}".fmt(ok: true, none: false)

# A name with no value raises KeyError
try
  print "{missing}".fmt(name: who)
catch err: KeyError
  print err.message
end
try
  print "{3}".fmt("a", "b")
catch err: KeyError
  print err.message
end
