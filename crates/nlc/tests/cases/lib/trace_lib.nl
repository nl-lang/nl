# Imported by trace_calls.nl: a frame of another file in a trace.
pub fn fail_in_lib(n)
  let doubled = n * 2
  raise "lib failed with #{doubled}"
end
