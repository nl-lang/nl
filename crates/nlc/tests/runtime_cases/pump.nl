# rt.pump raises nothing; each queued item runs protected and a
# failure goes to the unhandled hook, without losing the rest of the batch.
include("lib/pump.lua")
