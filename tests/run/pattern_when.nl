# Every pattern row in may-match position (when); a mismatch moves on
struct Circle
  r: Number
end

struct Tagged
  tag: String
end

fn test(v)
  case v
  when {tag: "lit", n: 1}
    "literal number"
  when {tag: "bool", flag: true}
    "literal bool"
  when {tag: "range", n: 1..5}
    "range"
  when {tag: "is", n: is Number}
    "is"
  when {tag: "nested", inner: {deep}}
    "nested #{deep}"
  when {tag: "short", name}
    "shorthand #{name}"
  when {tag: "typed", value: Number}
    "typed #{value}"
  when {tag: "renamed", value: val: String}
    "renamed #{val}"
  when [a, b]
    "two #{a} #{b}"
  when [head, *rest]
    "head #{head} rest #{rest.len}"
  when Circle {r}
    "circle #{r}"
  when {tag: "as"} as whole
    "as #{whole.extra}"
  when {tag: "colon"}: Tagged
    "colon"
  else
    "no match"
  end
end

print test({tag: "lit", n: 1})
print test({tag: "lit", n: 2})
print test({tag: "bool", flag: true})
print test({tag: "bool", flag: false})
print test({tag: "range", n: 3})
print test({tag: "range", n: 7})
print test({tag: "is", n: 4})
print test({tag: "is", n: "4"})
print test({tag: "nested", inner: {deep: "d"}})
print test({tag: "nested", inner: 5})
print test({tag: "short", name: "kim"})
print test({tag: "short"})
print test({tag: "typed", value: 3})
print test({tag: "typed", value: "x"})
print test({tag: "renamed", value: "s"})
print test({tag: "renamed", value: 1})
print test([1, 2])
print test([1, 2, 3])
print test({})
print test(Circle { r: 4 })
print test({tag: "as", extra: "e"})
print test(Tagged { tag: "colon" })
print test({tag: "colon"})
