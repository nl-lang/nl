# Destructuring parameters, with `as` and a type.
fn hit({target, amount: Number} as event)
end
fn pick([first, *rest]: List)
end
