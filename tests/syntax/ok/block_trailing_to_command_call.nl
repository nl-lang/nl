# In a command call the block belongs to the command
# call, after its last argument; in `a b(c) do … end` it goes to `a`.
date.replace_all text do |m|
  m
end
a b(c) do
end
