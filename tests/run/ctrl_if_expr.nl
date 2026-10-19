# if, unless, case, cond and try are expressions
fn sign(n)
  if n < 0
    "minus"
  else
    "plus"
  end
end

print sign(-2)
print sign(2)

let label = if 1 > 2
  "bigger"
else
  "smaller"
end
print label

fn check(ok)
  unless ok
    "refused"
  end
end

print check(false)
print check(true) == nil

let word = case 2
when 1
  "one"
when 2
  "two"
end
print word

let size = cond
when 3 > 4
  "big"
else
  "small"
end
print size

let safe = try
  raise "x"
catch
  "recovered"
end
print safe
