# An import binds a plain name, so `pub module
# Outer.Inner` cannot be imported; the nested module is exported with
# `Outer`.
module Outer
end
# expect-error: a nested module is exported with the module it belongs to
pub module Outer.Inner
  fn hi
    "hi"
  end
end
