# The trade of localized Lua paths: the value is read when the
# file loads, so a Lua global another file changes later is not seen.
# cases.rs, localize_trade, runs it with each setting.
fn upper(s)
  :string.upper(s)
end

fn late
  :LateGlobal
end

print upper("a"), late()
include("lib/localize_late.lua")
print upper("a"), late()
