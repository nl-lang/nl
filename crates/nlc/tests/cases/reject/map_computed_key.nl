# Computed keys are not written in a literal.
let k = "a"
# expect-error: computed key is set with an assignment
let t = {[k]: 1}
