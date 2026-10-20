-- Lua code calling NL's String methods (string_lua_methods.nl): a method
-- that NL code defined, NL's own library, and Lua's names, which stay Lua's.
local s = "héllo wörld"
return {
  shout = s:shout(),
  shout_fn = string.shout("lua"),
  snake = ("UsableItem"):snake_case(),
  words = #s:split(" "),
  length = s:length(),
  bytes = s:len(),
  upper = s:upper(),
  reverse_bytes = ("é"):reverse() == "\169\195",
  missing = string.no_such_function == nil,
}
