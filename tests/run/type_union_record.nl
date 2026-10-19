# A union takes a value of any of its parts and a record needs every field, however many there are
struct Cat
  name: String
end

struct Dog
  name: String
end

struct Fish
  name: String
end

interface Swims
  fn depth: Number
end

interface Swims for Fish
  fn depth: Number
    3
  end
end

enum Size
  Small
  Large
end

type Two = Cat | Dog
type Three = Cat | Dog | Swims
type Four = Cat | Dog | Swims | Size
type Five = Cat | Dog | Swims | Size | Bool
type One = {x: Number}
type Pair = {x: Number, y: String}
type Quad = {x: Number, y: String, z: Bool, pet: Two}
type Wide = {x: Number, y: String, z: Bool, pet: Two, size: Size}

fn marks(tests)
  let out = ""
  for t in tests
    out = out <> (t and "1" or "0")
  end
  out
end

fn unions(v)
  marks([v is Two, v is Three, v is Four, v is Five])
end

fn records(v)
  marks([v is One, v is Pair, v is Quad, v is Wide])
end

let cat = Cat { name: "tom" }
let values: any = [cat, Dog { name: "rex" }, Fish { name: "nemo" }, Size.Large, false, 7]
for v in values
  print typeof(v), unions(v)
end

let tables: any = [
  {},
  {x: 1},
  {x: 1, y: "a"},
  {x: 1, y: 2},
  {x: 1, y: "a", z: true, pet: cat},
  {x: 1, y: "a", z: true, pet: Fish { name: "nemo" }},
  {x: 1, y: "a", z: true, pet: cat, size: Size.Small},
  {x: 1, y: "a", z: true, pet: cat, size: "huge", more: 1},
]
for t in tables
  print records(t)
end
print records(cat), records(5)

fn name_of(pet: Three): String
  typeof(pet)
end

fn sum(p: Quad): Number
  p.x + p.y.length
end

print name_of(Fish { name: "dory" }), sum(tables[5])
let wrong: any = Size.Small
try
  name_of(wrong)
catch err: TypeError
  print "a Size is not a Cat | Dog | Swims"
end
try
  sum(tables[6])
catch err: TypeError
  print "a Fish is not the pet of a Quad"
end
