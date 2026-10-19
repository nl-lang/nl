# Rejected by the resolver.
fn outer
  fn inner(x)
    x
  end
  # expect-error: `inner` is declared inside another function, so a worker cannot look it up by name
  let t = spawn inner(1)
end
