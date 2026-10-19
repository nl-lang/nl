# There is no match operator `=~`; the targeted error in a
# header, on the right of `=`, in a `let` and inside parentheses.
if x =~ re
  go()
end
ok = x =~ re
let m = x =~ re
print(x =~ re)
