# A function of Table is a method of a map's value, an optional list and a union of table types
let m: {[String]: [Number]} = {a: [1, 2], b: [3]}
print m["a"].sum, m.a.sum, m["b"].first
let k = "b"
print m[k].len, m[k].sum

fn total(v: [Number] | {[String]: Number}): Number
  v.values.sum
end
print total([1, 2, 3]), total({x: 4, y: 5})

fn first_of(v: [Number]?): Number?
  v?.first
end
print first_of([7, 8]), first_of(nil)

# a part of another kind is narrowed first: `v.reverse` alone would be an error
fn flip(v: [Number] | String)
  if v is String
    v.reverse
  else
    v.reverse.join(",")
  end
end
print flip([1, 2, 3]), flip("abc")
