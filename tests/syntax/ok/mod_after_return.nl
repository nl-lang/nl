# `return` directly followed by a modifier returns nothing;
# `return value if cond` returns the value conditionally.
fn f(target, amount)
  return if done
  return amount / 2 if target.armored
end
