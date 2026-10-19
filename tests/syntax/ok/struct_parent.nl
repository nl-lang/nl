# `struct A : B` names the parent; spacing around `:`
# does not matter.
struct Bar : Foo
end
struct Baz :Foo
end
struct Qux: Lib.Foo
end
