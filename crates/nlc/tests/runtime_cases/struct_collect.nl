# A struct, with its description, is collected once nothing
# refers to it (a struct declared in a function, a file run again with
# include).
fn make(i)
  struct P
    x: Number = 0
    next: P?
  end
  fn P.+(o)
    P { x: @x + o.x }
  end
  let p = P { x: i, next: P {} }
  (p + p).x
end

fn kb()
  :collectgarbage("collect")
  :collectgarbage("collect")
  :collectgarbage("count")
end

for i in 1..200
  make(i)
  include("lib/structs.nl")
end
let before = kb()
for i in 1..3000
  make(i)
  include("lib/structs.nl")
end
let grown = kb() - before
# Keeping the structs would take over 5 MB here. The growth measured
# without a leak varies between runs by up to about 800 KB (the weak
# tables of the runtime keep the size they grew to), so the bound is set
# well between the two.
print "bounded:", grown < 2000
