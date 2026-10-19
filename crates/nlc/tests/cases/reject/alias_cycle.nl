# Aliases that refer to each other without a record in between
# describe no value; an error in both build modes.
# expect-error: `A` names `B`, which names `A` again
type A = B | Number
type B = A | String
let x: A = 1
print x
