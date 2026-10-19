# `is` takes a full type, a union included; the type ends at
# `and`.
x = v is Number | String and ok
x = v is ~nil
x = v is Box<T>?
