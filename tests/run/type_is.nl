# The `is` operator with every runtime-checkable kind of type
fn noop
end

let n = 1
let s = "a"
print(n is Number)
print(s is Number)
print(s is String)
print(true is Bool)
print(nil is nil)
print({} is Table)
print(noop! is Function)
print(Coroutine.create(noop!) is Thread)
print(IO.stdout is Userdata)
print(n is Number | String)
print(s is Number | String)
print(true is Number | String)
print(n is ~String)
print(s is ~String)
print(nil is Number?)
print(n is Number?)
print(s is Number?)
let u: unknown = 5
if u is Number
  print u + 1
end
