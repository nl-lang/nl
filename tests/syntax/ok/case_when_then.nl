# `then` ends the header of a `when` of `case` (after the values or the
# guard) and of `cond`; a `case` may take its first `when` on its own line.
let name = case n
when 1 then "one"
when 2, 3 then "a few"
when Point {x} if x > 0 then "right"
else "many"
end
let big = cond
when n > 9 then true
else false
end
let short = case n when 1 then "one" else "other" end
