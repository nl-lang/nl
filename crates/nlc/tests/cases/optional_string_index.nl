# An index of a union is the union of each member's index. A `nil` member
# raises at runtime (indexing nil) and adds no value, so a range index of
# a `String?` is a String, and a single index a `String?`.
fn pick(o: String?)
  let a: String = o[1..2]
  let b: String? = o[1]
  print(a, b)
end
pick("hello")
try
  pick(nil)
catch e: LuaError
  print("indexing nil raises")
end
