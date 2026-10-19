-- For trace_fold.nl: a chain of functions, each loaded as a chunk of its
-- own name, the innermost calling `f`. The chunks named like the runtime's
-- and the embedder's ("=nl runtime" and "=nl host" in `nlc run`,
-- "@=[nl runtime]" and "@=[nl event]" on the fork, whose bcsave -F keeps
-- the "=" and adds "@") must be left out of a trace; a Lua file whose name
-- merely starts with "nl_" must not.
local names = { "=nl runtime", "=[nl runtime]", "@=[nl runtime]", "@=[nl event]", "=[nl event]", "=nl host", "@nl_helpers.lua" }
local function chain(f)
  local call = f
  for i = 1, #names do
    local inner = call
    local wrap = assert(loadstring("local inner = ...\nreturn function()\n  local r = inner()\n  return r\nend", names[i]))(inner)
    call = wrap
  end
  return call
end
return chain
