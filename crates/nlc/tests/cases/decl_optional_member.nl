# A member of an optional declared type (`Panel?`) is the member of the
# type, under its Lua key, before and after a test narrows the value; and
# a declared `const` narrows like a local that nothing assigns.
# decl: lib/host_api.d.nl
include "lib/host_api.lua"

fn width(p: Panel): Number
  p.wide
end

let p = find_panel("frame")
if p
  print p.remove()
end
print p.wide
let none = find_panel("")
print none?.remove() ?? "no panel"

if PACKAGE
  print PACKAGE.remove()
  print width(PACKAGE)
end
print width(PACKAGE) if PACKAGE
