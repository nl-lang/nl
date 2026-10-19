# Structs are defined parents first; a cycle of parents has no
# order and is an error.
# expect-error: `A` inherits from `B`, which inherits from `A`
struct A : B
end
struct B : A
end
