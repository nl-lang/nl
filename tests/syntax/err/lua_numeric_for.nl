# Lua's numeric `for`; one error that shows the NL form, and the
# body still belongs to the loop.
for i = 1, 10 do
  print(i)
end
for j = 1, 10, 2
  print(j)
end
