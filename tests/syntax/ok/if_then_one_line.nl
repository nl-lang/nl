# `then` ends the header of `if`, `unless` and `elseif`, and the body
# follows on the same line; `then` at the end of a line is allowed too.
let x = if a then 1 else 2 end
if ready then go() end
unless done then wait() else stop() end
let s = if n == 1 then "one" elseif n == 2 then "two" else "many" end
if let v = find() then use(v) end
if ready then
  go()
end
