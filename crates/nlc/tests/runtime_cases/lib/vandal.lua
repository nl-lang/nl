-- Replaces what a runtime that looked things up at call time would use.
string.gmatch, string.sub, string.match, string.gsub, string.find = nil, nil, nil, nil, nil
string.format, string.byte, string.char, string.rep = nil, nil, nil, nil
table.concat, table.sort, table.remove, table.insert = nil, nil, nil, nil
coroutine.create, coroutine.resume, coroutine.status, coroutine.yield, coroutine.running = nil, nil, nil, nil, nil
loadstring, loadfile, load = nil, nil, nil
getmetatable, setmetatable = nil, nil
debug = nil
io = nil
