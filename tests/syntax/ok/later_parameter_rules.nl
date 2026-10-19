# A parameter with a default before one without, and
# partial annotations, parse; the resolver reports them.
fn f(a = 1, b)
end
fn g(a: Number, b)
end
