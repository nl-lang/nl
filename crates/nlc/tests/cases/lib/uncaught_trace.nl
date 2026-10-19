# An uncaught error, for the test `uncaught_error_trace` of cases.rs.
fn lookup(table: any, k: Number)
  let v = table[k]
  raise "no value for #{k}" if v == nil
  v
end

fn visit(k: Number, f: Function)
  print "visiting"
  let r = f(k)
  r
end

visit 3 do |k|
  let v = lookup({}, k)
  v
end
