# Rejected by the compiler.
fn convert<T1, T2>(a: T1, b: T2): T1
  # expect-error: `as` with a type parameter; type parameters do not exist at runtime
  let c = a as T2
  a
end

print convert(1, "x")
