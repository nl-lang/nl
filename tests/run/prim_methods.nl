# Methods on Number, String and Bool; library functions as methods
fn Number.is_multiple_of(n)
  self % n == 0
end

fn Number.is_cool
  self == 7
end

fn String.shout
  self.upcase <> "!"
end

fn Bool.flip
  !self
end

print 100.is_multiple_of(10)
print 100.is_multiple_of(7)
print 7.is_cool
print 2.5.floor
print(-5.abs)
print((-5).abs)
print Number.floor(2.5)
print 3.max(9)
print "hey".shout
print true.flip
let n = 16
print n.sqrt
print Number.pi
