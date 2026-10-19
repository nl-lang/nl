# `interface I for Box` implements `I` for every `Box<T>`; the struct's type
# parameters are in scope by their declared names.
struct Box<T>
  value: T
end
interface Show
  fn show: String
end
interface Show for Box
  fn show: String
    "Box(#{@value})"
  end
end
let a = Box { value: 1 }
let b = Box { value: "s" }
print a.show, b.show, a is Show
fn first<T>(box: Box<T>): T
  box.value
end
print first(b)
