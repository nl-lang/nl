-- Records the names that files registered through rt.register,
-- for registry_names.nl: REGISTERED(file) gives them, sorted.
local rt = assert(NL_RT, "run with NLC_TEST_RT_GLOBAL=NL_RT")
local register = rt.register
local seen = {}
rt.register = function(file, name, v)
  local list = seen[file]
  if list == nil then list = {}; seen[file] = list end
  list[#list + 1] = name
  return register(file, name, v)
end
function REGISTERED(file)
  local list = {}
  for i, n in ipairs(seen[file] or {}) do list[i] = n end
  table.sort(list)
  return table.concat(list, " ")
end
