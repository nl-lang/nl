# Rejected by the resolver.
fn plain
  # expect-error: a file-level function that is not `fn Type.name` has no `Self`
  Self
end

plain()
