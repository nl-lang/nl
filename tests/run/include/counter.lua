-- Helper for import_include.nl: a Lua file that counts how often it has run.
IncludeCount = (IncludeCount or 0) + 1
print("counter.lua run " .. IncludeCount)
return IncludeCount * 10
