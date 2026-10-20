# Table's whole-table functions: merge, deep_merge, copy, deep_copy, equal, deep_equal, size, has_key, get, dig, pick, omit
struct Point
  x: Number
  y: Number
end

let show = m -> Table.map(Table.sorted_keys(m), k -> "#{k}:#{m[k]}").join(" ")

let base = {a: 1, b: 2}
let merged = Table.merge(base, {b: 20, c: 30}, {d: 40})
print show(merged), show(base)
print Table.merge_into(base, {z: 26}) == base, show(base)

let defaults = {window: {w: 640, h: 480}, title: "game"}
let deep = Table.deep_merge(defaults, {window: {h: 720}, vsync: true})
print show(deep.window), deep.title, deep.vsync, defaults.window.h

let p = Point { x: 1, y: 2 }
let config = {origin: p, list: [1, [2, 3]]}
config.self = config
let shallow = Table.copy(config)
print shallow == config, shallow.list == config.list, shallow.self == config
let copy = Table.deep_copy(config)
print copy.list == config.list, copy.list[2][2], copy.self == copy
print copy.origin == p, copy.origin is Point, copy.origin.x
let p2 = Table.copy(p)
print p2 is Point, p2 == p, p2.y

print Table.equal({a: 1, b: 2}, {b: 2, a: 1}), Table.equal({a: 1}, {a: 1, b: 2}), Table.equal({t: []}, {t: []})
print Table.deep_equal({t: [1, {u: 2}]}, {t: [1, {u: 2}]}), Table.deep_equal([1, 2], [1, 3])
print Table.deep_equal(config, copy), Table.deep_equal([], {})

print Table.size({a: 1, b: nil, 1: 2}), Table.size([]), Table.is_empty({}), Table.is_empty([0])
print Table.has_key(base, "a"), Table.has_key(base, "q"), Table.has_key({f: false}, "f")
print Table.get(base, "a", 0), Table.get(base, "q", 0), Table.get(base, "q"), Table.get({f: false}, "f", true)

let tree = {user: {address: {city: "Oslo"}, tags: ["x", "y"]}}
print Table.dig(tree, "user", "address", "city"), Table.dig(tree, "user", "tags", 2)
print Table.dig(tree, "user", "phone", "home"), Table.dig(tree, "user", "address", "city", "len")

print show(Table.pick(merged, ["a", "d", "nope"])), show(Table.omit(merged, ["a", "d"]))
