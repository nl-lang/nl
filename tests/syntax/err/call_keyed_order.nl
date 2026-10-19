# A positional argument or a spread after a keyed one is an error; a second
# `**` parameter, or one that is not last, is an error.
f(k: 1, 2)
f a, k: 1, *list
f(*list, *more)
fn g(**a, b)
end
fn h(**a, **b)
end
fn i(*a, b, **c)
end
