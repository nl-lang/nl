# Operators and methods defined on primitives in an imported file, also
# one import away, are known in the importing file
import { joined, ready } from 'import/prim_chain'

print ready
print joined("a", "b")
print "x" + "y"
print "ab" * 3
print "hey".shout
let s: String = "x" + "z"
print s.len
print 2 * "ok"
