# Helper for import_once.nl: imports stateful.nl relative to its own directory.
import { bump } from 'stateful'
print "user loaded"

pub fn bump_twice
  bump()
  bump()
end
