# `then` puts the body of if, unless, elseif and when on the header's line
fn sign(n)
  if n < 0 then "minus" elseif n == 0 then "zero" else "plus" end
end

print sign(-2)
print sign(0)
print sign(5)

let hp = 30
let state = if hp > 50 then "healthy" else "hurt" end
print state

if hp < 50 then print "low health" end
unless hp > 0 then print "dead" else print "alive" end

fn name(n)
  case n
  when 1 then "one"
  when 2, 3 then "a few"
  when is Number if n < 0 then "negative"
  else "many"
  end
end

print name(1)
print name(3)
print name(-4)
print name(9)

let short = case hp when 30 then "thirty" else "other" end
print short

let size = cond
when hp > 100 then "big"
when hp > 10 then "medium"
else "small"
end
print size

if let n = [7, 8][2] then print "found #{n}" end

# a body may still begin on the next line after `then`
if hp == 30 then
  print "exactly thirty"
end
