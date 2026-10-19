# `spawn` sends positional arguments only.
fn work(a, **opts)
  a
end
# expect-error: `spawn` sends positional arguments only
let t = spawn work(1, k: 2)
