# `interface From<T>`; signatures may have type parameters.
interface From<T>
  fn from(val: T): Self
  fn map<U>(f: Function(x: T): U): U
end
