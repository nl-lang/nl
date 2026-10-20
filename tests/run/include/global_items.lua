-- Included by module_global_fn.nl: Lua code reaches the file's `global`
-- items through the globals of their names.
print("lua: " .. class_("Door"))
print("lua: " .. Registry:size())
local v = Vec2:new(1, 2)
print("lua: " .. v:sum())
