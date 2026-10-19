# Braces go by key, brackets by position; positional patterns are exact in length
fn check(v)
  case v
  when [123, 456]
    "exact pair"
  when {1: 123, 2: 456}
    "open pair"
  when [x, 456]
    "bracket #{x}"
  else
    "none"
  end
end

print check([123, 456])
print check([123, 456, 789])
print check({1: 123, 2: 456, extra: true})
print check([9, 456])
print check([123])
print check([7, 456, 1])
