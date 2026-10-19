# Who owns a primitive method or
# operator. A file is one owner however its name is spelled; each chunk
# of `load_nl` is an owner of its own, so two of them cannot silently
# replace each other's definitions, while one chunk run twice can.
import { loaded } from 'lib/owner_prim'
include("./lib/owner_prim.nl")
print 1.from_owner_lib

let first = load_nl("fn Number.owned_by\n  \"first chunk\"\nend\nfn Bool.+(r)\n  \"first +\"\nend", "plugin")
first()
first()
print 5.owned_by
print(true + 1)
let second = load_nl("fn Number.owned_by\n  \"second chunk\"\nend", "plugin")
try
  second()
catch e
  print e.message
end
let third = load_nl("fn Bool.+(r)\n  \"third +\"\nend", "plugin")
try
  third()
catch e
  print e.message
end
print 5.owned_by
print(true + 1)
