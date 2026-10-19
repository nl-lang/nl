# An uncaught error, for the test `uncaught_error_report` of
# runtime_cases.rs: one traceback, with the line of the `raise`.
fn fails(m)
  raise m
end
fails("left uncaught")
