# A pipe target path has names only; a step after its call is
# an error with the hint to parenthesize.
x = a |> :f().g
y = a |> :t[1]
