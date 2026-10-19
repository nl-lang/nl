# Rejected by the resolver.
fn main
  let count = 0
  let t = spawn do
    # expect-error: assigning to a captured local in a `spawn do` block; only the worker's copy would change
    count = 1
  end
end
