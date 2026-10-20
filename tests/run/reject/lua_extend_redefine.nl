# Rejected by the resolver: decl/interop.d.nl declares Gadget.label.
fn Gadget.shout
  self.label.upcase
end

# expect-error: a function added to a declared type cannot take the name of a member the declaration has
fn Gadget.label
  "label"
end
