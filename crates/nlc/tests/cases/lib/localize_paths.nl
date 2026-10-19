# The code localized Lua paths compile to;
# cases.rs, localize_generated_code, builds it with each setting
# and runs it.
global Score = 1

fn area(r)
  :math.pi * r * r
end

# Read by every function through one local each.
fn shout(s)
  :string.upper(s)
end

print :string.format("%.1f", area(2)), shout("a")
# Assigned by the file: read where it stands, before and after.
print :math.floor(2.5)
:math.floor = :math.floor
# The root assigned through `_G`: no path of `table` is localized.
print :table.concat(["a", "b"], ",")
:_G.table = :table
# `os` may be missing (the sandbox has none): its field is guarded.
print :os.getenv("NL_SURELY_UNSET_VARIABLE")
# A root alone, and a root followed by an index: localized under `all`.
:print "printed"
print :string["len"]("abc")
# A global the file declares stays a variable.
Score += 1
print :Score
