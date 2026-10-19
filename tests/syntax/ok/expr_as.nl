# `as` binds tighter than `*` and takes a
# qualified type name or `Self` with no type arguments.
x = a * b as Number
x = v as Lib.Point
x = v as Self
x = v as A as B
x = v as Number < 10
x = v as T | y
