# A trailing block after a callee or a call with parentheses.
items.each do |x|
  print x
end
f(a) do
  g()
end
f do
end
