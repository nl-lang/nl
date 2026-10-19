return {
  raising = setmetatable({}, { __tostring = function() error("tostring broke") end }),
  nonstring = setmetatable({}, { __tostring = function() return 42 end }),
}
