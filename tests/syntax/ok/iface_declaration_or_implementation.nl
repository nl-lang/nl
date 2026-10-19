# After `interface` and a type reference, `for` on the same line
# makes an implementation; otherwise it is a declaration.
interface From<T>
  fn from(v: T): Self
end
interface From<String> for Name
  fn from(v: String): Self
    Name {}
  end
end
