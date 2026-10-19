# Keyed arguments: after the positional ones, in both call forms,
# before a trailing block; `**t` passes a table as keyed arguments.
fn validates(field, *rest, **opts)
end
fn only(**opts: {presence: Bool?})
end
validates 'role', presence: true, inclusion: roles
validates('role', presence: true, **extra)
validates 'role', **extra, presence: true
f a, *list, k: 1 do |x| x end
only(k: 1)
only k: 1
each list, k: v do |a, **o| a end
# `x:m` with the `:` touching both sides stays a method call.
print conn:send("hi"), k: 2
# A `Function` signature declares the keyed parameter too.
fn typed(cb: Function(a: Number, **opts: any)): nil
end
