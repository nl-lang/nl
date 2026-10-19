# if, elseif, else, unless and the if/unless modifiers
fn classify(n)
  if n < 0
    print "negative"
  elseif n == 0
    print "zero"
  else
    print "positive"
  end
end

classify(-1)
classify(0)
classify(1)

let ready = false
unless ready
  print "not ready"
end
print "modifier if" if true
print "never printed" if false
print "modifier unless" unless false
print "never printed" unless true
let dirty = true
let saved = 0
saved += 1 if dirty
print saved
