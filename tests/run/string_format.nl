# format and rep behave as in Lua
print "%d-%s".format(5, "x")
print "%5.2f".format(3.14159)
print "%03d".format(7)
print String.format("%x", 255)
print "ab".rep(3)
print "-".rep(0) == ""
