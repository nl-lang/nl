# `do` followed on the same line by `|` in a header is a
# trailing block, not the Lua `do`.
if items.any do |x| x > 0 end
  g()
end
