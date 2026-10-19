# An imported name has the kind it has in
# its own file; the resolver of the exporting file decides it once.
import { maybe, always, typed, named, Num, Tools, Pt, own_view } from 'lib/export_kinds'
import from 'lib/export_kinds' as K
print own_view
print typeof(maybe)
print maybe()
print always
print typed
print named
fn n(v: Num)
  v
end
print n(4)
print("s" is Num)
# Reopening an imported module is not a clash.
module Tools
  fn more
    "more"
  end
end
print Tools.hi
print K.Tools.more
print Pt { x: 1 }.x
print typeof(K.maybe)
