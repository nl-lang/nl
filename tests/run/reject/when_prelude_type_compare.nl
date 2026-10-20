fn describe(v)
  case v
  # expect-error: a prelude type's name in `when` compares with the type itself, as a struct's does; the type test is `when is String`
  when String
    "a string"
  else
    "other"
  end
end

print describe("x")
