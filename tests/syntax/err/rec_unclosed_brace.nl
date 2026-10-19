# Recovery: an unclosed `{` of a table; the declaration on the next line
# parses. (A next line such as `y = 3` is read as a missing comma instead;
# see the report of the snapshot suite.)
x = {a: 1, b: 2
let y = 3
