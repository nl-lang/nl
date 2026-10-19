# Generic functions, structs and interfaces (type parameters are erased)
fn identity<T>(a: T): T
  a
end

print identity(5)
print identity("five")

fn first<T>(list: [T]): T
  list[1]
end

print first([7, 8])

fn pair<A, B>(a: A, b: B): A
  a
end

print pair(1, "x")

struct Box<T>
  value: T
end

let b: Box<Number> = Box { value: 3 }
print b.value
let s = Box { value: "s" }
print s.value
print(b is Box)

interface Container<T>
  fn get: T
end

struct Counter
  n: Number
end

interface Container<Number> for Counter
  fn get: Number
    @n
  end
end

let counter = Counter { n: 4 }
print counter.get
