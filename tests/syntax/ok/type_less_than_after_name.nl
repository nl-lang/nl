# `<` after a declared name opens type parameters; in a type it
# opens type arguments; after `as` and in an expression it is less-than;
# after `fn Type.` it is an operator name.
fn f<T>(x: T)
end
struct Box<T> end
type Pair<A, B> = {a: A, b: B}
let b: Box<T> = x
ok = x as Number < 10
ok = a < b
fn V.<(o) end
fn V.<=(o) end
fn V.<<(o) end
fn V.<>(o) end
