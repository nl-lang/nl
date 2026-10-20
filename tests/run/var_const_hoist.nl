# A const is declared at the top of its block, like fn, and gets its value on its line
fn total(n)
  n * RATE + BONUS
end

const RATE = 3
const BONUS: Number = 1
print total(2)

# a closure that runs before the const line reads nil
let early = fn() LIMIT end
print early() == nil
const LIMIT = 10
print early()

# module functions see the module's consts, declared below them
module Shop
  fn price(n)
    n * TAX
  end

  fn label
    "#{NAME} (tax #{Self.TAX})"
  end

  const TAX = 2
  const NAME = "shop"
end
print Shop.price(5)
print Shop.label

# in a function body
fn inner
  let get = fn() k * 2 end
  const k = 7
  get()
end
print inner()

# a const holding a function can call itself
const countdown = fn(n) if n > 0 then countdown(n - 1) else "liftoff" end end
print countdown(3)

# a const redeclared in the same block is not hoisted: each line sees the one above it
fn twice
  const step = 1
  let first = step
  const step = 2
  first + step
end
print twice()
