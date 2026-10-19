# load_nl compiles NL source from a string; loadstring still compiles Lua
let chunk = load_nl('print "inside load_nl"', "inline_chunk")
chunk()
let exporting = load_nl("pub const answer = 42", "exporting_chunk")
let exports = exporting()
print exports.answer
let lua_chunk = loadstring("return 1 + 2")
print lua_chunk()
