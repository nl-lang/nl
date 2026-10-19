# Compiled, not run, by the test `try_forms` (cases.rs): each `try` here
# takes the inline form of crates/nl_ir/src/lower/error.rs (`rt.xpcall`,
# no `rt.try`), and lift.rs makes every closure outside the code that
# runs the `try`.
fn with_finally(a)
  let v = try
    a + 1
  catch e
    0
  finally
    print "fin"
  end
  v
end

fn returns_early(a)
  try
    return a * 2 if a > 0
  finally
    print "done"
  end
  0
end

for i in 1..3
  try
    print with_finally(i), returns_early(i)
  catch e: TypeError
    print "type error"
  end
end
