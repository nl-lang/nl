# Helper for lua_localize_late.nl: adds `string.shout` and replaces
# `string.rep`, after the files imported before it have loaded.
let rep = :string.rep
:string.shout = fn(s) "#{s.upcase}!" end
:string.rep = fn(s, n) "<#{rep(s, n)}>" end
pub const installed = true
