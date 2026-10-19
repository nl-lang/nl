# An error the VM raises keeps its NL position in
# its message and in the trace, also where the Lua backend prints the code
# after its line has passed (the line map of crates/nl_luagen). The same on
# both backends.
# backends: lua,bc

# A nil index, raised by the VM itself.
fn deep(k)
  let v = :string.nothing[k]
  v
end

# Arithmetic on a table, after a block expression that spans lines: the
# Lua backend prints the addition on line 20, after the block.
fn add_branch(a)
  let r = a + (if a
    1
  else
    2
  end)
  r
end

fn via(k)
  let r = deep(k)
  r
end

try
  deep("x")
catch e
  print e.message
  print e.trace
end

try
  add_branch({})
catch e
  print e.message
  print e.trace
end

try
  via("y")
catch e
  print e.message
  print e.trace
end
