# `,` is not a continuation token, so a line that starts
# with `,` does not continue the list of the line before. One error per line.
return a
, b
f a
, b
x
, y = 1, 2
let c
, d = 1, 2
let e = 1
, 2
case x
when 1
, 2
  y
end
