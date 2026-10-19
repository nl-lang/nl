# An implementation that fails changes
# nothing: here a method of the implementation for Number
# exists already, defined by another file, so neither the
# conformance nor the other method is recorded.
import { Tagged } from 'lib/iface_tagged'
fn Number.label
  "label from the main file"
end
try
  include("lib/iface_tagged_number.nl")
catch e
  print "include failed: #{e.message}"
end
print(5 is Tagged)
print 5.label
print typeof(5.tag)

