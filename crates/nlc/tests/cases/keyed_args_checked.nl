# Keyed arguments in a checked build: `**opts: T`
# checks the table against a record type, and a function without `**opts`
# refuses keyed arguments that reach one of its parameters. The analyzer
# reports the same at compile time when it knows the callee and the
# value, so the failing calls here go through `any` to reach the runtime
# checks (tests/run/README.md).
# checked-only
fn typed(**opts: {presence: Bool?, inclusion: [String]?})
  print "presence=#{opts.presence}"
end
typed(presence: true)
typed()
let one: any = 1
try
  typed(presence: one)
catch e: TypeError
  print e.message
end

fn plain(a, b)
  print "plain #{a} #{b}"
end
plain(1, 2)
let plain_any: any = plain!
try
  plain_any(1, k: 2)
catch e: TypeError
  print e.message
end
plain_any(1, 2, k: 3)         # beyond the parameters: dropped, like any extra argument
fn with_rest(a, *rest)
  print "with_rest #{a} #{rest.len}"
end
let with_rest_any: any = with_rest!
try
  with_rest_any(1, 2, k: 3, j: 4)
catch e: TypeError
  print e.message
end
let f: any = fn(x) x end
try
  f(k: 1)
catch e: TypeError
  print e.message
end
