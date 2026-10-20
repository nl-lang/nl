# fn on a declared module adds a plain function to its Lua table, on a declared class a method of its values
import { loaded } from 'import/gadget_extras'

fn Shelf.describe(n: Number): String
  "#{n} of #{Shelf.count()}"
end

fn Gadget.shout: String
  self.label.upcase
end

fn Gadget.rename(name: String)
  @name = name
end

# A function of the module, for NL and for Lua alike.
print Shelf.describe(2)
print :shelf.describe(5)

# A method of the class's values: called in NL as a declared one is, and by
# Lua with `:`.
let g = make_gadget("box")
print g.shout
g.rename "lid"
print g.label
let h: any = make_gadget("cup")
print h.shout
print :lua_shout(g)
print Gadget.shout(make_gadget("jar"))

# A method the imported file added.
print loaded, g.twice

# A host type gets the method in the metatable the host registered for it;
# Widget has none here, so the definition raises when it runs.
try
  fn Widget.size: Number
    1
  end
catch e
  print typeof(e), e.message
end
