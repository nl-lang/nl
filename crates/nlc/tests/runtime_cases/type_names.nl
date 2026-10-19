# An optional union or negation is named with parentheses.
fn opt_union(x: (Number | String)?)
  x
end
fn opt_neg(x: (~String)?)
  x
end
let table_value: any = {}
let string_value: any = "s"
try
  opt_union(table_value)
catch e: TypeError
  print e.message
end
try
  opt_neg(string_value)
catch e: TypeError
  print e.message
end
