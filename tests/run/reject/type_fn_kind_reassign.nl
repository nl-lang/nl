# Rejected by the analyzer.
let f = fn 1 end
# expect-error: a `let` initialized with an `fn` expression counts as annotated `Function`; assigning a number to it is a type error
f = 5
