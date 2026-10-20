# Rejected by the resolver: the block takes a copy of `args`, which does not carry its count.
fn count_later(*args)
  let t = spawn do
    # expect-error: the count of the rest parameter `args` in a `spawn do` block; a trailing nil would be lost
    args.len
  end
  t
end

print count_later(1, 2) unless Worker.parent
