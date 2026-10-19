# Rejected by the compiler.
# expect-error: a string literal passed to Regex.new is validated at compile time, and '(' is not a valid pattern
let re = Regex.new('(')
