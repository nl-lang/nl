-- Run by global_fn.nl through gmod's include: the NL file's global items
-- are globals here.
local p = setmetatable({ a = 3, b = 4 }, Pair)
return greet("lua") .. ", " .. Counter:twice(21) .. ", " .. tostring(getmetatable(p) == Pair)
