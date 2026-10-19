# A bundle split into parts (tests/gmod/run.sh gives it a small
# --part-limit), loaded last part first, with a sourcemap (client_parts.map):
# the output is compared after `nlc remap`.
import { shout } from "./helpers/shout"

print shout("parts")

fn deep(t)
  t.missing.field
end

try
  deep({})
catch e
  print e.message
end
