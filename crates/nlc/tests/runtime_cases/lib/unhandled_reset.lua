-- rt.setunhandled(nil) restores the default hook:
-- the report goes to stderr again, and the hook does not fail.
local rt = assert(NL_RT, "run with NLC_TEST_RT_GLOBAL=NL_RT")
rt.setunhandled(nil)
local ok = pcall(rt.unhandled, rt.exception(nil, "reported by the default hook"))
print("setunhandled(nil), then a report:", ok)
