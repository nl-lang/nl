-- Helper for string_from_lua.nl: NL's String library seen from a Lua file:
-- the global String; Lua's own string methods are untouched.
local M = {}

function M.run()
  local out = {}
  out[#out + 1] = String.length("héllo")
  out[#out + 1] = String.upcase("héllo")
  out[#out + 1] = ("héllo"):upper()
  out[#out + 1] = tostring(String.valid_encoding(string.char(255)))
  local ok, err = pcall(String.length, string.char(255))
  out[#out + 1] = tostring(ok) .. " " .. tostring(err)
  out[#out + 1] = table.concat(String.chars("aé"), ",")
  out[#out + 1] = String.format("%s-%d", "x", 3)
  return table.concat(out, "\n")
end

return M
