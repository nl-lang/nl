# Imported by lua_extend_declared.nl: adds a method to the declared Gadget,
# which the importing file knows too.
include("include/gadgets.lua")

pub const loaded = true

fn Gadget.twice: String
  "#{self.label}#{self.label}"
end
