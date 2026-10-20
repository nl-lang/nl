# Helper for lua_localize_late.nl: reads a field of `string` that a file
# imported after this one adds, and one of Lua's own.
#!localize std
pub fn shout_all(s: String): String
  :string.shout(s)
end

pub fn rep_std(s: String): String
  :string.rep(s, 2)
end
