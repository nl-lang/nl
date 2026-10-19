# A comparison of two constants of different
# types reaches the primitive operator definitions, as the same comparison
# of variables does; two numbers or two strings compare as Lua
# compares them.
fn Number.<(r)
  print "Number.< with #{typeof(r)}"
  true
end
fn Number.>(r)
  print "Number.> with #{typeof(r)}"
  false
end
fn Bool.<(r)
  print "Bool.< with #{typeof(r)}"
  true
end
print(1 < "a")
print(1 > "a")
print(true < false)
let s: any = "a"
print(1 < s)
print(1 < 2)
print("b" < "a")
print(2 >= 1)
