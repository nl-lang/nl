# include of a Lua file is gmod's include; the global include stays gmod's
let v = include("helper.lua")
print "helper returned #{v}"
print(:include != nil, :load_nl == nil, :add_path == nil)
print(:typeof == typeof, :String != nil)
