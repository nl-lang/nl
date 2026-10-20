# A bare function-kind name passed as an argument is a call, with no arguments
fn now
  42
end

fn label(x)
  "label #{x}"
end

fn twice(f: Function, x: any)
  f(f(x))
end

# A function of no parameter is called on purpose: its result is passed.
print now
print typeof(now)

# `!` passes the function itself.
print typeof(label!)
print twice(label!, 1)

# Without `!` the function is called with nothing, and `x` is nil
# (the analyzer's lint autocall-argument says so).
print typeof(label)

# A parameter that is optional gets nil anyway, so no call is a mistake.
fn greet(name = "you")
  "hi #{name}"
end
print greet
