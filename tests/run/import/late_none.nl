# Helper for lua_localize_late.nl: under `#!localize none` every field
# is read where it stands.
#!localize none
pub fn rep_none(s: String): String
  :string.rep(s, 2)
end
