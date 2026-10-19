# Function definitions, anonymous functions and hoisting
print is_even(10)
print is_odd(7)

fn is_even(n)
  if n == 0
    return true
  end
  is_odd(n - 1)
end

fn is_odd(n)
  if n == 0
    return false
  end
  is_even(n - 1)
end

fn greet(name, greeting = "hello")
  "#{greeting}, #{name}"
end

print greet("ann")
print greet("bob", "hi")

fn area(w: Number, h: Number): Number
  w * h
end

print area(3, 4)

let double = fn(x) x * 2 end
print double(4)
let hello = fn print("hi") end
hello
let pick = fn() ({a: 1, b: 2}).b end
print pick

fn no_params
  "no parentheses needed"
end

print no_params
