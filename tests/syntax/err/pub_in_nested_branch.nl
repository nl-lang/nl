# `pub` is allowed only one level down, in a branch of a file-level `if`:
# not in a nested `if`, a loop, an `if` used as a value, or a `case`.
if a
  if b
    pub fn f end
  end
end
while c
  pub const x = 1
end
let v = if d
  pub fn g end
end
case e
when 1
  pub fn h end
end
