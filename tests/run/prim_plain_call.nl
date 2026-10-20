# A method on a primitive type called through the type, with its receiver as the first argument
fn String.shout
  self.upcase <> "!"
end

fn String.surround(left: String, right: String): String
  left <> self <> right
end

fn Number.plus(n)
  self + n
end

fn Bool.flip
  !self
end

fn Bool.nor(other)
  Self.flip(self or other)
end

print String.shout("hey")
print String.surround("x", "[", "]")
print Number.plus(40, 2)
print false.nor(false)
print true.nor(false)

# The same calls through the function value
print((String.shout!)("hey"))
print((Number.plus!)(1, 2))

# A command call and a pipe pass the first argument the same way
let loud = String.shout "cmd"
print loud
print "piped" |> String.shout

# Read without a call, the member is the function itself, as a library function is
let shout = String.shout
print shout("value")
let upcase = String.upcase
print upcase("library")

# Library functions are unchanged
print String.upcase("abc")
print Number.max(3, 9)

# `from` of `From<T>` is called on the type, which stays its self
interface From<Bool> for Number
  fn from(val: Bool): Self
    if val
      1
    else
      0
    end
  end
end

print Number.from(true)

# Through a value of the type, the value is self as before
print "dot".shout
print 5.plus(5)

# Lua code calls it the same way, from the global table
print loadstring("return String.shout('lua')")()
