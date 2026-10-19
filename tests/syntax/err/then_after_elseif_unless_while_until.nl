# `then` in the headers of `elseif`, `unless`, `while` and
# `until`, each reported once.
if a
  x()
elseif x > 0 then
  y()
end
unless done then
  z()
end
while busy then
  w()
end
until done then
  v()
end
