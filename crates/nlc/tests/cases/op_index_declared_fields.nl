# `[]` and `[]=` never run for a declared field: an
# optional field reads as nil and can be assigned, and `@name = v` inside
# `[]=` does not recurse.
struct Grid
  w: Number
  label: String?
end
fn Grid.[](key)
  "cell #{key}"
end
fn Grid.[]=(key, value)
  @label = "set #{key}=#{value}"
end
let g = Grid { w: 2 }
print g.label == nil, g[1], g.w
g[5] = "x"
print g.label
g.label = "direct"
print g.label
g.label = nil
print g.label == nil
