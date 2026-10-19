# Type aliases, with type parameters and a union continued on
# the next lines.
type Point = {x: Number, y: Number}
type Pair<A, B> = {first: A, second: B}
type Shape = Circle
  | Rect
  | Tri
type Composite = Number |
  String
