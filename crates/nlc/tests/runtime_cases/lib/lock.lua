return function(S)
  S.__metatable = "locked"
  assert(getmetatable(setmetatable({}, S)) == "locked")
end
