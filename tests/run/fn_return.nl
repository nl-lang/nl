# Explicit returns, tail values, tail calls and `: nil`
fn two
  return 1, 2
end

let a, b = two()
print "#{a} #{b}"

fn pass_through
  two()
end

let c, d = pass_through()
print "#{c} #{d}"

fn tail_value(x)
  let y = x * 3
  y + 1
end

print tail_value(2)

fn branch(x)
  if x > 0
    "positive"
  elseif x < 0
    "negative"
  else
    "zero"
  end
end

print branch(5)
print branch(-5)
print branch(0)

fn no_else(x)
  if x
    "yes"
  end
end

print no_else(false) == nil

fn ends_with_let
  let z = 1
end

print ends_with_let() == nil

fn discards: nil
  42
end

print discards() == nil

fn positive(n): Bool
  if n > 0
    return true
  end
  false
end

print positive(1)
print positive(-1)
