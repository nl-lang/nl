# Type parameters are erased, so an implementation names the struct
# alone.
struct Box<T>
  value: T
end
interface Show
  fn show: String
end
# expect-error: `Box<Number>` takes no type arguments here
interface Show for Box<Number>
  fn show: String
    "n"
  end
end
