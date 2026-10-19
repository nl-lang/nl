# Rejected by the resolver.
on Tick do
  print "first"
end

# expect-error: a second unnamed `on` for the same event in one file would replace the first; name one with `as`
on Tick do
  print "second"
end
