-- Helper for lua_interop.nl: a plain Lua module, loaded with require.
local M = {}

function M.apply(f, x)
  return f(x)
end

function M.call_method(obj, n)
  return obj:scaled(n)
end

function M.protect(f)
  local ok, err = pcall(f)
  return ok, err
end

function M.fail()
  error({code = 7})
end

function M.fail_plain()
  error("plain", 0)
end

return M
