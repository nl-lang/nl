# The host interface: rt.gettype and
# rt.settype follow the C API; rt.new and rt.exception validate. The host
# interface is reached from Lua through the test hook (NL_RT).
include("lib/host_types.lua")
