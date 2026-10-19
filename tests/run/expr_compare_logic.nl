# Comparison and logical operators, truthiness
print(1 == 1)
print(1 != 1)
print(1 < 2)
print(2 <= 2)
print(3 > 2)
print(3 >= 4)
print("a" < "b")
print(1 == "1")
print(nil or 5)
print(false || "x")
print(1 and 2)
print(nil && 2)
print(not nil)
print(!true)
print(true or false and false)
print((true or false) and false)
print(0 and "zero is truthy")
print("" and "empty string is truthy")

let calls = 0
fn touch
  calls += 1
  true
end

let r = false and touch()
let r2 = true or touch()
print calls
