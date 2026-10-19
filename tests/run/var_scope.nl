# Lexical scope, do ... end blocks, nested functions
let a = 1
do
  let a = 2
  print a
  let inner = 3
  print inner
end
print a

fn outer
  let v = "outer"
  fn inner_fn
    v
  end
  inner_fn
end

print outer
