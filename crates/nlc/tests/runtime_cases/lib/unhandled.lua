-- Reports unhandled failures on stdout, for the cases that check them.
local rt = assert(NL_RT, "run with NLC_TEST_RT_GLOBAL=NL_RT")
rt.setunhandled(function(e) print("unhandled: " .. rt.typeof(e) .. ": " .. tostring(e.message)) end)
