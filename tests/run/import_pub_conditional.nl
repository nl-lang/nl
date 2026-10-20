# pub items in a branch of a file-level if are exported; one whose branch did not run is nil
import { render, STYLE, Theme, debug_dump, describe } from './import/conditional'
import from './import/conditional' as Conditional

print render("hi")
print STYLE
print Theme.color
print describe

# debug_dump may be nil, so it is a plain value: naming it does not call it
print debug_dump == nil
print typeof(debug_dump)
print debug_dump?.() ?? "no debug_dump"

print Conditional.render("whole")
print Conditional.debug_dump == nil
