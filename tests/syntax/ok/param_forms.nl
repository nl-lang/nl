# A parameter is a pattern with an optional default;
# `*name` collects the rest into a table and is the last parameter, also
# after a `Function` parameter that takes a trailing block; a trailing comma
# is allowed.
fn a(x, y: Number, z = 1, w: String = "s",)
end
fn b(*args: Number)
end
fn c(a, b, *args)
end
fn d(a, b, callback: Function, *args)
end
