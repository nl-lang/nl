# The pipe operator
fn double(x)
  x * 2
end

fn add(a, b)
  a + b
end

module Calc
  fn sub(a, b)
    a - b
  end
end

print(5 |> double)
print(5 |> add(10))
print(10 |> Calc.sub(3))
let chained = 1
  |> double
  |> add(3)
  |> double
print chained
print(1 + 2 |> double)
print((3 |> double) == 6)
